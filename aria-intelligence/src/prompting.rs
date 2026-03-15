use super::*;
use aria_core::{
    AdapterFamily, CapabilitySupport, ContextBlock, ContextBlockKind, ExecutionContextPack,
    PromptContextMessage, ToolCallingMode,
};

// ---------------------------------------------------------------------------
// Item 3: Bump-Pointer Arena Prompt Assembly (P3)
// ---------------------------------------------------------------------------

/// A short-lived arena for prompt construction.
/// Reduces allocator churn for large string concatenations during orchestration.
pub struct PromptArena {
    bump: bumpalo::Bump,
}

impl PromptArena {
    pub fn new() -> Self {
        Self {
            bump: bumpalo::Bump::new(),
        }
    }

    /// Allocate a formatted string in the arena.
    pub fn format<'a>(&'a self, args: std::fmt::Arguments) -> &'a str {
        use std::fmt::Write;
        let mut s = bumpalo::collections::String::new_in(&self.bump);
        s.write_fmt(args).expect("arena allocation failed");
        s.into_bump_str()
    }

    /// Allocate a segment of history or context in the arena.
    pub fn alloc(&self, s: &str) -> &str {
        self.bump.alloc_str(s)
    }
}

impl Default for PromptArena {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptMode {
    Routing,
    Planning,
    Execution,
    Scheduling,
    Clarification,
    Summarization,
    Media,
    Robotics,
}

pub struct PromptManager;

impl PromptManager {
    fn estimate_token_count(text: &str) -> usize {
        text.split_whitespace().count()
    }

    fn render_user_text<'a>(arena: &'a PromptArena, request: &AgentRequest) -> &'a str {
        match &request.content {
            MessageContent::Text(s) => arena.alloc(s),
            MessageContent::Image { url, caption } => arena.format(format_args!(
                "User sent an image.\nurl: {}\ncaption: {}",
                url,
                caption.as_deref().unwrap_or_default()
            )),
            MessageContent::Audio { url, transcript } => arena.format(format_args!(
                "User sent an audio message.\nurl: {}\ntranscript: {}",
                url,
                transcript.as_deref().unwrap_or_default()
            )),
            MessageContent::Video {
                url,
                caption,
                transcript,
            } => arena.format(format_args!(
                "User sent a video.\nurl: {}\ncaption: {}\naudio_transcript: {}",
                url,
                caption.as_deref().unwrap_or_default(),
                transcript.as_deref().unwrap_or_default()
            )),
            MessageContent::Document {
                url,
                caption,
                mime_type,
            } => arena.format(format_args!(
                "User sent a document.\nurl: {}\ncaption: {}\nmime_type: {}",
                url,
                caption.as_deref().unwrap_or_default(),
                mime_type.as_deref().unwrap_or("unknown")
            )),
            MessageContent::Location { lat, lng } => arena.format(format_args!(
                "User shared location: lat={}, lng={}",
                lat, lng
            )),
        }
    }

    pub fn build_execution_context_pack(
        arena: &PromptArena,
        agent_system_prompt: &str,
        request: &AgentRequest,
        history_messages: &[PromptContextMessage],
        context_blocks: Vec<ContextBlock>,
        tools: &[CachedTool],
        capability_profile: Option<&ModelCapabilityProfile>,
        tool_calling_mode: Option<ToolCallingMode>,
    ) -> ExecutionContextPack {
        let user_text = Self::render_user_text(arena, request).to_string();
        let now_utc = chrono::Utc::now();
        let now_local = chrono::Local::now();
        let provider_variant = Self::provider_execution_guidance(capability_profile);
        let tool_section =
            Self::textual_tool_instruction_block(tools, capability_profile, tool_calling_mode);
        let meta_instructions = format!(
            "--- System Directives ---\n\
            1. You are a precise, concise AI agent. Follow the defined system prompt strictly.\n\
            2. If native tool calling is unavailable, use only the documented fallback tool format.\n\
            3. Do not over-explain. Provide direct answers.\n\
            4. Current UTC time: {}.\n\
            5. Current local time: {}.\n\
            6. For reminders/scheduling, always pass a structured 'schedule' object. Use kind='at' with RFC3339 timestamps for one-shot events, kind='every' for fixed intervals, kind='daily' for daily wall-clock time, kind='weekly' for weekly or biweekly wall-clock time, and kind='cron' for advanced cron expressions.\n\
            7. For schedule_message/set_reminder, use mode='notify' for static reminders, mode='defer' to execute work at trigger time, and mode='both' if both are needed.\n\
            8. If user asks to perform work \"in X\" time, default to mode='defer' (do not execute now) unless user explicitly asks for both immediate and delayed output.\n\
            9. Prefer schedule.kind='at' for one-shot requests like \"in 1 minute\", \"after 2 hours\", \"today at 10 PM\", or \"tomorrow 8:15 AM\". Use schedule.kind='every' only for true repeating intervals. Use schedule.kind='daily' or 'weekly' only when the user asks for recurring wall-clock schedules.\n\
            10. Never emit placeholder schedule payloads like \"{{}}\", \"null\", or empty objects. Always send a concrete schedule object with a kind.\n\
            11. {}\n",
            now_utc.to_rfc3339(),
            now_local.format("%Y-%m-%d %H:%M:%S %:z"),
            provider_variant
        );
        let mut system_prompt = format!("{}\n\n{}", agent_system_prompt, meta_instructions);
        let mut blocks = context_blocks;
        if let Some(tool_block) = tool_section {
            blocks.push(ContextBlock {
                kind: ContextBlockKind::ToolInstructions,
                label: "Tool fallback instructions".into(),
                token_estimate: Self::estimate_token_count(&tool_block) as u32,
                content: tool_block,
            });
        }
        system_prompt = system_prompt.trim().to_string();
        ExecutionContextPack {
            system_prompt,
            history_messages: history_messages.to_vec(),
            context_blocks: blocks,
            user_request: user_text,
            channel: request.channel,
            execution_contract: None,
            retrieved_context: None,
        }
    }

    pub fn render_execution_context_pack(pack: &ExecutionContextPack) -> String {
        let mut rendered = format!(
            "Prompt Mode: {:?}\nSystem Prompt:\n{}",
            PromptMode::Execution,
            pack.system_prompt
        );
        if let Some(contract) = &pack.execution_contract {
            rendered.push_str("\n\nExecution Contract:\n");
            rendered.push_str(&format!(
                "kind={:?}; required_artifacts={:?}; fallback={:?}",
                contract.kind, contract.required_artifact_kinds, contract.fallback_mode
            ));
        }
        for block in &pack.context_blocks {
            rendered.push_str("\n\n");
            rendered.push_str(&Self::render_context_block_label(block));
            rendered.push('\n');
            rendered.push_str(&block.content);
        }
        if let Some(bundle) = &pack.retrieved_context {
            rendered.push_str("\n\nRetrieved Context Summary:\n");
            if let Some(plan) = &bundle.plan_summary {
                rendered.push_str(plan);
                rendered.push('\n');
            }
            for block in &bundle.blocks {
                rendered.push_str(&format!(
                    "- {:?} {} score={:?}\n",
                    block.source_kind, block.label, block.score
                ));
            }
        }
        rendered.push_str("\n\nSession History:\n");
        for message in &pack.history_messages {
            rendered.push_str(&format!("{}: {}\n", message.role, message.content));
        }
        rendered.push_str(&format!(
            "\nUser Request (channel={:?}):\n{}",
            pack.channel, pack.user_request
        ));
        rendered
    }

    pub fn build_execution_prompt_arena(
        arena: &PromptArena,
        agent_system_prompt: &str,
        request: &AgentRequest,
        history_context: &str,
        rag_context: &str,
        tools: &[CachedTool],
        capability_profile: Option<&ModelCapabilityProfile>,
    ) -> String {
        let history_messages = history_context
            .lines()
            .filter_map(|line| {
                let (role, content) = line.split_once(':')?;
                Some(PromptContextMessage {
                    role: role.trim().to_string(),
                    content: content.trim().to_string(),
                    timestamp_us: 0,
                })
            })
            .collect::<Vec<_>>();
        let mut blocks = Vec::new();
        if !rag_context.trim().is_empty() {
            blocks.push(ContextBlock {
                kind: ContextBlockKind::Retrieval,
                label: "RAG Context".into(),
                token_estimate: Self::estimate_token_count(rag_context) as u32,
                content: rag_context.to_string(),
            });
        }
        let pack = Self::build_execution_context_pack(
            arena,
            agent_system_prompt,
            request,
            &history_messages,
            blocks,
            tools,
            capability_profile,
            None,
        );
        Self::render_execution_context_pack(&pack)
    }

    fn render_context_block_label(block: &ContextBlock) -> String {
        match block.kind {
            ContextBlockKind::Retrieval => format!("RAG Context [{}]:", block.label),
            ContextBlockKind::ControlDocument => {
                format!("Control Documents [{}]:", block.label)
            }
            ContextBlockKind::DurableConstraint => {
                format!("Durable Constraints [{}]:", block.label)
            }
            ContextBlockKind::SubAgentResult => format!("Sub-Agent Results [{}]:", block.label),
            ContextBlockKind::ToolInstructions => {
                format!("Tool Instructions [{}]:", block.label)
            }
            ContextBlockKind::PromptAsset => format!("Prompt Assets [{}]:", block.label),
            ContextBlockKind::ResourceContext => format!("Resource Context [{}]:", block.label),
            ContextBlockKind::CapabilityIndex => format!("Capability Index [{}]:", block.label),
            ContextBlockKind::DocumentIndex => format!("Document Index [{}]:", block.label),
            ContextBlockKind::ContractRequirements => {
                format!("Execution Contract [{}]:", block.label)
            }
        }
    }

    fn textual_tool_instruction_block(
        tools: &[CachedTool],
        capability_profile: Option<&ModelCapabilityProfile>,
        tool_calling_mode: Option<ToolCallingMode>,
    ) -> Option<String> {
        if tools.is_empty() {
            return None;
        }
        let should_inline = match capability_profile {
            _ if matches!(
                tool_calling_mode,
                Some(ToolCallingMode::TextFallbackWithRepair)
            ) =>
            {
                true
            }
            Some(profile) => {
                matches!(
                    profile.tool_calling,
                    CapabilitySupport::Unsupported | CapabilitySupport::Unknown
                ) || matches!(profile.adapter_family, AdapterFamily::TextOnlyCli)
            }
            None => true,
        };
        if !should_inline {
            return None;
        }
        let mut tools_buffer = String::new();
        use std::fmt::Write;
        let _ = writeln!(tools_buffer, "--- Available Tools ---");
        let _ = writeln!(
            tools_buffer,
            "To call a tool, respond with ONLY the following JSON and nothing else:"
        );
        let _ = writeln!(
            tools_buffer,
            r#"{{"tool": "<tool_name>", "args": {{<key>: <value>, ...}}}}"#
        );
        let _ = writeln!(tools_buffer, "\nTools:");
        for t in tools {
            let _ = writeln!(tools_buffer, "- {}: {}", t.name, t.description);
            if !t.parameters_schema.is_empty() && t.parameters_schema != "{}" {
                let rendered_schema = normalize_tool_schema(&t.parameters_schema)
                    .unwrap_or_else(|_| t.parameters_schema.clone());
                let _ = writeln!(tools_buffer, "  Schema: {}", rendered_schema);
            }
        }
        let _ = writeln!(
            tools_buffer,
            "\nIf no tool is needed, reply with plain text."
        );
        Some(tools_buffer.trim().to_string())
    }

    fn provider_execution_guidance(capability_profile: Option<&ModelCapabilityProfile>) -> String {
        let Some(profile) = capability_profile else {
            return "Use the simplest valid tool call format available to the current model."
                .into();
        };
        match profile.adapter_family {
            AdapterFamily::OpenAiCompatible => {
                "Prefer one precise JSON tool call at a time; do not wrap the JSON in markdown."
                    .into()
            }
            AdapterFamily::Anthropic => {
                "Prefer concise tool arguments and avoid extra narration before or after tool use."
                    .into()
            }
            AdapterFamily::GoogleGemini => {
                "Keep tool arguments compact and strongly typed; avoid speculative optional fields."
                    .into()
            }
            AdapterFamily::OllamaNative => {
                "Use only tool arguments that are essential; reduced-schema models are less tolerant of verbose payloads."
                    .into()
            }
            AdapterFamily::TextOnlyCli => {
                "Do not assume native tool calling is available; if tools are unavailable, answer directly or ask for a supported model."
                    .into()
            }
        }
    }

    pub fn build_routing_prompt_arena(
        arena: &PromptArena,
        request: &AgentRequest,
        history_context: &str,
        candidate_agents: &[AgentConfig],
    ) -> String {
        let user_text = Self::render_user_text(arena, request);
        let mut agents_buffer = bumpalo::collections::String::new_in(&arena.bump);
        use std::fmt::Write;
        let _ = writeln!(agents_buffer, "Candidate Agents:");
        for agent in candidate_agents {
            let _ = writeln!(agents_buffer, "- {}: {}", agent.id, agent.description);
        }

        format!(
            "Prompt Mode: {:?}\nRoute this request to the best matching agent. Do not execute tools.\n{}\n\nRecent Session Context:\n{}\n\nUser Request (channel={:?}):\n{}",
            PromptMode::Routing,
            agents_buffer,
            history_context,
            request.channel,
            user_text
        )
    }

    pub fn build_routing_prompt(
        request: &AgentRequest,
        history_context: &str,
        candidate_agents: &[AgentConfig],
    ) -> String {
        let arena = PromptArena::new();
        Self::build_routing_prompt_arena(&arena, request, history_context, candidate_agents)
    }

    pub fn build_scheduling_context(
        mode: &str,
        rationale: &str,
        normalized_schedule_json: Option<&str>,
        deferred_task: Option<&str>,
        timezone_name: &str,
        local_now: &str,
    ) -> String {
        let mut lines = vec![
            format!("Prompt Mode: {:?}", PromptMode::Scheduling),
            format!(
                "<request_timezone>\niana={}\nlocal_now={}\n</request_timezone>",
                timezone_name, local_now
            ),
            "<request_classifier>".to_string(),
            "scheduling_intent=true".to_string(),
            format!("mode={}", mode),
            format!("rationale={}", rationale),
        ];
        if let Some(schedule_json) = normalized_schedule_json {
            lines.push(format!("normalized_schedule_json={}", schedule_json));
        }
        if let Some(task) = deferred_task {
            lines.push(format!("deferred_task={}", task));
        }
        lines.push(
            "When using schedule_message/set_reminder, respect this classified mode unless the user explicitly asked otherwise.".to_string(),
        );
        lines.push(
            "Prefer schedule.kind='at' for one-shot requests. Use schedule.kind='every' only for true repeating intervals. Use schedule.kind='daily' or 'weekly' only for recurring wall-clock schedules.".to_string(),
        );
        lines.push(
            "Never send schedule='{}' or an empty schedule object; use normalized_schedule_json when present."
                .to_string(),
        );
        lines.push("</request_classifier>".to_string());
        lines.join("\n")
    }

    pub fn build_clarification_message(candidates: &[String]) -> String {
        format!(
            "Prompt Mode: {:?}\nI am not confident which agent should handle this request. Please choose one of: {}.",
            PromptMode::Clarification,
            candidates.join(", ")
        )
    }

    pub fn build_planning_prompt(goal: &str, risk_context: &str, tools_summary: &str) -> String {
        format!(
            "Prompt Mode: {:?}\nPlan the next steps before acting. Prefer minimal-step execution, ask for clarification if the goal is ambiguous, and do not assume missing details.\n\
            Goal:\n{}\n\nRisk Context:\n{}\n\nAvailable Tools Summary:\n{}",
            PromptMode::Planning,
            goal,
            risk_context,
            tools_summary
        )
    }

    pub fn build_summarization_prompt(conversation_snippet: &str) -> String {
        format!(
            "Prompt Mode: {:?}\nYou are an AI memory manager. Analyze the conversation snippet below and extract durable constraints plus a concise summary.\n\
            1. Identify any long-term durable user constraints and return them as a JSON array under 'durable_constraints' (max 8 items).\n\
            2. Provide a concise summary under 'summary'.\n\
            Return ONLY valid JSON. Example: {{\"durable_constraints\": [\"constraint 1\"], \"summary\": \"User asked to build a web server\"}}\n\n\
            Conversation:\n{}",
            PromptMode::Summarization,
            conversation_snippet
        )
    }

    pub fn build_media_prompt(media_type: &str, extracted_context: &str) -> String {
        format!(
            "Prompt Mode: {:?}\nInterpret the provided {} context conservatively. Prefer direct extraction over speculation.\n\nMedia Context:\n{}",
            PromptMode::Media,
            media_type,
            extracted_context
        )
    }

    pub fn build_robotics_prompt(
        intent: &str,
        state_context: &str,
        safety_context: &str,
    ) -> String {
        format!(
            "Prompt Mode: {:?}\nGenerate only high-level robot intent, never direct actuator commands.\n\
            Intent Request:\n{}\n\nRobot State:\n{}\n\nSafety Envelope:\n{}",
            PromptMode::Robotics,
            intent,
            state_context,
            safety_context
        )
    }
}
