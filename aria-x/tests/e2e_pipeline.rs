//! E2E pipeline integration test.
//!
//! Traces a request through the full ARIA-X stack using mocks:
//! Gateway → SemanticRouter → AgentOrchestrator → Cedar → Wasm → Response

use aria_intelligence::{
    AgentOrchestrator, CachedTool, LLMBackend, LLMResponse, OrchestratorError, SemanticRouter,
    ToolCall, ToolExecutor,
};
use aria_policy::CedarEvaluator;

// ---------------------------------------------------------------------------
// Mock LLM: simulates a developer agent
// ---------------------------------------------------------------------------

struct MockDeveloperLLM {
    call_count: std::sync::atomic::AtomicUsize,
}

impl MockDeveloperLLM {
    fn new() -> Self {
        Self {
            call_count: std::sync::atomic::AtomicUsize::new(0),
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
    async fn execute(&self, call: &ToolCall) -> Result<String, OrchestratorError> {
        match call.name.as_str() {
            "list_directory" => Ok("file1.txt, file2.rs".into()),
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
    }];

    let final_answer = orchestrator
        .run(user_query, &tools, 5)
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
    assert!(
        final_answer.contains("file1.txt"),
        "output should contain file1.txt, got: {}",
        final_answer
    );
    assert!(
        final_answer.contains("file2.rs"),
        "output should contain file2.rs, got: {}",
        final_answer
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
    struct AlwaysToolCallLLM;

    #[async_trait::async_trait]
    impl LLMBackend for AlwaysToolCallLLM {
        async fn query(
            &self,
            _prompt: &str,
            _tools: &[CachedTool],
        ) -> Result<LLMResponse, OrchestratorError> {
            Ok(LLMResponse::ToolCalls(vec![ToolCall {
                name: "list_directory".into(),
                arguments: "{}".into(),
            }]))
        }
    }

    let orchestrator = AgentOrchestrator::new(AlwaysToolCallLLM, MockWasmExecutor);

    let result = orchestrator.run("infinite loop test", &[], 5).await;

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

    assert_eq!(request.content, "List contents of workspace");
    assert_eq!(request.user_id, "42");
    assert_eq!(request.timestamp_us, 1700000000 * 1_000_000);
}
