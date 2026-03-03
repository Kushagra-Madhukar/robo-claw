//! ARIA-X Orchestrator — the final binary that wires all crates together.
//!
//! Reads TOML configuration, initializes all subsystems, and runs the
//! ReAct agent loop with graceful SIGINT shutdown via a CLI gateway.

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;

use aria_core::AgentRequest;
use aria_gateway::{GatewayAdapter, GatewayError};
use aria_intelligence::{
    AgentOrchestrator, CachedTool, LLMBackend, LLMResponse, LocalHashEmbedder, OrchestratorError,
    SemanticRouter, ToolCall, ToolExecutor,
};
use aria_ssmu::{PageIndexTree, PageNode};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Top-level TOML configuration.
#[derive(Debug, Deserialize)]
pub struct Config {
    pub llm: LlmConfig,
    pub policy: PolicyConfig,
    pub gateway: GatewayConfig,
    pub mesh: MeshConfig,
}

#[derive(Debug, Deserialize)]
pub struct LlmConfig {
    pub backend: String,
    pub model: String,
    pub max_tool_rounds: usize,
}

#[derive(Debug, Deserialize)]
pub struct PolicyConfig {
    pub policy_path: String,
}

#[derive(Debug, Deserialize)]
pub struct GatewayConfig {
    pub adapter: String,
}

#[derive(Debug, Deserialize)]
pub struct MeshConfig {
    pub mode: String,
    pub endpoints: Vec<String>,
}

fn load_config(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

// ---------------------------------------------------------------------------
// Gateway & Mock Adapters
// ---------------------------------------------------------------------------

struct CliGateway;

#[async_trait::async_trait]
impl GatewayAdapter for CliGateway {
    async fn receive(&self) -> Result<AgentRequest, GatewayError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            print!("\n[aria-x] Enter request > ");
            let _ = io::stdout().flush();
            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_ok() {
                let _ = tx.send(input.trim().to_string());
            }
        });

        let content = rx.await.unwrap_or_default();
        if content.is_empty() {
            // Keep the loop alive if empty line
            return Err(GatewayError::TransportError("Empty input".into()));
        }

        Ok(AgentRequest {
            request_id: [0u8; 16],
            session_id: [0u8; 16],
            user_id: "cli_user".into(),
            content,
            timestamp_us: 0,
        })
    }
}

struct LocalMockLLM;

#[async_trait::async_trait]
impl LLMBackend for LocalMockLLM {
    async fn query(
        &self,
        prompt: &str,
        _tools: &[CachedTool],
    ) -> Result<LLMResponse, OrchestratorError> {
        Ok(LLMResponse::TextAnswer(format!(
            "Echo LLM: I processed '{}'",
            prompt
        )))
    }
}

struct LocalWasmExecutor;

#[async_trait::async_trait]
impl ToolExecutor for LocalWasmExecutor {
    async fn execute(&self, call: &ToolCall) -> Result<String, OrchestratorError> {
        Ok(format!("Executed: {}", call.name))
    }
}

struct PolicyCheckedExecutor<T: ToolExecutor> {
    inner: T,
    cedar: Arc<aria_policy::CedarEvaluator>,
    principal: String,
}

impl<T: ToolExecutor> PolicyCheckedExecutor<T> {
    fn new(inner: T, cedar: Arc<aria_policy::CedarEvaluator>, principal: String) -> Self {
        Self {
            inner,
            cedar,
            principal,
        }
    }

    fn to_ast_call(call: &ToolCall) -> String {
        let mut ast_args = Vec::new();
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&call.arguments) {
            if let Some(obj) = value.as_object() {
                for (k, v) in obj {
                    let v_str = if let Some(s) = v.as_str() {
                        s.to_string()
                    } else {
                        v.to_string()
                    };
                    let escaped = v_str.replace('"', "\\\"");
                    ast_args.push(format!(r#"{}="{}""#, k, escaped));
                }
            }
        }
        format!("{}({})", call.name, ast_args.join(", "))
    }
}

#[async_trait::async_trait]
impl<T: ToolExecutor> ToolExecutor for PolicyCheckedExecutor<T> {
    async fn execute(&self, call: &ToolCall) -> Result<String, OrchestratorError> {
        let parsed = aria_policy::parse_ast_action(&Self::to_ast_call(call))
            .map_err(|e| OrchestratorError::ToolError(format!("policy AST parse failed: {}", e)))?;
        let decision = self
            .cedar
            .evaluate(&self.principal, &parsed.action, &parsed.resource)
            .map_err(|e| {
                OrchestratorError::ToolError(format!("policy evaluation failed: {}", e))
            })?;
        if decision == aria_policy::Decision::Deny {
            return Err(OrchestratorError::ToolError(format!(
                "tool '{}' denied by policy for resource '{}'",
                parsed.action, parsed.resource
            )));
        }
        self.inner.execute(call).await
    }
}

fn build_bootstrap_page_index() -> PageIndexTree {
    let mut tree = PageIndexTree::new(32);
    let seed_nodes = vec![
        PageNode {
            node_id: "workspace.files".into(),
            title: "Workspace Files".into(),
            summary: "Tools and workflows for listing files and reading project source code."
                .into(),
            start_index: 0,
            end_index: 1,
            children: vec!["workspace.rust".into()],
        },
        PageNode {
            node_id: "workspace.rust".into(),
            title: "Rust Build and Test".into(),
            summary: "Cargo build and cargo test commands for validating Rust crates.".into(),
            start_index: 1,
            end_index: 2,
            children: vec![],
        },
        PageNode {
            node_id: "security.policy".into(),
            title: "Cedar Policy Constraints".into(),
            summary: "Authorization decisions for workspace access and denied system paths.".into(),
            start_index: 2,
            end_index: 3,
            children: vec![],
        },
    ];

    for node in seed_nodes {
        let _ = tree.insert(node);
    }
    tree
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let config_path = std::env::args().nth(1).unwrap_or_else(|| {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir
            .join("config.toml")
            .to_string_lossy()
            .to_string()
    });

    println!("[aria-x] Loading config from: {}", config_path);

    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[aria-x] Failed to load config: {}", e);
            std::process::exit(1);
        }
    };

    println!(
        "[aria-x] Config loaded (LLM: {}/{})",
        config.llm.backend, config.llm.model
    );

    // Initialize Cedar policy engine
    let policy_content = std::fs::read_to_string(&config.policy.policy_path).unwrap_or_default();
    let cedar = aria_policy::CedarEvaluator::from_policy_str(&policy_content)
        .unwrap_or_else(|_| {
            println!("[aria-x] Warning: Failed to parse Cedar policies, proceeding without strict policy enforcement.");
            aria_policy::CedarEvaluator::from_policy_str("").unwrap()
        });
    let cedar = Arc::new(cedar);

    // Initialize Semantic Router
    let embedder = LocalHashEmbedder::new(64);
    let mut router = SemanticRouter::new();
    let _ = router.register_agent_text(
        "developer",
        "rust code workspace build test list file project source",
        &embedder,
    );
    let _ = router.register_agent_text(
        "analyst",
        "finance stock market revenue quarterly report analysis",
        &embedder,
    );

    // Initialize Session Memory
    let session_memory = aria_ssmu::SessionMemory::new(100);
    // PageIndex retrieval is primary for context assembly.
    let page_index = build_bootstrap_page_index();

    // Wire Adapters
    let gateway = CliGateway;
    let tools = vec![]; // Configured tools would go here

    println!("[aria-x] ✅ All subsystems wired (Gateway → Router → Orchestrator → Exec)");
    println!("[aria-x] Interactive CLI started. (press Ctrl+C to exit)");

    let sigint = tokio::signal::ctrl_c();
    tokio::pin!(sigint);

    loop {
        tokio::select! {
            _ = &mut sigint => {
                println!("\n[aria-x] Received SIGINT — shutting down gracefully");
                break;
            }
            req_res = gateway.receive() => {
                let req = match req_res {
                    Ok(r) => r,
                    Err(_) => continue, // Ignore empty lines or minor errors
                };
                if req.content.eq_ignore_ascii_case("exit") {
                    println!("[aria-x] Exiting...");
                    break;
                }

                // Route via Semantic Router using a local, no-network embedder.
                let (agent, _) = router
                    .route_text(&req.content, &embedder)
                    .unwrap_or(("developer".into(), 1.0));
                println!("[aria-x] 🔀 Routed to agent: {}", agent);
                let policy_executor =
                    PolicyCheckedExecutor::new(LocalWasmExecutor, cedar.clone(), agent.clone());
                let orchestrator = AgentOrchestrator::new(LocalMockLLM, policy_executor);

                // ReAct Orchestrator loop
                let mut history_ctx = String::new();
                let session_uuid = uuid::Uuid::from_bytes(req.session_id);
                if let Ok(hist) = session_memory.get_history(&session_uuid) {
                    for m in hist {
                        history_ctx.push_str(&format!("{}: {}\n", m.role, m.content));
                    }
                }
                let page_context = page_index
                    .retrieve_relevant(&req.content, 3)
                    .into_iter()
                    .map(|n| format!("- {}: {}", n.title, n.summary))
                    .collect::<Vec<_>>()
                    .join("\n");
                let contextual_prompt = format!(
                    "PageIndex Context:\n{}\n\nSession History:\n{}\nUser request: {}",
                    page_context,
                    history_ctx,
                    req.content
                );
                session_memory.append(session_uuid, aria_ssmu::Message {
                    role: "user".into(),
                    content: req.content.clone(),
                    timestamp_us: req.timestamp_us,
                }).ok();
                match orchestrator.run(&contextual_prompt, &tools, config.llm.max_tool_rounds).await {
                    Ok(response) => {
                        println!("[aria-x] 🤖 Agent response: {}", response);
                    }
                    Err(e) => {
                        eprintln!("[aria-x] ❌ Orchestrator error: {}", e);
                    }
                }
            }
        }
    }

    // Cleanup
    drop(session_memory);
    drop(page_index);
    drop(router);
    println!("[aria-x] Shutdown complete. Goodbye!");
}
