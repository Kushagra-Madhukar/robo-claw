//! E2E pipeline integration test.
//!
//! Traces a request through the full ARIA-X stack using mocks:
//! Gateway → SemanticRouter → AgentOrchestrator → Cedar → Wasm → Response

use aria_core::{AgentRequest, GatewayChannel, MessageContent};
use aria_intelligence::{
    llm_route_fallback, AgentOrchestrator, CachedTool, DynamicToolCache, LLMBackend, LLMResponse,
    LlmBackendPool, LocalHashEmbedder, OrchestratorError, RouteConfig, RouterDecision,
    SemanticRouter, ToolCall, ToolExecutionResult, ToolExecutor, ToolManifestStore,
};
use aria_policy::CedarEvaluator;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Mock LLM: simulates a developer agent
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct MockDeveloperLLM {
    call_count: Arc<std::sync::atomic::AtomicUsize>,
}

impl MockDeveloperLLM {
    fn new() -> Self {
        Self {
            call_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }
}

#[async_trait::async_trait]
impl LLMBackend for MockDeveloperLLM {
    async fn query(
        &self,
        _prompt: &str,
        _tools: &[CachedTool],
    ) -> Result<LLMResponse, OrchestratorError> {
        let n = self
            .call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if n == 0 {
            // Round 1: request directory listing tool call
            Ok(LLMResponse::ToolCalls(vec![ToolCall {
                invocation_id: None,
                name: "list_directory".into(),
                arguments: r#"{"path": "/workspace"}"#.into(),
            }]))
        } else {
            // Round 2: produce final answer from tool results
            Ok(LLMResponse::TextAnswer(
                "The workspace contains: file1.txt, file2.rs".into(),
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Mock Tool Executor: simulates Wasm execution
// ---------------------------------------------------------------------------

struct MockWasmExecutor;

#[async_trait::async_trait]
impl ToolExecutor for MockWasmExecutor {
    async fn execute(&self, call: &ToolCall) -> Result<ToolExecutionResult, OrchestratorError> {
        match call.name.as_str() {
            "list_directory" => Ok(ToolExecutionResult::text("file1.txt, file2.rs")),
            other => Err(OrchestratorError::ToolError(format!(
                "unknown tool: {}",
                other
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// E2E tests
// ---------------------------------------------------------------------------

/// Full pipeline test:
///
/// 1. Inject `AgentRequest("List contents of workspace")`
/// 2. SemanticRouter picks `developer` agent
/// 3. Mock LLM returns `ToolCall("list_directory")`
/// 4. Cedar allows workspace read
/// 5. Mock Wasm returns "file1.txt, file2.rs"
/// 6. Mock LLM (round 2) returns final TextAnswer
/// 7. Assert output contains file listing
#[tokio::test]
async fn full_pipeline_mocked() {
    // ---- Step 1: Simulate gateway normalization ----
    let user_query = "List contents of workspace";

    // ---- Step 2: Semantic router picks developer agent ----
    let mut router = SemanticRouter::new();
    router
        .register_agent("developer", vec![0.9, 0.8, 0.1, 0.05])
        .expect("register developer");
    router
        .register_agent("analyst", vec![0.1, 0.05, 0.9, 0.85])
        .expect("register analyst");

    // Query embedding close to developer
    let query_embedding = vec![0.85, 0.75, 0.15, 0.1];
    let (agent_name, score) = router
        .route(&query_embedding)
        .expect("route should succeed");

    assert_eq!(agent_name, "developer", "should route to developer agent");
    assert!(score > 0.9, "score should be high");

    // ---- Step 3-6: Orchestrator ReAct loop ----
    let llm = MockDeveloperLLM::new();
    let executor = MockWasmExecutor;
    let orchestrator = AgentOrchestrator::new(llm, executor);

    let tools = vec![CachedTool {
        name: "list_directory".into(),
        description: "List files in a directory".into(),
        parameters_schema: r#"{"path": "string"}"#.into(),
        embedding: Vec::new(),
        requires_strict_schema: false,
        streaming_safe: false,
        parallel_safe: true,
        modalities: vec![aria_core::ToolModality::Text],
    }];

    let final_answer = orchestrator
        .run(user_query, &tools, 5, None, None)
        .await
        .expect("orchestrator should succeed");

    // ---- Step 4: Cedar policy check ----
    let policy_text = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../aria-policy/policies/default.cedar"),
    )
    .expect("should read default policy");

    let cedar = CedarEvaluator::from_policy_str(&policy_text).expect("should parse policy");

    let decision = cedar
        .evaluate("developer", "read_file", "/workspace/src")
        .expect("evaluation should succeed");

    assert_eq!(
        decision,
        aria_policy::Decision::Allow,
        "Cedar should allow developer to read_file in workspace"
    );

    // ---- Step 7: Verify final output ----
    let final_answer_str = match final_answer {
        aria_intelligence::OrchestratorResult::Completed(t) => t,
        _ => panic!("Expected completed text"),
    };

    assert!(
        final_answer_str.contains("file1.txt"),
        "output should contain file1.txt, got: {}",
        final_answer_str
    );
    assert!(
        final_answer_str.contains("file2.rs"),
        "output should contain file2.rs, got: {}",
        final_answer_str
    );

    // Verify Cedar denies system paths
    let deny_decision = cedar
        .evaluate("developer", "read_file", "/etc/shadow")
        .expect("evaluation should succeed");
    assert_eq!(
        deny_decision,
        aria_policy::Decision::Deny,
        "Cedar should deny reading /etc/shadow"
    );
}

/// Verify the orchestrator aborts on infinite tool loops.
#[tokio::test]
async fn pipeline_infinite_loop_prevention() {
    #[derive(Clone)]
    struct AlwaysToolCallLLM;

    #[async_trait::async_trait]
    impl LLMBackend for AlwaysToolCallLLM {
        async fn query(
            &self,
            _prompt: &str,
            _tools: &[CachedTool],
        ) -> Result<LLMResponse, OrchestratorError> {
            Ok(LLMResponse::ToolCalls(vec![ToolCall {
                invocation_id: None,
                name: "list_directory".into(),
                arguments: "{}".into(),
            }]))
        }
    }

    let orchestrator = AgentOrchestrator::new(AlwaysToolCallLLM, MockWasmExecutor);

    let result = orchestrator
        .run("infinite loop test", &[], 5, None, None)
        .await;

    assert!(result.is_err(), "should fail with max rounds exceeded");
    match result {
        Err(OrchestratorError::MaxRoundsExceeded { limit: 5 }) => {}
        other => panic!("expected MaxRoundsExceeded, got: {:?}", other),
    }
}

/// Verify Telegram gateway normalization works in the integration context.
#[test]
fn gateway_normalization_integration() {
    let telegram_json = r#"{
        "update_id": 999,
        "message": {
            "message_id": 1,
            "from": {"id": 42, "first_name": "Dev"},
            "chat": {"id": 100, "type": "private"},
            "text": "List contents of workspace",
            "date": 1700000000
        }
    }"#;

    let request =
        aria_gateway::TelegramNormalizer::normalize(telegram_json).expect("should normalize");

    assert_eq!(
        request.content,
        MessageContent::Text("List contents of workspace".into())
    );
    assert_eq!(request.user_id, "42");
    assert_eq!(request.timestamp_us, 1700000000 * 1_000_000);
}

#[tokio::test]
async fn router_fallback_integration_path() {
    #[derive(Clone)]
    struct RouteChoiceLLM;

    #[async_trait::async_trait]
    impl LLMBackend for RouteChoiceLLM {
        async fn query(
            &self,
            _prompt: &str,
            _tools: &[CachedTool],
        ) -> Result<LLMResponse, OrchestratorError> {
            Ok(LLMResponse::TextAnswer("analyst".into()))
        }
    }

    let mut router = SemanticRouter::new();
    router
        .register_agent("developer", vec![0.8, 0.7, 0.1, 0.05])
        .expect("register developer");
    router
        .register_agent("analyst", vec![0.75, 0.72, 0.1, 0.05])
        .expect("register analyst");
    let cfg = RouteConfig {
        confidence_threshold: 0.95,
        tie_break_gap: 0.10,
    };
    let decision = router
        .route_with_config(&[0.79, 0.71, 0.1, 0.05], cfg)
        .expect("route should succeed");
    let candidates = match decision {
        RouterDecision::NeedsLlmFallback { candidates } => candidates,
        other => panic!("expected fallback decision, got {:?}", other),
    };

    let chosen = llm_route_fallback(&RouteChoiceLLM, "who should handle this?", &candidates)
        .await
        .expect("fallback llm should pick candidate");
    assert_eq!(chosen, "analyst");
}

#[tokio::test]
async fn backend_failover_integration_path() {
    #[derive(Clone)]
    struct PrimaryFailingLLM {
        calls: Arc<AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl LLMBackend for PrimaryFailingLLM {
        async fn query(
            &self,
            _prompt: &str,
            _tools: &[CachedTool],
        ) -> Result<LLMResponse, OrchestratorError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(OrchestratorError::LLMError("primary unavailable".into()))
        }
    }

    #[derive(Clone)]
    struct FallbackAnswerLLM;
    #[async_trait::async_trait]
    impl LLMBackend for FallbackAnswerLLM {
        async fn query(
            &self,
            _prompt: &str,
            _tools: &[CachedTool],
        ) -> Result<LLMResponse, OrchestratorError> {
            Ok(LLMResponse::TextAnswer("fallback succeeded".into()))
        }
    }

    #[derive(Clone)]
    struct PoolBackedLLM {
        pool: Arc<LlmBackendPool>,
    }
    #[async_trait::async_trait]
    impl LLMBackend for PoolBackedLLM {
        async fn query(
            &self,
            prompt: &str,
            tools: &[CachedTool],
        ) -> Result<LLMResponse, OrchestratorError> {
            self.pool.query_with_fallback(prompt, tools).await
        }
    }

    let primary_calls = Arc::new(AtomicUsize::new(0));
    let pool = LlmBackendPool::new(
        vec!["primary".into(), "fallback".into()],
        Duration::from_millis(50),
    );
    pool.register_backend(
        "primary",
        Box::new(PrimaryFailingLLM {
            calls: primary_calls.clone(),
        }),
    );
    pool.register_backend("fallback", Box::new(FallbackAnswerLLM));
    let pool = Arc::new(pool);

    let orchestrator = AgentOrchestrator::new(PoolBackedLLM { pool }, MockWasmExecutor);
    let answer = orchestrator
        .run("produce answer", &[], 2, None, None)
        .await
        .expect("fallback backend should succeed");

    let answer_str = match answer {
        aria_intelligence::OrchestratorResult::Completed(t) => t,
        _ => panic!("Expected completed text"),
    };
    assert_eq!(answer_str, "fallback succeeded");
    assert_eq!(primary_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn dynamic_tool_hotswap_integration_path() {
    #[derive(Clone)]
    struct DynamicSwapLLM {
        step: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl LLMBackend for DynamicSwapLLM {
        async fn query(
            &self,
            _prompt: &str,
            _tools: &[CachedTool],
        ) -> Result<LLMResponse, OrchestratorError> {
            match self.step.fetch_add(1, Ordering::SeqCst) {
                0 => Ok(LLMResponse::ToolCalls(vec![ToolCall {
                    invocation_id: None,
                    name: "search_tool_registry".into(),
                    arguments: r#"{"query":"sensor telemetry read"}"#.into(),
                }])),
                1 => Ok(LLMResponse::ToolCalls(vec![ToolCall {
                    invocation_id: None,
                    name: "read_sensor".into(),
                    arguments: r#"{"sensor":"imu"}"#.into(),
                }])),
                _ => Ok(LLMResponse::TextAnswer("imu=nominal".into())),
            }
        }
    }

    struct SensorExecutor;
    #[async_trait::async_trait]
    impl ToolExecutor for SensorExecutor {
        async fn execute(&self, call: &ToolCall) -> Result<ToolExecutionResult, OrchestratorError> {
            match call.name.as_str() {
                "read_sensor" => Ok(ToolExecutionResult::text("imu=nominal")),
                other => Err(OrchestratorError::ToolError(format!(
                    "unexpected tool execution: {}",
                    other
                ))),
            }
        }
    }

    let llm = DynamicSwapLLM {
        step: Arc::new(AtomicUsize::new(0)),
    };
    let orchestrator = AgentOrchestrator::new(llm, SensorExecutor);
    let embedder = LocalHashEmbedder::new(64);
    let mut registry = ToolManifestStore::new();
    registry.register(CachedTool {
        name: "read_sensor".into(),
        description: "Read telemetry from on-device sensors".into(),
        parameters_schema: r#"{"sensor":"string"}"#.into(),
        embedding: Vec::new(),
        requires_strict_schema: false,
        streaming_safe: false,
        parallel_safe: true,
        modalities: vec![aria_core::ToolModality::Text],
    });
    let mut cache = DynamicToolCache::new(8, 15);
    cache
        .insert(CachedTool {
            name: "search_tool_registry".into(),
            description: "Search and hot-swap tools by semantic query".into(),
            parameters_schema: r#"{"query":"string"}"#.into(),
            embedding: Vec::new(),
            requires_strict_schema: false,
            streaming_safe: false,
            parallel_safe: true,
            modalities: vec![aria_core::ToolModality::Text],
        })
        .expect("insert meta tool");

    let req = AgentRequest {
        request_id: uuid::Uuid::new_v4().into_bytes(),
        session_id: uuid::Uuid::new_v4().into_bytes(),
        channel: GatewayChannel::Cli,
        user_id: "integration-user".into(),
        content: MessageContent::Text("check imu health".into()),
        tool_runtime_policy: None,
        timestamp_us: 1_700_000_000_000_000,
    };

    let answer = orchestrator
        .run_for_request_with_dynamic_tools(aria_intelligence::DynamicRunContext {
            agent_system_prompt: "sys",
            request: &req,
            history_context: "",
            rag_context: "",
            history_messages: &[],
            context_blocks: &[],
            prompt_tools: None,
            tool_selection: None,
            cache: &mut cache,
            tool_registry: &registry,
            embedder: &embedder,
            max_tool_rounds: 5,
            model_capability: None,
            steering_rx: None,
            global_estop: None,
        })
        .await
        .expect("dynamic hot-swap flow should succeed");

    let answer_str = match answer {
        aria_intelligence::OrchestratorResult::Completed(t) => t,
        _ => panic!("Expected completed text"),
    };
    assert_eq!(answer_str, "imu=nominal");
    assert!(
        cache.get("read_sensor").is_some(),
        "tool should be loaded in cache"
    );
}
