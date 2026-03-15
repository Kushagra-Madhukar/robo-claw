use super::*;
use aria_core::{ExecutionContextPack, GatewayChannel};

fn tool_failure_result(call: &ToolCall, error: &str) -> ToolExecutionResult {
    ToolExecutionResult::structured(
        format!("Tool '{}' failed: {}", call.name, error),
        "tool_error",
        serde_json::json!({
            "ok": false,
            "tool": call.name,
            "error": error,
        }),
    )
}

// ---------------------------------------------------------------------------
// AgentOrchestrator
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestratorResult {
    /// The agent finished its reasoning and generated a final text response.
    Completed(String),
    /// The resolved agent is privileged and requires an explicit session-scoped elevation grant.
    AgentElevationRequired { agent_id: String, message: String },
    /// The agent requested a tool that requires human approval.
    /// Contains the pending tool call and the current aggregated reasoning prompt.
    ToolApprovalRequired {
        call: ToolCall,
        pending_prompt: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestratorEvent {
    RepairFallbackUsed {
        tool_name: String,
        model_ref: Option<String>,
    },
    StreamingDecision {
        phase: &'static str,
        mode: &'static str,
        model_ref: Option<String>,
    },
}

pub trait OrchestratorEventSink: Send + Sync {
    fn on_event(&self, event: &OrchestratorEvent);
}

/// ReAct (Reasoning and Acting) agent orchestrator.
///
/// Implements the core loop:
/// 1. Query LLM with prompt + tools
/// 2. If `TextAnswer` → return final response
/// 3. If `ToolCalls` → execute each tool, append results, re-query
/// 4. If rounds exceed `max_tool_rounds` → abort with error
pub struct AgentOrchestrator<L: LLMBackend, T: ToolExecutor> {
    llm: L,
    tool_executor: T,
    tool_runtime_policy: ToolRuntimePolicy,
    allow_repair_fallback: bool,
    event_sink: Option<Arc<dyn OrchestratorEventSink>>,
}

/// Commands injected by the human operator mid-flight to alter agent reasoning
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteeringCommand {
    /// Human sent a new message that should abort the current tool plan and pivot
    Pivot(String),
    /// Instantly halt evaluation
    Abort,
}

pub(crate) struct ToolLoopProgress<'a> {
    pub(crate) rounds: &'a mut usize,
    pub(crate) max_tool_rounds: usize,
    pub(crate) prompt: &'a mut String,
    pub(crate) context_pack: &'a mut aria_core::ExecutionContextPack,
    pub(crate) uses_prompt_override: &'a mut bool,
    pub(crate) last_progress: &'a mut Instant,
}

impl ToolLoopProgress<'_> {
    fn advance_round(&mut self) -> Result<(), OrchestratorError> {
        *self.rounds += 1;
        if *self.rounds > self.max_tool_rounds {
            return Err(OrchestratorError::MaxRoundsExceeded {
                limit: self.max_tool_rounds,
            });
        }
        Ok(())
    }
}

pub(crate) struct GenericToolLoopContext<'a> {
    pub(crate) tools_cache: &'a [CachedTool],
    pub(crate) tool_runtime_policy: &'a ToolRuntimePolicy,
    pub(crate) steering_rx: Option<&'a mut tokio::sync::mpsc::Receiver<SteeringCommand>>,
}

pub struct DynamicRunContext<'a, E: EmbeddingModel> {
    pub agent_system_prompt: &'a str,
    pub request: &'a AgentRequest,
    pub history_context: &'a str,
    pub rag_context: &'a str,
    pub history_messages: &'a [aria_core::PromptContextMessage],
    pub context_blocks: &'a [aria_core::ContextBlock],
    pub prompt_tools: Option<&'a [CachedTool]>,
    pub tool_selection: Option<&'a aria_core::ToolSelectionDecision>,
    pub cache: &'a mut DynamicToolCache,
    pub tool_registry: &'a ToolManifestStore,
    pub embedder: &'a E,
    pub max_tool_rounds: usize,
    pub model_capability: Option<&'a ModelCapabilityProfile>,
    pub steering_rx: Option<&'a mut tokio::sync::mpsc::Receiver<SteeringCommand>>,
    pub global_estop: Option<&'a Arc<std::sync::atomic::AtomicBool>>,
}

struct DynamicToolLoopContext<'a, E: EmbeddingModel> {
    steering_rx: Option<&'a mut tokio::sync::mpsc::Receiver<SteeringCommand>>,
    global_estop: Option<&'a std::sync::atomic::AtomicBool>,
    cache: &'a mut DynamicToolCache,
    tool_registry: &'a ToolManifestStore,
    embedder: &'a E,
    model_capability: Option<&'a ModelCapabilityProfile>,
    tool_runtime_policy: &'a ToolRuntimePolicy,
}

fn tool_selection_requires_tool_round(decision: Option<&aria_core::ToolSelectionDecision>) -> bool {
    let Some(decision) = decision else {
        return false;
    };
    matches!(
        decision.tool_calling_mode,
        aria_core::ToolCallingMode::TextFallbackWithRepair
    ) && !decision.selected_tool_names.is_empty()
}

fn selected_tool_names_for_obligation(
    decision: Option<&aria_core::ToolSelectionDecision>,
    active_tools: &[CachedTool],
) -> Vec<String> {
    if let Some(decision) = decision {
        if !decision.selected_tool_names.is_empty() {
            return decision.selected_tool_names.clone();
        }
        if !decision.available_tool_names.is_empty() {
            return decision.available_tool_names.clone();
        }
    }
    active_tools.iter().map(|tool| tool.name.clone()).collect()
}

impl<L: LLMBackend, T: ToolExecutor> AgentOrchestrator<L, T> {
    /// Create a new orchestrator with the given LLM and tool executor.
    pub fn new(llm: L, tool_executor: T) -> Self {
        Self {
            llm,
            tool_executor,
            tool_runtime_policy: ToolRuntimePolicy::default(),
            allow_repair_fallback: false,
            event_sink: None,
        }
    }

    pub fn with_tool_runtime_policy(mut self, tool_runtime_policy: ToolRuntimePolicy) -> Self {
        self.tool_runtime_policy = tool_runtime_policy;
        self
    }

    pub fn with_repair_fallback(mut self, allow_repair_fallback: bool) -> Self {
        self.allow_repair_fallback = allow_repair_fallback;
        self
    }

    pub fn with_event_sink(mut self, event_sink: Arc<dyn OrchestratorEventSink>) -> Self {
        self.event_sink = Some(event_sink);
        self
    }

    fn effective_tool_runtime_policy(&self, request: Option<&AgentRequest>) -> ToolRuntimePolicy {
        request
            .and_then(|req| req.tool_runtime_policy.clone())
            .unwrap_or_else(|| self.tool_runtime_policy.clone())
    }

    fn repair_fallback_mode(&self) -> ToolCallingMode {
        tool_calling_mode_for_model_with_repair(
            self.llm.capability_profile().as_ref(),
            self.allow_repair_fallback,
        )
    }

    fn repair_fallback_permitted(&self) -> bool {
        match self.llm.capability_profile() {
            Some(_) => matches!(
                self.repair_fallback_mode(),
                ToolCallingMode::TextFallbackWithRepair
            ),
            None => true,
        }
    }

    fn emit_event(&self, event: OrchestratorEvent) {
        if let Some(sink) = &self.event_sink {
            sink.on_event(&event);
        }
    }

    fn repaired_tool_call_from_text(&self, answer: &str, tools: &[CachedTool]) -> Option<ToolCall> {
        let repaired = repair_tool_call_json(answer, tools)?;
        self.emit_event(OrchestratorEvent::RepairFallbackUsed {
            tool_name: repaired.name.clone(),
            model_ref: self.llm.model_ref().map(|model| model.as_slash_ref()),
        });
        Some(repaired)
    }

    fn should_use_streaming(&self, tools: &[CachedTool]) -> bool {
        let Some(profile) = self.llm.capability_profile() else {
            return false;
        };
        if !matches!(profile.streaming, aria_core::CapabilitySupport::Supported) {
            return false;
        }
        tools.is_empty() || tools.iter().all(|tool| tool.streaming_safe)
    }

    fn model_ref_label(&self) -> Option<String> {
        self.llm.model_ref().map(|model| model.as_slash_ref())
    }

    async fn execute_tool_calls(
        &self,
        calls: Vec<ToolCall>,
        available_tools: &[CachedTool],
        tool_runtime_policy: &ToolRuntimePolicy,
        mut steering_rx: Option<&mut tokio::sync::mpsc::Receiver<SteeringCommand>>,
        prompt: &mut String,
    ) -> Result<Vec<(ToolCall, Result<ToolExecutionResult, OrchestratorError>)>, OrchestratorError>
    {
        let all_parallel_safe = calls.iter().all(|call| {
            let metadata_parallel_safe = available_tools
                .iter()
                .find(|tool| tool.name == call.name)
                .map(|tool| tool.parallel_safe)
                .unwrap_or(true);
            matches!(
                classify_tool_execution(&call.name),
                ToolExecutionClass::ParallelSafe
            ) && metadata_parallel_safe
        });

        if tool_runtime_policy.allow_parallel_tool_calls && all_parallel_safe {
            let executions = calls.into_iter().map(|call| async move {
                let result = self.tool_executor.execute(&call).await;
                (call, result)
            });
            let executions_future = join_all(executions);
            return if let Some(rx) = steering_rx.as_mut() {
                tokio::select! {
                    res = executions_future => Ok(res),
                    cmd = rx.recv() => {
                        match cmd {
                            Some(SteeringCommand::Abort) => Err(OrchestratorError::UserAborted),
                            Some(SteeringCommand::Pivot(new_instructions)) => {
                                debug!("Orchestrator: user pivot intercepted during tool execution");
                                *prompt = format!("{}\n\n<<SYSTEM INTERRUPT during tool execution: User steered with: '{}'>>\nIgnore the pending tools.", prompt, new_instructions);
                                Ok(Vec::new())
                            }
                            None => Err(OrchestratorError::UserAborted),
                        }
                    }
                }
            } else {
                Ok(executions_future.await)
            };
        }

        let mut outputs = Vec::new();
        for call in calls {
            if let Some(rx) = steering_rx.as_mut() {
                if let Ok(cmd) = rx.try_recv() {
                    match cmd {
                        SteeringCommand::Abort => return Err(OrchestratorError::UserAborted),
                        SteeringCommand::Pivot(new_instructions) => {
                            debug!(
                                "Orchestrator: user pivot intercepted before serial tool execution"
                            );
                            *prompt = format!("{}\n\n<<SYSTEM INTERRUPT during serial tool execution: User steered with: '{}'>>\nIgnore the pending tools.", prompt, new_instructions);
                            return Ok(Vec::new());
                        }
                    }
                }
            }
            let result = self.tool_executor.execute(&call).await;
            outputs.push((call, result));
        }
        Ok(outputs)
    }

    /// Run the ReAct loop.
    ///
    /// - `initial_prompt`: The user's query or assembled prompt
    /// - `tools`: Available tools for this session
    /// - `max_tool_rounds`: Maximum number of tool-call iterations
    ///
    /// Returns the final text answer from the LLM.
    pub async fn run(
        &self,
        initial_prompt: &str,
        tools: &[CachedTool],
        max_tool_rounds: usize,
        steering_rx: Option<&mut tokio::sync::mpsc::Receiver<SteeringCommand>>,
        global_estop: Option<&Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<OrchestratorResult, OrchestratorError> {
        self.run_with_tool_runtime_policy(
            initial_prompt,
            tools,
            max_tool_rounds,
            &self.tool_runtime_policy,
            steering_rx,
            global_estop,
        )
        .await
    }

    async fn run_with_tool_runtime_policy(
        &self,
        initial_prompt: &str,
        tools: &[CachedTool],
        max_tool_rounds: usize,
        tool_runtime_policy: &ToolRuntimePolicy,
        mut steering_rx: Option<&mut tokio::sync::mpsc::Receiver<SteeringCommand>>,
        global_estop: Option<&Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<OrchestratorResult, OrchestratorError> {
        let mut prompt = initial_prompt.to_string();
        let mut rounds = 0_usize;
        let mut last_progress = std::time::Instant::now();

        loop {
            // Check global emergency stop flag
            if let Some(estop) = global_estop {
                if estop.load(std::sync::atomic::Ordering::SeqCst) {
                    debug!("Orchestrator: Global ESTOP triggered, aborting run loop");
                    return Err(OrchestratorError::UserAborted);
                }
            }

            // Check for stuck jobs: if no progress in the last 60 seconds, inject a correction prompt.
            if rounds > 0 && last_progress.elapsed().as_secs() > 60 {
                debug!("Orchestrator: Job heartbeat stalled. Injecting correction prompt.");
                prompt.push_str("\n\n[SYSTEM CORRECTIVE: Your previous approach has stalled for over 60 seconds without resolving the query. Please pivot and try a different strategy or tool immediately.]");
                last_progress = std::time::Instant::now(); // reset heartbeat after injection
            }

            // Check if we have an override signal before querying the LLM
            if let Some(rx) = steering_rx.as_deref_mut() {
                if let Ok(cmd) = rx.try_recv() {
                    match cmd {
                        SteeringCommand::Abort => return Err(OrchestratorError::UserAborted),
                        SteeringCommand::Pivot(new_instructions) => {
                            debug!("Orchestrator: user pivot detected mid-flight");
                            prompt = format!("{}\n\n<<SYSTEM INTERRUPT: User provided new steerings: '{}'>>\nAbort your previous tool plan and restart reasoning.", prompt, new_instructions);
                        }
                    }
                }
            }
            let response = if self.should_use_streaming(tools) {
                self.emit_event(OrchestratorEvent::StreamingDecision {
                    phase: "initial",
                    mode: "stream_attempt",
                    model_ref: self.model_ref_label(),
                });
                match self
                    .llm
                    .query_stream_with_policy(&prompt, tools, tool_runtime_policy)
                    .await
                {
                    Ok(response) => {
                        self.emit_event(OrchestratorEvent::StreamingDecision {
                            phase: "initial",
                            mode: "stream_used",
                            model_ref: self.model_ref_label(),
                        });
                        response
                    }
                    Err(_) => {
                        self.emit_event(OrchestratorEvent::StreamingDecision {
                            phase: "initial",
                            mode: "fallback_used",
                            model_ref: self.model_ref_label(),
                        });
                        self.llm
                            .query_with_policy(&prompt, tools, tool_runtime_policy)
                            .await?
                    }
                }
            } else {
                self.emit_event(OrchestratorEvent::StreamingDecision {
                    phase: "initial",
                    mode: "stream_disabled",
                    model_ref: self.model_ref_label(),
                });
                self.llm
                    .query_with_policy(&prompt, tools, tool_runtime_policy)
                    .await?
            };

            match response {
                LLMResponse::TextAnswer(answer) => {
                    if let Some(repaired) = self.repaired_tool_call_from_text(&answer, tools) {
                        debug!(
                            tool = %repaired.name,
                            "Orchestrator: Repaired ToolCall from TextAnswer"
                        );
                        let calls = vec![repaired];
                        let mut generic_context_pack = ExecutionContextPack {
                            system_prompt: String::new(),
                            history_messages: Vec::new(),
                            context_blocks: Vec::new(),
                            user_request: prompt.clone(),
                            channel: GatewayChannel::Cli,
                            execution_contract: None,
                            retrieved_context: None,
                        };
                        let mut uses_prompt_override = true;
                        let mut progress = ToolLoopProgress {
                            rounds: &mut rounds,
                            max_tool_rounds,
                            prompt: &mut prompt,
                            context_pack: &mut generic_context_pack,
                            uses_prompt_override: &mut uses_prompt_override,
                            last_progress: &mut last_progress,
                        };
                        let ctx = GenericToolLoopContext {
                            tools_cache: tools,
                            tool_runtime_policy,
                            steering_rx: steering_rx.as_deref_mut(),
                        };
                        let res = self
                            .process_generic_tool_calls(calls, &mut progress, ctx)
                            .await?;
                        if matches!(res, OrchestratorResult::ToolApprovalRequired { .. }) {
                            return Ok(res);
                        }
                        if let OrchestratorResult::Completed(ref s) = res {
                            if s != "CONTINUE_LOOP" {
                                return Ok(res);
                            }
                        }
                    } else if self.repair_fallback_permitted() {
                        if let Some(tool_name) = extract_tool_name_candidate(&answer) {
                            rounds += 1;
                            if rounds > max_tool_rounds {
                                return Err(OrchestratorError::MaxRoundsExceeded {
                                    limit: max_tool_rounds,
                                });
                            }
                            let available = tools
                                .iter()
                                .map(|t| t.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ");
                            let limitation = tool_mode_limitation_message(
                                self.llm.capability_profile().as_ref(),
                            )
                            .unwrap_or_default();
                            prompt = format!(
                                "{}\n\n<<SYSTEM INTERRUPT: Tool '{}' is not available in this session. {} Use one of [{}], or answer in plain text. If using a tool, return only valid JSON.>>",
                                prompt, tool_name, limitation, available
                            );
                            last_progress = std::time::Instant::now();
                            continue;
                        } else {
                            return Ok(OrchestratorResult::Completed(answer));
                        }
                    } else if let Some(tool_name) = extract_tool_name_candidate(&answer) {
                        rounds += 1;
                        if rounds > max_tool_rounds {
                            return Err(OrchestratorError::MaxRoundsExceeded {
                                limit: max_tool_rounds,
                            });
                        }
                        let limitation =
                            tool_mode_limitation_message(self.llm.capability_profile().as_ref())
                                .unwrap_or_default();
                        prompt = format!(
                            "{}\n\n<<SYSTEM INTERRUPT: Tool '{}' was requested in text, but repair fallback is disabled. {} Continue without tools or request a supported model.>>",
                            prompt, tool_name, limitation
                        );
                        last_progress = std::time::Instant::now();
                        continue;
                    } else {
                        return Ok(OrchestratorResult::Completed(answer));
                    }
                }
                LLMResponse::ToolCalls(calls) => {
                    last_progress = std::time::Instant::now(); // Reset heartbeat on success
                    let mut generic_context_pack = ExecutionContextPack {
                        system_prompt: String::new(),
                        history_messages: Vec::new(),
                        context_blocks: Vec::new(),
                        user_request: prompt.clone(),
                        channel: GatewayChannel::Cli,
                        execution_contract: None,
                        retrieved_context: None,
                    };
                    let mut uses_prompt_override = true;
                    let mut progress = ToolLoopProgress {
                        rounds: &mut rounds,
                        max_tool_rounds,
                        prompt: &mut prompt,
                        context_pack: &mut generic_context_pack,
                        uses_prompt_override: &mut uses_prompt_override,
                        last_progress: &mut last_progress,
                    };
                    let ctx = GenericToolLoopContext {
                        tools_cache: tools,
                        tool_runtime_policy,
                        steering_rx: steering_rx.as_deref_mut(),
                    };
                    let res = self
                        .process_generic_tool_calls(calls, &mut progress, ctx)
                        .await?;
                    if matches!(res, OrchestratorResult::ToolApprovalRequired { .. }) {
                        return Ok(res);
                    }
                    if let OrchestratorResult::Completed(ref s) = res {
                        if s != "CONTINUE_LOOP" {
                            return Ok(res);
                        }
                    }
                }
            }
        }
    }

    pub(crate) async fn process_generic_tool_calls(
        &self,
        calls: Vec<ToolCall>,
        progress: &mut ToolLoopProgress<'_>,
        mut ctx: GenericToolLoopContext<'_>,
    ) -> Result<OrchestratorResult, OrchestratorError> {
        if calls.is_empty() {
            return Ok(OrchestratorResult::Completed(String::new()));
        }

        progress.advance_round()?;

        let mut async_calls = Vec::new();
        let mut validation_errors = Vec::new();

        for call in calls {
            if call.name == "run_shell" || call.name == "write_file" {
                // Yield execution to human operator for destructive commands.
                return Ok(OrchestratorResult::ToolApprovalRequired {
                    call: call.clone(),
                    pending_prompt: progress.prompt.clone(),
                });
            } else if call.name == "search_tool_registry" {
                // Special system tool, schema is loose, execute downstream
                async_calls.push(call);
            } else {
                // Perform JSON Schema Validation
                let mut is_valid = false;
                if let Some(cached_tool) = ctx.tools_cache.iter().find(|t| t.name == call.name) {
                    is_valid = true;
                    if let Ok(normalized_schema) =
                        normalize_tool_schema(&cached_tool.parameters_schema)
                    {
                        if let Ok(schema_val) =
                            serde_json::from_str::<serde_json::Value>(&normalized_schema)
                        {
                            if let Ok(compiled_schema) = jsonschema::Validator::new(&schema_val) {
                                if let Ok(args_val) =
                                    serde_json::from_str::<serde_json::Value>(&call.arguments)
                                {
                                    if compiled_schema.validate(&args_val).is_err() {
                                        is_valid = false;
                                        let mut err_msgs = Vec::new();
                                        for err in compiled_schema.iter_errors(&args_val) {
                                            err_msgs.push(format!("{}", err));
                                        }
                                        let merged_errs = err_msgs.join("; ");
                                        debug!(tool = %call.name, error = %merged_errs, "Tool arguments failed JSON schema validation");
                                        validation_errors.push(format!(
                                            "[Tool '{}' Rejected]: schema validation failed on arguments `{}` - {}",
                                            call.name, call.arguments, merged_errs
                                        ));
                                    }
                                } else {
                                    is_valid = false;
                                    validation_errors.push(format!(
                                        "[Tool '{}' Rejected]: arguments must be valid JSON object",
                                        call.name
                                    ));
                                }
                            }
                        }
                    } else {
                        is_valid = false;
                        validation_errors.push(format!(
                            "[Tool '{}' Rejected]: tool schema is invalid for active runtime",
                            call.name
                        ));
                    }
                } else {
                    debug!(tool = %call.name, "Attempted to call unregistered tool");
                    validation_errors.push(format!(
                        "[Tool '{}' Rejected]: tool is not available. Please use 'search_tool_registry' to find and load it, or pick from the available tools listed above.",
                        call.name
                    ));
                }

                if is_valid {
                    async_calls.push(call);
                }
            }
        }

        // If validation errors occurred, intercept the loop and auto-correct LLM immediately
        // without running any of the other async tools to prevent partial state corruption.
        if !validation_errors.is_empty() {
            *progress.prompt = format!(
                "{}\n\n<<SYSTEM INTERRUPT: Tool execution failed schema validation: \n{}\nPlease correct the tool call parameters and try again.>>",
                progress.prompt,
                validation_errors.join("\n")
            );
            *progress.uses_prompt_override = true;
            return Ok(OrchestratorResult::Completed(String::from("CONTINUE_LOOP")));
        }

        let mut tool_results = Vec::new();
        let mut executed_turns: Vec<ExecutedToolCall> = Vec::new();
        if !async_calls.is_empty() {
            let mut executed_tools: Vec<(String, ToolExecutionResult)> = Vec::new();
            let resolved = self
                .execute_tool_calls(
                    async_calls,
                    ctx.tools_cache,
                    ctx.tool_runtime_policy,
                    ctx.steering_rx.as_deref_mut(),
                    progress.prompt,
                )
                .await?;
            if resolved.is_empty() {
                return Ok(OrchestratorResult::Completed(String::new()));
            }

            for (call, result) in resolved {
                let output = match result {
                    Ok(output) => output,
                    Err(OrchestratorError::ToolError(msg)) => {
                        if approval_required_tool_name(&msg).is_some() {
                            return Ok(OrchestratorResult::ToolApprovalRequired {
                                call,
                                pending_prompt: progress.prompt.clone(),
                            });
                        }
                        tool_failure_result(&call, &msg)
                    }
                    Err(err) => return Err(err),
                };
                let rendered_output = render_tool_result_for_model(&call.name, &output);
                debug!(tool = %call.name, output_len = rendered_output.len(), output_preview = %rendered_output.chars().take(100).collect::<String>(), "Tool: executed");
                tool_results.push(format!("[Tool: {}] Result: {}", call.name, rendered_output));
                executed_turns.push(ExecutedToolCall {
                    call: call.clone(),
                    result: output.clone(),
                });
                executed_tools.push((call.name, output));
            }

            if let Some(final_text) = maybe_finalize_after_scheduler_tools(&executed_tools) {
                return Ok(OrchestratorResult::Completed(final_text));
            }

            // Update heartbeat after successful tool executions
            *progress.last_progress = std::time::Instant::now();
        }

        if executed_turns.is_empty() {
            *progress.prompt = format!(
                "{}\n\n--- Tool Results ---\n{}",
                progress.prompt,
                tool_results.join("\n")
            );
            *progress.uses_prompt_override = true;
        } else {
            let follow_up = if self.should_use_streaming(ctx.tools_cache) {
                self.emit_event(OrchestratorEvent::StreamingDecision {
                    phase: "follow_up",
                    mode: "stream_attempt",
                    model_ref: self.model_ref_label(),
                });
                match self
                    .llm
                    .query_stream_with_tool_results_and_policy(
                        progress.prompt,
                        ctx.tools_cache,
                        &executed_turns,
                        ctx.tool_runtime_policy,
                    )
                    .await
                {
                    Ok(response) => {
                        self.emit_event(OrchestratorEvent::StreamingDecision {
                            phase: "follow_up",
                            mode: "stream_used",
                            model_ref: self.model_ref_label(),
                        });
                        response
                    }
                    Err(_) => {
                        self.emit_event(OrchestratorEvent::StreamingDecision {
                            phase: "follow_up",
                            mode: "fallback_used",
                            model_ref: self.model_ref_label(),
                        });
                        if *progress.uses_prompt_override {
                            self.llm
                                .query_with_tool_results_and_policy(
                                    progress.prompt,
                                    ctx.tools_cache,
                                    &executed_turns,
                                    ctx.tool_runtime_policy,
                                )
                                .await?
                        } else {
                            self.llm
                                .query_context_with_tool_results_and_policy(
                                    progress.context_pack,
                                    ctx.tools_cache,
                                    &executed_turns,
                                    ctx.tool_runtime_policy,
                                )
                                .await?
                        }
                    }
                }
            } else {
                self.emit_event(OrchestratorEvent::StreamingDecision {
                    phase: "follow_up",
                    mode: "stream_disabled",
                    model_ref: self.model_ref_label(),
                });
                if *progress.uses_prompt_override {
                    self.llm
                        .query_with_tool_results_and_policy(
                            progress.prompt,
                            ctx.tools_cache,
                            &executed_turns,
                            ctx.tool_runtime_policy,
                        )
                        .await?
                } else {
                    self.llm
                        .query_context_with_tool_results_and_policy(
                            progress.context_pack,
                            ctx.tools_cache,
                            &executed_turns,
                            ctx.tool_runtime_policy,
                        )
                        .await?
                }
            };
            match follow_up {
                LLMResponse::TextAnswer(answer) => {
                    if let Some(repaired) =
                        self.repaired_tool_call_from_text(&answer, ctx.tools_cache)
                    {
                        *progress.context_pack = append_tool_results_to_context_pack(
                            progress.context_pack,
                            &executed_turns,
                        );
                        if *progress.uses_prompt_override {
                            *progress.prompt =
                                append_tool_results_to_prompt(progress.prompt, &executed_turns);
                        }
                        return Box::pin(self.process_generic_tool_calls(
                            vec![repaired],
                            progress,
                            ctx,
                        ))
                        .await;
                    }
                    return Ok(OrchestratorResult::Completed(answer));
                }
                LLMResponse::ToolCalls(next_calls) => {
                    *progress.context_pack =
                        append_tool_results_to_context_pack(progress.context_pack, &executed_turns);
                    if *progress.uses_prompt_override {
                        *progress.prompt =
                            append_tool_results_to_prompt(progress.prompt, &executed_turns);
                    }
                    if !next_calls.is_empty() {
                        return Box::pin(
                            self.process_generic_tool_calls(next_calls, progress, ctx),
                        )
                        .await;
                    }
                }
            }
        }

        // Signal that tools were processed and the loop should continue
        Ok(OrchestratorResult::Completed("CONTINUE_LOOP".to_string()))
    }

    pub fn build_request_prompt(
        agent_system_prompt: &str,
        request: &AgentRequest,
        history_context: &str,
        rag_context: &str,
        tools: &[CachedTool],
        capability_profile: Option<&ModelCapabilityProfile>,
    ) -> String {
        let arena = PromptArena::new();
        PromptManager::build_execution_prompt_arena(
            &arena,
            agent_system_prompt,
            request,
            history_context,
            rag_context,
            tools,
            capability_profile,
        )
    }

    /// Request-aware orchestrator entrypoint aligned with gateway normalization.
    pub async fn run_for_request(
        &self,
        agent_system_prompt: &str,
        request: &AgentRequest,
        history_context: &str,
        rag_context: &str,
        tools: &[CachedTool],
        max_tool_rounds: usize,
    ) -> Result<OrchestratorResult, OrchestratorError> {
        let prompt = Self::build_request_prompt(
            agent_system_prompt,
            request,
            history_context,
            rag_context,
            tools,
            self.llm.capability_profile().as_ref(),
        );
        let tool_runtime_policy = self.effective_tool_runtime_policy(Some(request));
        self.run_with_tool_runtime_policy(
            &prompt,
            tools,
            max_tool_rounds,
            &tool_runtime_policy,
            None,
            None,
        )
        .await
    }

    /// Request-aware orchestrator loop that supports dynamic tool hot-swap via
    /// the `search_tool_registry` meta-tool.
    pub async fn run_for_request_with_dynamic_tools<E: EmbeddingModel>(
        &self,
        run: DynamicRunContext<'_, E>,
    ) -> Result<OrchestratorResult, OrchestratorError> {
        let DynamicRunContext {
            agent_system_prompt,
            request,
            history_context,
            rag_context,
            history_messages,
            context_blocks,
            prompt_tools,
            tool_selection,
            cache,
            tool_registry,
            embedder,
            max_tool_rounds,
            model_capability,
            mut steering_rx,
            global_estop,
        } = run;
        let effective_tool_runtime_policy = self.effective_tool_runtime_policy(Some(request));
        let llm_capability_profile = self.llm.capability_profile();
        // Build initial prompt with tool schemas injected so small models know the format.
        let initial_tools = if let Some(prompt_tools) = prompt_tools {
            prompt_tools.to_vec()
        } else {
            filter_tools_for_model_capability_with_repair(
                &cache.active_tools(),
                model_capability,
                self.allow_repair_fallback,
            )
        };
        let arena = PromptArena::new();
        let mut context_pack = PromptManager::build_execution_context_pack(
            &arena,
            agent_system_prompt,
            request,
            history_messages,
            context_blocks.to_vec(),
            &initial_tools,
            model_capability.or(llm_capability_profile.as_ref()),
            Some(tool_calling_mode_for_model_with_repair(
                model_capability.or(llm_capability_profile.as_ref()),
                self.allow_repair_fallback,
            )),
        );
        let mut prompt = PromptManager::render_execution_context_pack(&context_pack);
        let mut rounds = 0usize;
        let mut last_progress = std::time::Instant::now();
        let mut uses_prompt_override = false;
        let mut enforced_tool_obligation = false;

        let prompt_preview = if prompt.len() > 500 {
            format!("{}...", &prompt[..500])
        } else {
            prompt.clone()
        };
        debug!(
            prompt_len = prompt.len(),
            prompt_preview = %prompt_preview,
            prompt_full = %prompt,
            history_context_len = history_context.len(),
            rag_context_len = rag_context.len(),
            "Orchestrator: built prompt"
        );

        loop {
            // Check global emergency stop flag
            if let Some(estop) = global_estop {
                if estop.load(std::sync::atomic::Ordering::SeqCst) {
                    debug!("Orchestrator: Global ESTOP triggered, aborting dynamic run loop");
                    return Err(OrchestratorError::UserAborted);
                }
            }

            // Check for stuck jobs: if no progress in the last 60 seconds, inject a correction prompt.
            if rounds > 0 && last_progress.elapsed().as_secs() > 60 {
                debug!("Orchestrator: dynamic loop job heartbeat stalled. Injecting correction prompt.");
                prompt.push_str("\n\n[SYSTEM CORRECTIVE: Your previous approach has stalled for over 60 seconds without resolving the query. Please pivot and try a different strategy or tool immediately.]");
                uses_prompt_override = true;
                last_progress = std::time::Instant::now(); // reset heartbeat after injection
            }

            if let Some(rx) = steering_rx.as_deref_mut() {
                if let Ok(cmd) = rx.try_recv() {
                    match cmd {
                        SteeringCommand::Abort => return Err(OrchestratorError::UserAborted),
                        SteeringCommand::Pivot(new_instructions) => {
                            debug!("Orchestrator: user pivot detected mid-flight before dynamic LLM call");
                            prompt = format!("{}\n\n<<SYSTEM INTERRUPT: User provided new steerings: '{}'>>\nAbort your previous tool plan and restart reasoning.", prompt, new_instructions);
                            uses_prompt_override = true;
                        }
                    }
                }
            }

            let active_tools = filter_tools_for_model_capability_with_repair(
                &cache.active_tools(),
                model_capability,
                self.allow_repair_fallback,
            );
            let tool_names: Vec<String> = active_tools.iter().map(|t| t.name.clone()).collect();
            debug!(round = rounds + 1, tools = ?tool_names, "Orchestrator: querying LLM");

            let response = if self.should_use_streaming(&active_tools) {
                self.emit_event(OrchestratorEvent::StreamingDecision {
                    phase: "initial",
                    mode: "stream_attempt",
                    model_ref: self.model_ref_label(),
                });
                match self
                    .llm
                    .query_stream_with_policy(
                        &prompt,
                        &active_tools,
                        &effective_tool_runtime_policy,
                    )
                    .await
                {
                    Ok(response) => {
                        self.emit_event(OrchestratorEvent::StreamingDecision {
                            phase: "initial",
                            mode: "stream_used",
                            model_ref: self.model_ref_label(),
                        });
                        response
                    }
                    Err(_) => {
                        self.emit_event(OrchestratorEvent::StreamingDecision {
                            phase: "initial",
                            mode: "fallback_used",
                            model_ref: self.model_ref_label(),
                        });
                        self.llm
                            .query_context_with_policy(
                                &context_pack,
                                &active_tools,
                                &effective_tool_runtime_policy,
                            )
                            .await?
                    }
                }
            } else {
                self.emit_event(OrchestratorEvent::StreamingDecision {
                    phase: "initial",
                    mode: "stream_disabled",
                    model_ref: self.model_ref_label(),
                });
                if uses_prompt_override {
                    self.llm
                        .query_with_policy(&prompt, &active_tools, &effective_tool_runtime_policy)
                        .await?
                } else {
                    self.llm
                        .query_context_with_policy(
                            &context_pack,
                            &active_tools,
                            &effective_tool_runtime_policy,
                        )
                        .await?
                }
            };
            match response {
                LLMResponse::TextAnswer(answer) => {
                    if let Some(repaired) =
                        self.repaired_tool_call_from_text(&answer, &active_tools)
                    {
                        debug!(
                            tool = %repaired.name,
                            "Orchestrator: Repaired ToolCall from TextAnswer (dynamic)"
                        );
                        let calls = vec![repaired];
                        let mut progress = ToolLoopProgress {
                            rounds: &mut rounds,
                            max_tool_rounds,
                            prompt: &mut prompt,
                            context_pack: &mut context_pack,
                            uses_prompt_override: &mut uses_prompt_override,
                            last_progress: &mut last_progress,
                        };
                        let ctx = DynamicToolLoopContext {
                            steering_rx: steering_rx.as_deref_mut(),
                            global_estop: global_estop.map(|a| a.as_ref()),
                            cache,
                            tool_registry,
                            embedder,
                            model_capability,
                            tool_runtime_policy: &effective_tool_runtime_policy,
                        };
                        let res = self
                            .process_dynamic_tool_calls(calls, &mut progress, ctx)
                            .await?;
                        if matches!(res, OrchestratorResult::ToolApprovalRequired { .. }) {
                            return Ok(res);
                        }
                        if let OrchestratorResult::Completed(ref s) = res {
                            if s != "CONTINUE_LOOP" {
                                return Ok(res);
                            }
                        }
                    } else if self.repair_fallback_permitted() {
                        let repair_mode = matches!(
                            tool_calling_mode_for_model_with_repair(
                                model_capability.or(llm_capability_profile.as_ref()),
                                self.allow_repair_fallback,
                            ),
                            ToolCallingMode::TextFallbackWithRepair
                        );
                        if let Some(tool_name) = extract_tool_name_candidate(&answer) {
                            rounds += 1;
                            if rounds > max_tool_rounds {
                                return Err(OrchestratorError::MaxRoundsExceeded {
                                    limit: max_tool_rounds,
                                });
                            }
                            let available = active_tools
                                .iter()
                                .map(|t| t.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ");
                            let limitation =
                                tool_mode_limitation_message(model_capability).unwrap_or_default();
                            prompt = format!(
                                "{}\n\n<<SYSTEM INTERRUPT: Tool '{}' is not available in this session. {} Use one of [{}], or answer in plain text. If using a tool, return only valid JSON.>>",
                                prompt, tool_name, limitation, available
                            );
                            uses_prompt_override = true;
                            last_progress = Instant::now();
                            continue;
                        } else if repair_mode
                            && !enforced_tool_obligation
                            && tool_selection_requires_tool_round(tool_selection)
                            && !active_tools.is_empty()
                        {
                            rounds += 1;
                            if rounds > max_tool_rounds {
                                return Err(OrchestratorError::MaxRoundsExceeded {
                                    limit: max_tool_rounds,
                                });
                            }
                            enforced_tool_obligation = true;
                            let available =
                                selected_tool_names_for_obligation(tool_selection, &active_tools)
                                    .join(", ");
                            prompt = format!(
                                "{}\n\n<<SYSTEM INTERRUPT: A relevant tool path is available for this request. Do not answer with a plan or promise. Either return a valid JSON tool call using one of [{}], or explain concretely why none of the available tools can satisfy the request.>>",
                                prompt, available
                            );
                            uses_prompt_override = true;
                            last_progress = Instant::now();
                            continue;
                        } else {
                            debug!(
                                answer_len = answer.len(),
                                answer_preview = %answer.chars().take(200).collect::<String>(),
                                "Orchestrator: LLM returned TextAnswer"
                            );
                            return Ok(OrchestratorResult::Completed(answer));
                        }
                    } else if let Some(tool_name) = extract_tool_name_candidate(&answer) {
                        rounds += 1;
                        if rounds > max_tool_rounds {
                            return Err(OrchestratorError::MaxRoundsExceeded {
                                limit: max_tool_rounds,
                            });
                        }
                        let limitation =
                            tool_mode_limitation_message(model_capability).unwrap_or_default();
                        prompt = format!(
                            "{}\n\n<<SYSTEM INTERRUPT: Tool '{}' was requested in text, but repair fallback is disabled. {} Continue without tools or request a supported model.>>",
                            prompt, tool_name, limitation
                        );
                        last_progress = Instant::now();
                        continue;
                    } else {
                        debug!(
                            answer_len = answer.len(),
                            answer_preview = %answer.chars().take(200).collect::<String>(),
                            "Orchestrator: LLM returned TextAnswer"
                        );
                        return Ok(OrchestratorResult::Completed(answer));
                    }
                }
                LLMResponse::ToolCalls(calls) => {
                    last_progress = Instant::now();
                    let mut progress = ToolLoopProgress {
                        rounds: &mut rounds,
                        max_tool_rounds,
                        prompt: &mut prompt,
                        context_pack: &mut context_pack,
                        uses_prompt_override: &mut uses_prompt_override,
                        last_progress: &mut last_progress,
                    };
                    let ctx = DynamicToolLoopContext {
                        steering_rx: steering_rx.as_deref_mut(),
                        global_estop: global_estop.map(|a| a.as_ref()),
                        cache,
                        tool_registry,
                        embedder,
                        model_capability,
                        tool_runtime_policy: &effective_tool_runtime_policy,
                    };
                    let res = self
                        .process_dynamic_tool_calls(calls, &mut progress, ctx)
                        .await?;
                    if matches!(res, OrchestratorResult::ToolApprovalRequired { .. }) {
                        return Ok(res);
                    }
                    if let OrchestratorResult::Completed(ref s) = res {
                        if s != "CONTINUE_LOOP" {
                            return Ok(res);
                        }
                    }
                }
            }
        }
    }

    async fn process_dynamic_tool_calls<E: EmbeddingModel>(
        &self,
        calls: Vec<ToolCall>,
        progress: &mut ToolLoopProgress<'_>,
        mut ctx: DynamicToolLoopContext<'_, E>,
    ) -> Result<OrchestratorResult, OrchestratorError> {
        if let Some(estop) = ctx.global_estop {
            if estop.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(OrchestratorError::UserAborted);
            }
        }
        if calls.is_empty() {
            return Ok(OrchestratorResult::Completed(String::new()));
        }

        progress.advance_round()?;

        let mut tool_results = Vec::new();
        let mut executed_turns: Vec<ExecutedToolCall> = Vec::new();
        let mut async_calls = Vec::new();

        for call in calls {
            if call.name == "search_tool_registry" {
                let query = extract_registry_query(&call.arguments).unwrap_or_default();
                debug!(query = %query, "Tool: search_tool_registry invoked");
                let results = ctx
                    .tool_registry
                    .search_with_explanations(&query, ctx.embedder, 3, ctx.model_capability)
                    .map_err(|e| OrchestratorError::ToolError(format!("{}", e)))?;
                let available = results
                    .iter()
                    .filter(|entry| entry.visibility.available)
                    .map(|entry| entry.tool.name.clone())
                    .collect::<Vec<_>>();
                let hidden = results
                    .iter()
                    .filter(|entry| !entry.visibility.available)
                    .map(|entry| entry.visibility.reason.as_message(&entry.tool.name))
                    .collect::<Vec<_>>();
                if available.is_empty() {
                    debug!("Tool: search_tool_registry found no matching tools");
                    tool_results.push(if hidden.is_empty() {
                        "[Tool: search_tool_registry] Result: no matching tools found".to_string()
                    } else {
                        format!(
                            "[Tool: search_tool_registry] Result: no compatible tools found. {}",
                            hidden.join("; ")
                        )
                    });
                } else {
                    tool_results.push(format!(
                        "[Tool: search_tool_registry] Result: discovered [{}]. Use one of these already-bound tools if available, or bind/import the desired tool explicitly before retrying.",
                        available.join(", ")
                    ));
                }
            } else if call.name == "run_shell" || call.name == "write_file" {
                debug!(tool = %call.name, "Tool requires human-in-the-loop approval, yielding execution flow");
                return Ok(OrchestratorResult::ToolApprovalRequired {
                    call: call.clone(),
                    pending_prompt: progress.prompt.clone(),
                });
            } else {
                async_calls.push(call);
            }
        }

        if !async_calls.is_empty() {
            let mut executed_tools: Vec<(String, ToolExecutionResult)> = Vec::new();
            let active_tools_snapshot = ctx.cache.active_tools();
            let resolved = self
                .execute_tool_calls(
                    async_calls,
                    &active_tools_snapshot,
                    ctx.tool_runtime_policy,
                    ctx.steering_rx.as_deref_mut(),
                    progress.prompt,
                )
                .await?;
            if resolved.is_empty() {
                return Ok(OrchestratorResult::Completed(String::new()));
            }

            for (call, result) in resolved {
                let output = match result {
                    Ok(output) => output,
                    Err(OrchestratorError::ToolError(msg)) => {
                        if approval_required_tool_name(&msg).is_some() {
                            return Ok(OrchestratorResult::ToolApprovalRequired {
                                call,
                                pending_prompt: progress.prompt.clone(),
                            });
                        }
                        tool_failure_result(&call, &msg)
                    }
                    Err(err) => return Err(err),
                };
                let rendered_output = render_tool_result_for_model(&call.name, &output);
                debug!(tool = %call.name, output_len = rendered_output.len(), output_preview = %rendered_output.chars().take(100).collect::<String>(), "Tool: executed");
                tool_results.push(format!("[Tool: {}] Result: {}", call.name, rendered_output));
                executed_turns.push(ExecutedToolCall {
                    call: call.clone(),
                    result: output.clone(),
                });
                executed_tools.push((call.name, output));
            }

            if let Some(final_text) = maybe_finalize_after_scheduler_tools(&executed_tools) {
                return Ok(OrchestratorResult::Completed(final_text));
            }
        }

        let active_tools = filter_tools_for_model_capability_with_repair(
            &ctx.cache.active_tools(),
            ctx.model_capability,
            self.allow_repair_fallback,
        );
        if executed_turns.is_empty() {
            *progress.prompt = format!(
                "{}\n\n--- Tool Results ---\n{}",
                progress.prompt,
                tool_results.join("\n")
            );
            *progress.uses_prompt_override = true;
        } else {
            let follow_up = if self.should_use_streaming(&active_tools) {
                self.emit_event(OrchestratorEvent::StreamingDecision {
                    phase: "follow_up",
                    mode: "stream_attempt",
                    model_ref: self.model_ref_label(),
                });
                match self
                    .llm
                    .query_stream_with_tool_results_and_policy(
                        progress.prompt,
                        &active_tools,
                        &executed_turns,
                        ctx.tool_runtime_policy,
                    )
                    .await
                {
                    Ok(response) => {
                        self.emit_event(OrchestratorEvent::StreamingDecision {
                            phase: "follow_up",
                            mode: "stream_used",
                            model_ref: self.model_ref_label(),
                        });
                        response
                    }
                    Err(_) => {
                        self.emit_event(OrchestratorEvent::StreamingDecision {
                            phase: "follow_up",
                            mode: "fallback_used",
                            model_ref: self.model_ref_label(),
                        });
                        if *progress.uses_prompt_override {
                            self.llm
                                .query_with_tool_results_and_policy(
                                    progress.prompt,
                                    &active_tools,
                                    &executed_turns,
                                    ctx.tool_runtime_policy,
                                )
                                .await?
                        } else {
                            self.llm
                                .query_context_with_tool_results_and_policy(
                                    progress.context_pack,
                                    &active_tools,
                                    &executed_turns,
                                    ctx.tool_runtime_policy,
                                )
                                .await?
                        }
                    }
                }
            } else {
                self.emit_event(OrchestratorEvent::StreamingDecision {
                    phase: "follow_up",
                    mode: "stream_disabled",
                    model_ref: self.model_ref_label(),
                });
                if *progress.uses_prompt_override {
                    self.llm
                        .query_with_tool_results_and_policy(
                            progress.prompt,
                            &active_tools,
                            &executed_turns,
                            ctx.tool_runtime_policy,
                        )
                        .await?
                } else {
                    self.llm
                        .query_context_with_tool_results_and_policy(
                            progress.context_pack,
                            &active_tools,
                            &executed_turns,
                            ctx.tool_runtime_policy,
                        )
                        .await?
                }
            };
            match follow_up {
                LLMResponse::TextAnswer(answer) => {
                    if let Some(repaired) =
                        self.repaired_tool_call_from_text(&answer, &active_tools)
                    {
                        *progress.context_pack = append_tool_results_to_context_pack(
                            progress.context_pack,
                            &executed_turns,
                        );
                        if *progress.uses_prompt_override {
                            *progress.prompt =
                                append_tool_results_to_prompt(progress.prompt, &executed_turns);
                        }
                        return Box::pin(self.process_dynamic_tool_calls(
                            vec![repaired],
                            progress,
                            ctx,
                        ))
                        .await;
                    }
                    return Ok(OrchestratorResult::Completed(answer));
                }
                LLMResponse::ToolCalls(next_calls) => {
                    *progress.context_pack =
                        append_tool_results_to_context_pack(progress.context_pack, &executed_turns);
                    if *progress.uses_prompt_override {
                        *progress.prompt =
                            append_tool_results_to_prompt(progress.prompt, &executed_turns);
                    }
                    if !next_calls.is_empty() {
                        return Box::pin(
                            self.process_dynamic_tool_calls(next_calls, progress, ctx),
                        )
                        .await;
                    }
                }
            }
        }
        *progress.last_progress = Instant::now();
        Ok(OrchestratorResult::Completed("CONTINUE_LOOP".to_string()))
    }
}

fn extract_registry_query(arguments_json: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(arguments_json).ok()?;
    value
        .get("query")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

pub(crate) fn maybe_finalize_after_scheduler_tools(
    executed_tools: &[(String, ToolExecutionResult)],
) -> Option<String> {
    if executed_tools.is_empty() {
        return None;
    }
    let all_scheduler_tools = executed_tools.iter().all(|(name, _)| {
        matches!(
            name.as_str(),
            "schedule_message" | "set_reminder" | "manage_cron"
        )
    });
    if !all_scheduler_tools {
        return None;
    }
    let merged = executed_tools
        .iter()
        .map(|(_, out)| out.render_for_prompt().trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if merged.is_empty() {
        Some("Scheduling updated.".to_string())
    } else {
        Some(merged)
    }
}

pub(crate) fn append_tool_results_to_prompt(
    prompt: &str,
    executed_tools: &[ExecutedToolCall],
) -> String {
    let rendered = executed_tools
        .iter()
        .map(|entry| {
            format!(
                "[Tool: {}] Result: {}",
                entry.call.name,
                render_tool_result_for_model(&entry.call.name, &entry.result)
            )
        })
        .collect::<Vec<_>>();
    format!(
        "{}\n\n--- Tool Results ---\n{}",
        prompt,
        rendered.join("\n")
    )
}

pub(crate) fn append_tool_results_to_context_pack(
    pack: &aria_core::ExecutionContextPack,
    executed_tools: &[ExecutedToolCall],
) -> aria_core::ExecutionContextPack {
    if executed_tools.is_empty() {
        return pack.clone();
    }
    let mut next = pack.clone();
    let content = executed_tools
        .iter()
        .map(|entry| {
            format!(
                "[Tool: {}] Result: {}",
                entry.call.name,
                render_tool_result_for_model(&entry.call.name, &entry.result)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    next.context_blocks.push(aria_core::ContextBlock {
        kind: aria_core::ContextBlockKind::ToolInstructions,
        label: "tool_results".into(),
        token_estimate: content.split_whitespace().count() as u32,
        content,
    });
    next
}
