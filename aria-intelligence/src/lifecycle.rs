use super::*;
use std::future::Future;
use std::pin::Pin;

use aria_core::{ContextBlock, GatewayChannel, Uuid};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LifecycleHookPhase {
    SessionStart,
    PromptSubmit,
    PreTool,
    PermissionRequest,
    PostTool,
    PreCompact,
    PostCompact,
    SubAgentStart,
    SubAgentStop,
    ApprovalResume,
    SessionEnd,
}

#[derive(Debug, Clone, Default)]
pub struct LifecycleHookEvent {
    pub phase: Option<LifecycleHookPhase>,
    pub request_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub agent_id: Option<String>,
    pub channel: Option<GatewayChannel>,
    pub tool_name: Option<String>,
    pub run_id: Option<String>,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LifecycleHookEffect {
    ContextBlock(ContextBlock),
    AuditNote(String),
}

pub type AsyncLifecycleHookFn = Box<
    dyn Fn(
            LifecycleHookEvent,
        )
            -> Pin<Box<dyn Future<Output = Result<Vec<LifecycleHookEffect>, OrchestratorError>> + Send>>
        + Send
        + Sync,
>;

#[derive(Default)]
pub struct LifecycleHookRegistry {
    session_start: Vec<AsyncLifecycleHookFn>,
    prompt_submit: Vec<AsyncLifecycleHookFn>,
    pre_tool: Vec<AsyncLifecycleHookFn>,
    permission_request: Vec<AsyncLifecycleHookFn>,
    post_tool: Vec<AsyncLifecycleHookFn>,
    pre_compact: Vec<AsyncLifecycleHookFn>,
    post_compact: Vec<AsyncLifecycleHookFn>,
    subagent_start: Vec<AsyncLifecycleHookFn>,
    subagent_stop: Vec<AsyncLifecycleHookFn>,
    approval_resume: Vec<AsyncLifecycleHookFn>,
    session_end: Vec<AsyncLifecycleHookFn>,
}

impl LifecycleHookRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_session_start(&mut self, hook: AsyncLifecycleHookFn) {
        self.register(LifecycleHookPhase::SessionStart, hook);
    }

    pub fn register(&mut self, phase: LifecycleHookPhase, hook: AsyncLifecycleHookFn) {
        self.handlers_mut(phase).push(hook);
    }

    pub fn register_prompt_submit(&mut self, hook: AsyncLifecycleHookFn) {
        self.register(LifecycleHookPhase::PromptSubmit, hook);
    }

    pub fn register_pre_tool(&mut self, hook: AsyncLifecycleHookFn) {
        self.register(LifecycleHookPhase::PreTool, hook);
    }

    pub fn register_permission_request(&mut self, hook: AsyncLifecycleHookFn) {
        self.register(LifecycleHookPhase::PermissionRequest, hook);
    }

    pub fn register_post_tool(&mut self, hook: AsyncLifecycleHookFn) {
        self.register(LifecycleHookPhase::PostTool, hook);
    }

    pub fn register_pre_compact(&mut self, hook: AsyncLifecycleHookFn) {
        self.register(LifecycleHookPhase::PreCompact, hook);
    }

    pub fn register_post_compact(&mut self, hook: AsyncLifecycleHookFn) {
        self.register(LifecycleHookPhase::PostCompact, hook);
    }

    pub fn register_subagent_start(&mut self, hook: AsyncLifecycleHookFn) {
        self.register(LifecycleHookPhase::SubAgentStart, hook);
    }

    pub fn register_subagent_stop(&mut self, hook: AsyncLifecycleHookFn) {
        self.register(LifecycleHookPhase::SubAgentStop, hook);
    }

    pub fn register_approval_resume(&mut self, hook: AsyncLifecycleHookFn) {
        self.register(LifecycleHookPhase::ApprovalResume, hook);
    }

    pub fn register_session_end(&mut self, hook: AsyncLifecycleHookFn) {
        self.register(LifecycleHookPhase::SessionEnd, hook);
    }

    pub async fn execute(
        &self,
        phase: LifecycleHookPhase,
        mut event: LifecycleHookEvent,
    ) -> Result<Vec<LifecycleHookEffect>, OrchestratorError> {
        event.phase = Some(phase);
        let mut effects = Vec::new();
        for hook in self.handlers(phase) {
            effects.extend(hook(event.clone()).await?);
        }
        Ok(effects)
    }

    fn handlers(&self, phase: LifecycleHookPhase) -> &[AsyncLifecycleHookFn] {
        match phase {
            LifecycleHookPhase::SessionStart => &self.session_start,
            LifecycleHookPhase::PromptSubmit => &self.prompt_submit,
            LifecycleHookPhase::PreTool => &self.pre_tool,
            LifecycleHookPhase::PermissionRequest => &self.permission_request,
            LifecycleHookPhase::PostTool => &self.post_tool,
            LifecycleHookPhase::PreCompact => &self.pre_compact,
            LifecycleHookPhase::PostCompact => &self.post_compact,
            LifecycleHookPhase::SubAgentStart => &self.subagent_start,
            LifecycleHookPhase::SubAgentStop => &self.subagent_stop,
            LifecycleHookPhase::ApprovalResume => &self.approval_resume,
            LifecycleHookPhase::SessionEnd => &self.session_end,
        }
    }

    fn handlers_mut(&mut self, phase: LifecycleHookPhase) -> &mut Vec<AsyncLifecycleHookFn> {
        match phase {
            LifecycleHookPhase::SessionStart => &mut self.session_start,
            LifecycleHookPhase::PromptSubmit => &mut self.prompt_submit,
            LifecycleHookPhase::PreTool => &mut self.pre_tool,
            LifecycleHookPhase::PermissionRequest => &mut self.permission_request,
            LifecycleHookPhase::PostTool => &mut self.post_tool,
            LifecycleHookPhase::PreCompact => &mut self.pre_compact,
            LifecycleHookPhase::PostCompact => &mut self.post_compact,
            LifecycleHookPhase::SubAgentStart => &mut self.subagent_start,
            LifecycleHookPhase::SubAgentStop => &mut self.subagent_stop,
            LifecycleHookPhase::ApprovalResume => &mut self.approval_resume,
            LifecycleHookPhase::SessionEnd => &mut self.session_end,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audit_hook(note: &'static str) -> AsyncLifecycleHookFn {
        Box::new(move |_| {
            Box::pin(async move { Ok(vec![LifecycleHookEffect::AuditNote(note.to_string())]) })
        })
    }

    #[tokio::test]
    async fn lifecycle_registry_runs_hooks_in_registration_order() {
        let mut registry = LifecycleHookRegistry::new();
        registry.register_prompt_submit(audit_hook("first"));
        registry.register_prompt_submit(audit_hook("second"));

        let effects = registry
            .execute(
                LifecycleHookPhase::PromptSubmit,
                LifecycleHookEvent::default(),
            )
            .await
            .expect("prompt_submit hooks");

        assert_eq!(
            effects,
            vec![
                LifecycleHookEffect::AuditNote("first".into()),
                LifecycleHookEffect::AuditNote("second".into())
            ]
        );
    }

    #[tokio::test]
    async fn lifecycle_registry_is_phase_scoped() {
        let mut registry = LifecycleHookRegistry::new();
        registry.register_session_start(audit_hook("session"));
        registry.register_post_tool(audit_hook("post_tool"));

        let session_effects = registry
            .execute(
                LifecycleHookPhase::SessionStart,
                LifecycleHookEvent::default(),
            )
            .await
            .expect("session_start hooks");
        let post_tool_effects = registry
            .execute(LifecycleHookPhase::PostTool, LifecycleHookEvent::default())
            .await
            .expect("post_tool hooks");

        assert_eq!(
            session_effects,
            vec![LifecycleHookEffect::AuditNote("session".into())]
        );
        assert_eq!(
            post_tool_effects,
            vec![LifecycleHookEffect::AuditNote("post_tool".into())]
        );
    }

    #[tokio::test]
    async fn lifecycle_registry_propagates_hook_errors() {
        let mut registry = LifecycleHookRegistry::new();
        registry.register_pre_tool(Box::new(|_| {
            Box::pin(async {
                Err(OrchestratorError::ToolError(
                    "pre_tool hook rejected request".into(),
                ))
            })
        }));

        let error = registry
            .execute(LifecycleHookPhase::PreTool, LifecycleHookEvent::default())
            .await
            .expect_err("pre_tool hook should fail");

        assert!(
            error
                .to_string()
                .contains("pre_tool hook rejected request"),
            "unexpected error: {error}"
        );
    }
}
