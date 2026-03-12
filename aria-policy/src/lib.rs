//! # aria-policy
//!
//! Cedar-based zero-trust policy engine for ARIA-X.
//!
//! Evaluates authorization requests against Cedar policies to determine
//! whether an agent action should be allowed or denied. This implements
//! the AST-level security evaluation described in the L3 architecture.
//!
//! ## Quick Example
//! ```rust,no_run
//! use aria_policy::{CedarEvaluator, Decision};
//!
//! let evaluator = CedarEvaluator::from_policy_str(r#"
//!     permit(principal, action == Action::"read_file", resource)
//!     when { resource.path like "/workspace/*" };
//! "#).unwrap();
//!
//! let decision = evaluator.evaluate("developer", "read_file", "/workspace/main.rs").unwrap();
//! assert_eq!(decision, Decision::Allow);
//! ```

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors from the policy evaluation engine.
#[derive(Debug)]
pub enum PolicyError {
    /// The Cedar policy source could not be parsed.
    PolicyParseError(String),
    /// Failed to construct or evaluate a Cedar request.
    EvaluationError(String),
    /// Schema or entity definition error.
    EntityError(String),
    /// Failed to parse an LLM-emitted AST action call.
    AstParseError(String),
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyError::PolicyParseError(msg) => write!(f, "policy parse error: {}", msg),
            PolicyError::EvaluationError(msg) => write!(f, "evaluation error: {}", msg),
            PolicyError::EntityError(msg) => write!(f, "entity error: {}", msg),
            PolicyError::AstParseError(msg) => write!(f, "ast parse error: {}", msg),
        }
    }
}

impl std::error::Error for PolicyError {}

/// Parsed LLM action call (e.g. `read_sensor(node="relay_01")`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAstAction {
    pub action: String,
    pub resource: String,
    pub arguments: HashMap<String, String>,
}

/// Parse an LLM tool-call string into a structured action for Cedar requests.
pub fn parse_ast_action(input: &str) -> Result<ParsedAstAction, PolicyError> {
    let trimmed = input.trim();
    let open_idx = trimmed.find('(').ok_or_else(|| {
        PolicyError::AstParseError("missing '(' in AST call expression".to_string())
    })?;
    let close_idx = trimmed.rfind(')').ok_or_else(|| {
        PolicyError::AstParseError("missing ')' in AST call expression".to_string())
    })?;
    if close_idx <= open_idx {
        return Err(PolicyError::AstParseError(
            "malformed AST call expression".to_string(),
        ));
    }

    let action = trimmed[..open_idx].trim().to_string();
    if action.is_empty() {
        return Err(PolicyError::AstParseError(
            "action name cannot be empty".to_string(),
        ));
    }

    let args_raw = &trimmed[open_idx + 1..close_idx];
    let mut args = HashMap::new();
    if !args_raw.trim().is_empty() {
        for pair in split_ast_args(args_raw) {
            let part = pair.trim();
            if part.is_empty() {
                continue;
            }
            let eq_idx = part.find('=').ok_or_else(|| {
                PolicyError::AstParseError(format!("missing '=' in argument '{}'", part))
            })?;
            let key = part[..eq_idx].trim();
            let raw_val = part[eq_idx + 1..].trim();
            if key.is_empty() || raw_val.is_empty() {
                return Err(PolicyError::AstParseError(format!(
                    "invalid argument '{}'",
                    part
                )));
            }
            // Accept quoted strings ("val"), single-quoted ('val'), and bare values (123, /path)
            let value = raw_val.trim_matches('"').trim_matches('\'').to_string();
            args.insert(key.to_string(), value);
        }
    }

    // Priority-ordered resource key resolution
    let resource = args
        .get("resource")
        .or_else(|| args.get("path"))
        .or_else(|| args.get("node"))
        .or_else(|| args.get("target"))
        .or_else(|| args.get("file"))
        .or_else(|| args.get("topic"))
        .or_else(|| args.get("sensor"))
        .or_else(|| args.get("motor"))
        .or_else(|| args.get("endpoint"))
        .cloned()
        .unwrap_or_else(|| "global".to_string());

    Ok(ParsedAstAction {
        action,
        resource,
        arguments: args,
    })
}

fn split_ast_args(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for ch in input.chars() {
        if escaped {
            buf.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => {
                escaped = true;
                buf.push(ch);
            }
            '\'' if !in_double => {
                in_single = !in_single;
                buf.push(ch);
            }
            '"' if !in_single => {
                in_double = !in_double;
                buf.push(ch);
            }
            ',' if !in_single && !in_double => {
                out.push(buf.trim().to_string());
                buf.clear();
            }
            _ => buf.push(ch),
        }
    }
    if !buf.trim().is_empty() {
        out.push(buf.trim().to_string());
    }
    out
}

// ---------------------------------------------------------------------------
// Decision
// ---------------------------------------------------------------------------

/// Extra context passed to `evaluate_with_context`.
///
/// Lets policies reference attributes like `channel`, `blast_radius`, and
/// `prompt_origin` via Cedar's context map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalContext {
    /// Gateway channel that originated the request (e.g. `"telegram"`, `"cli"`).
    pub channel: String,
    /// Estimated number of nodes affected by the action.
    pub blast_radius: u32,
    /// Originating agent or subsystem label.
    pub prompt_origin: String,
    /// Whitelisted path prefixes.
    pub whitelist: Vec<String>,
    /// Forbidden path prefixes.
    pub forbid: Vec<String>,
}

impl Default for EvalContext {
    fn default() -> Self {
        Self {
            channel: "unknown".into(),
            blast_radius: 1,
            prompt_origin: "agent".into(),
            whitelist: vec!["/workspace/".into()],
            forbid: vec!["/etc/".into(), "/usr/".into(), "/var/".into()],
        }
    }
}

/// Authorization decision returned by the evaluator.
///
/// Extends the binary Cedar Allow/Deny with a third `AskUser` path for
/// sensitive actions that require explicit runtime confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// The request is authorized.
    Allow,
    /// The request is denied.
    Deny,
    /// Cedar allows the action, but it is flagged as sensitive and requires
    /// out-of-band user confirmation before proceeding.
    AskUser,
}

// ---------------------------------------------------------------------------
// CedarEvaluator
// ---------------------------------------------------------------------------

/// Cedar policy evaluator for ARIA-X authorization.
///
/// Loads Cedar policies and evaluates requests of the form:
/// `(Principal=Agent::"<id>", Action::"<tool>", Resource::"<target>")`
pub struct CedarEvaluator {
    policy_set: cedar_policy::PolicySet,
}

impl CedarEvaluator {
    /// Parse Cedar policy text and create an evaluator.
    pub fn from_policy_str(src: &str) -> Result<Self, PolicyError> {
        let policy_set: cedar_policy::PolicySet =
            src.parse().map_err(|e: cedar_policy::ParseErrors| {
                PolicyError::PolicyParseError(format!("{}", e))
            })?;
        Ok(Self { policy_set })
    }

    /// Evaluate an authorization request.
    ///
    /// - `principal`: Agent identifier (e.g. `"developer"`)
    /// - `action`: Tool/action name (e.g. `"read_file"`)
    /// - `resource`: Target path or node (e.g. `"/workspace/main.rs"`)
    ///
    /// Returns [`Decision::Allow`] or [`Decision::Deny`].
    pub fn evaluate(
        &self,
        principal: &str,
        action: &str,
        resource: &str,
    ) -> Result<Decision, PolicyError> {
        // Build entity UIDs
        let principal_uid = format!("Agent::\"{}\"", principal)
            .parse::<cedar_policy::EntityUid>()
            .map_err(|e| PolicyError::EvaluationError(format!("principal parse: {}", e)))?;

        let action_uid = format!("Action::\"{}\"", action)
            .parse::<cedar_policy::EntityUid>()
            .map_err(|e| PolicyError::EvaluationError(format!("action parse: {}", e)))?;

        let resource_uid = format!("Resource::\"{}\"", resource)
            .parse::<cedar_policy::EntityUid>()
            .map_err(|e| PolicyError::EvaluationError(format!("resource parse: {}", e)))?;

        // Build context with `path` attribute for policy `when` clauses
        let mut context_map = HashMap::new();
        context_map.insert(
            "path".to_string(),
            cedar_policy::RestrictedExpression::new_string(resource.to_string()),
        );
        let context = cedar_policy::Context::from_pairs(context_map)
            .map_err(|e| PolicyError::EvaluationError(format!("context: {}", e)))?;

        // Build the resource entity with a `path` attribute so policies
        // can reference `resource.path`.
        let resource_entity = cedar_policy::Entity::new(
            resource_uid.clone(),
            {
                let mut attrs = HashMap::new();
                attrs.insert(
                    "path".to_string(),
                    cedar_policy::RestrictedExpression::new_string(resource.to_string()),
                );
                attrs
            },
            std::collections::HashSet::new(),
        )
        .map_err(|e| PolicyError::EntityError(format!("resource entity: {}", e)))?;

        let entities = cedar_policy::Entities::from_entities([resource_entity], None)
            .map_err(|e| PolicyError::EntityError(format!("entities: {}", e)))?;

        // Construct the request
        let request =
            cedar_policy::Request::new(principal_uid, action_uid, resource_uid, context, None)
                .map_err(|e| PolicyError::EvaluationError(format!("request: {}", e)))?;

        // Authorize
        let authorizer = cedar_policy::Authorizer::new();
        let response = authorizer.is_authorized(&request, &self.policy_set, &entities);

        match response.decision() {
            cedar_policy::Decision::Allow => Ok(Decision::Allow),
            cedar_policy::Decision::Deny => Ok(Decision::Deny),
        }
    }

    /// Tri-state evaluation: `Allow`, `Deny`, or `AskUser`.
    ///
    /// If Cedar permits the action but `sensitive_actions` contains the action
    /// name, the decision is upgraded to `AskUser` so the caller can pause and
    /// request explicit confirmation before executing.
    pub fn evaluate_tristate(
        &self,
        principal: &str,
        action: &str,
        resource: &str,
        sensitive_actions: &[&str],
    ) -> Result<Decision, PolicyError> {
        let base = self.evaluate(principal, action, resource)?;
        if base == Decision::Allow && sensitive_actions.contains(&action) {
            return Ok(Decision::AskUser);
        }
        Ok(base)
    }

    /// Evaluate with extra runtime context attributes.
    ///
    /// Passes `channel`, `blast_radius`, and `prompt_origin` into the Cedar
    /// context so policies can reference them in `when` / `unless` clauses.
    pub fn evaluate_with_context(
        &self,
        principal: &str,
        action: &str,
        resource: &str,
        ctx: &EvalContext,
    ) -> Result<Decision, PolicyError> {
        // --- Whitelist/Forbid Path Check ---
        // If resource is "global" or "agent", we skip path-based checks.
        if resource != "global" && resource != "agent" {
            // Check forbidden prefixes first (explicit deny)
            for f in &ctx.forbid {
                if resource.starts_with(f) {
                    return Ok(Decision::Deny);
                }
            }

            // Check whitelisted prefixes (implicit allow zone)
            let mut whitelisted = false;
            for w in &ctx.whitelist {
                if resource.starts_with(w) {
                    whitelisted = true;
                    break;
                }
            }

            // If it's a path-based resource and not in a whitelisted zone, deny.
            // We assume resources containing '/' or starting with './' or '../' are paths.
            if !whitelisted
                && (resource.contains('/')
                    || resource.starts_with('.')
                    || !resource.chars().all(|c| c.is_alphanumeric() || c == '_'))
            {
                return Ok(Decision::Deny);
            }
        }

        let principal_uid = format!("Agent::\"{}\"", principal)
            .parse::<cedar_policy::EntityUid>()
            .map_err(|e| PolicyError::EvaluationError(format!("principal parse: {}", e)))?;
        let action_uid = format!("Action::\"{}\"", action)
            .parse::<cedar_policy::EntityUid>()
            .map_err(|e| PolicyError::EvaluationError(format!("action parse: {}", e)))?;
        let resource_uid = format!("Resource::\"{}\"", resource)
            .parse::<cedar_policy::EntityUid>()
            .map_err(|e| PolicyError::EvaluationError(format!("resource parse: {}", e)))?;

        let mut context_map = HashMap::new();
        context_map.insert(
            "path".to_string(),
            cedar_policy::RestrictedExpression::new_string(resource.to_string()),
        );
        context_map.insert(
            "channel".to_string(),
            cedar_policy::RestrictedExpression::new_string(ctx.channel.clone()),
        );
        context_map.insert(
            "blast_radius".to_string(),
            cedar_policy::RestrictedExpression::new_long(i64::from(ctx.blast_radius)),
        );
        context_map.insert(
            "prompt_origin".to_string(),
            cedar_policy::RestrictedExpression::new_string(ctx.prompt_origin.clone()),
        );
        let context = cedar_policy::Context::from_pairs(context_map)
            .map_err(|e| PolicyError::EvaluationError(format!("context: {}", e)))?;

        let resource_entity = cedar_policy::Entity::new(
            resource_uid.clone(),
            {
                let mut attrs = HashMap::new();
                attrs.insert(
                    "path".to_string(),
                    cedar_policy::RestrictedExpression::new_string(resource.to_string()),
                );
                attrs
            },
            std::collections::HashSet::new(),
        )
        .map_err(|e| PolicyError::EntityError(format!("resource entity: {}", e)))?;

        let entities = cedar_policy::Entities::from_entities([resource_entity], None)
            .map_err(|e| PolicyError::EntityError(format!("entities: {}", e)))?;

        let request =
            cedar_policy::Request::new(principal_uid, action_uid, resource_uid, context, None)
                .map_err(|e| PolicyError::EvaluationError(format!("request: {}", e)))?;

        let authorizer = cedar_policy::Authorizer::new();
        let response = authorizer.is_authorized(&request, &self.policy_set, &entities);
        match response.decision() {
            cedar_policy::Decision::Allow => Ok(Decision::Allow),
            cedar_policy::Decision::Deny => Ok(Decision::Deny),
        }
    }

    /// Hot-reload policy from a new source string without restarting.
    ///
    /// On parse failure the existing policy set is kept intact.
    pub fn reload_from_str(&mut self, src: &str) -> Result<(), PolicyError> {
        let new_set: cedar_policy::PolicySet =
            src.parse().map_err(|e: cedar_policy::ParseErrors| {
                PolicyError::PolicyParseError(format!("{}", e))
            })?;
        self.policy_set = new_set;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests — TDD
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The default ARIA-X policy set.
    const DEFAULT_POLICY: &str = r#"
        permit (
            principal,
            action == Action::"read_file",
            resource
        )
        when { resource.path like "/workspace/*" };

        permit (
            principal,
            action == Action::"read_sensor",
            resource
        );

        forbid (
            principal,
            action,
            resource
        )
        when { resource.path like "/etc/*" };

        forbid (
            principal,
            action,
            resource
        )
        when { resource.path like "/usr/*" };
    "#;

    fn evaluator() -> CedarEvaluator {
        CedarEvaluator::from_policy_str(DEFAULT_POLICY).expect("policy parse")
    }

    // =====================================================================
    // Core authorization tests (from Phase 2 spec)
    // =====================================================================

    #[test]
    fn deny_read_etc_shadow() {
        let eval = evaluator();
        let decision = eval
            .evaluate("developer", "read_file", "/etc/shadow")
            .expect("evaluate");
        assert_eq!(decision, Decision::Deny, "/etc/shadow should be denied");
    }

    #[test]
    fn allow_read_workspace_file() {
        let eval = evaluator();
        let decision = eval
            .evaluate("developer", "read_file", "/workspace/main.rs")
            .expect("evaluate");
        assert_eq!(
            decision,
            Decision::Allow,
            "/workspace/main.rs should be allowed"
        );
    }

    // =====================================================================
    // Extended authorization tests
    // =====================================================================

    #[test]
    fn deny_read_usr_path() {
        let eval = evaluator();
        let decision = eval
            .evaluate("developer", "read_file", "/usr/bin/bash")
            .expect("evaluate");
        assert_eq!(decision, Decision::Deny);
    }

    #[test]
    fn deny_unknown_action_on_workspace() {
        let eval = evaluator();
        // "exec_shell" is not explicitly permitted on workspace resources
        let decision = eval
            .evaluate("developer", "exec_shell", "/workspace/script.sh")
            .expect("evaluate");
        assert_eq!(
            decision,
            Decision::Deny,
            "unknown actions should default-deny"
        );
    }

    #[test]
    fn allow_read_sensor() {
        let eval = evaluator();
        let decision = eval
            .evaluate("developer", "read_sensor", "relay_01")
            .expect("evaluate");
        assert_eq!(decision, Decision::Allow, "read_sensor should be allowed");
    }

    #[test]
    fn deny_write_to_workspace_not_permitted() {
        let eval = evaluator();
        // "write_file" is not in the permit rules
        let decision = eval
            .evaluate("developer", "write_file", "/workspace/data.json")
            .expect("evaluate");
        assert_eq!(
            decision,
            Decision::Deny,
            "write_file not explicitly permitted"
        );
    }

    // =====================================================================
    // Error handling tests
    // =====================================================================

    #[test]
    fn policy_parse_error_on_invalid_syntax() {
        let result = CedarEvaluator::from_policy_str("this is not valid cedar {{{");
        assert!(result.is_err());
        match result {
            Err(PolicyError::PolicyParseError(msg)) => {
                assert!(!msg.is_empty(), "error message should be non-empty");
            }
            _ => panic!("expected PolicyParseError"),
        }
    }

    #[test]
    fn empty_policy_denies_everything() {
        // No policies → default deny
        let eval = CedarEvaluator::from_policy_str("").expect("empty is valid");
        let decision = eval
            .evaluate("anyone", "anything", "/anywhere")
            .expect("evaluate");
        assert_eq!(decision, Decision::Deny, "empty policy set should deny all");
    }

    // =====================================================================
    // Error type display
    // =====================================================================

    // -------------------------------------------------------------------------------------
    // Contextual evaluation tests
    // -------------------------------------------------------------------------------------

    const CONTEXTUAL_POLICY: &str = r#"
        permit (principal, action == Action::"read_file", resource)
            when { resource.path like "/workspace/*" };

        forbid (principal, action, resource)
            when { context.blast_radius > 5 };

        forbid (principal, action, resource)
            when { context.channel == "public" };

        forbid (principal, action == Action::"write_motor", resource);
    "#;

    fn ctx_evaluator() -> CedarEvaluator {
        CedarEvaluator::from_policy_str(CONTEXTUAL_POLICY).expect("ctx policy parse")
    }

    #[test]
    fn contextual_eval_allows_low_blast_radius() {
        let eval = ctx_evaluator();
        let ctx = EvalContext {
            channel: "cli".into(),
            blast_radius: 1,
            prompt_origin: "developer".into(),
            whitelist: vec!["/workspace/".into()],
            forbid: vec![],
        };
        let d = eval
            .evaluate_with_context("dev", "read_file", "/workspace/a", &ctx)
            .unwrap();
        assert_eq!(d, Decision::Allow);
    }

    #[test]
    fn contextual_eval_denies_high_blast_radius() {
        let eval = ctx_evaluator();
        let ctx = EvalContext {
            channel: "cli".into(),
            blast_radius: 10,
            prompt_origin: "developer".into(),
            whitelist: vec!["/workspace/".into()],
            forbid: vec![],
        };
        let d = eval
            .evaluate_with_context("dev", "read_file", "/workspace/a", &ctx)
            .unwrap();
        assert_eq!(d, Decision::Deny, "high blast-radius should be denied");
    }

    #[test]
    fn contextual_eval_denies_public_channel() {
        let eval = ctx_evaluator();
        let ctx = EvalContext {
            channel: "public".into(),
            blast_radius: 1,
            prompt_origin: "developer".into(),
            whitelist: vec!["/workspace/".into()],
            forbid: vec![],
        };
        let d = eval
            .evaluate_with_context("dev", "read_file", "/workspace/a", &ctx)
            .unwrap();
        assert_eq!(d, Decision::Deny, "public channel should be denied");
    }

    // -------------------------------------------------------------------------------------
    // Channel trust boundary test matrix
    // -------------------------------------------------------------------------------------

    const CHANNEL_POLICY: &str = r#"
        permit (principal == Agent::"trusted_cli", action == Action::"read_file", resource)
            when { resource.path like "/workspace/*" };

        forbid (principal == Agent::"telegram_bot", action == Action::"write_file", resource);
    "#;

    #[test]
    fn channel_trust_matrix_trusted_cli_read_allowed() {
        let eval = CedarEvaluator::from_policy_str(CHANNEL_POLICY).unwrap();
        let d = eval
            .evaluate("trusted_cli", "read_file", "/workspace/src")
            .unwrap();
        assert_eq!(d, Decision::Allow);
    }

    #[test]
    fn channel_trust_matrix_telegram_bot_write_denied() {
        let eval = CedarEvaluator::from_policy_str(CHANNEL_POLICY).unwrap();
        let d = eval
            .evaluate("telegram_bot", "write_file", "/workspace/out")
            .unwrap();
        assert_eq!(d, Decision::Deny);
    }

    #[test]
    fn channel_trust_matrix_untrusted_agent_denied_by_default() {
        let eval = CedarEvaluator::from_policy_str(CHANNEL_POLICY).unwrap();
        let d = eval
            .evaluate("unknown_agent", "read_file", "/workspace/src")
            .unwrap();
        assert_eq!(d, Decision::Deny, "no matching permit rule → deny");
    }

    // -------------------------------------------------------------------------------------
    // Robotics safety: forbid write-path actions fail-closed
    // -------------------------------------------------------------------------------------

    const ROBOTICS_POLICY: &str = r#"
        permit (principal, action == Action::"read_sensor", resource);

        forbid (principal, action == Action::"write_motor", resource);
        forbid (principal, action == Action::"set_pwm", resource);
        forbid (principal, action == Action::"emergency_stop_override", resource);
    "#;

    fn robotics_eval() -> CedarEvaluator {
        CedarEvaluator::from_policy_str(ROBOTICS_POLICY).unwrap()
    }

    #[test]
    fn robotics_read_sensor_allowed() {
        let d = robotics_eval()
            .evaluate("robotics_ctrl", "read_sensor", "imu_01")
            .unwrap();
        assert_eq!(d, Decision::Allow);
    }

    #[test]
    fn robotics_write_motor_denied_fail_closed() {
        let d = robotics_eval()
            .evaluate("robotics_ctrl", "write_motor", "motor_01")
            .unwrap();
        assert_eq!(d, Decision::Deny, "raw motor writes must always be denied");
    }

    #[test]
    fn robotics_set_pwm_denied_fail_closed() {
        let d = robotics_eval()
            .evaluate("robotics_ctrl", "set_pwm", "pwm_01")
            .unwrap();
        assert_eq!(d, Decision::Deny, "raw PWM from LLM is forbidden");
    }

    #[test]
    fn robotics_emergency_stop_override_denied() {
        let d = robotics_eval()
            .evaluate("orchestrator", "emergency_stop_override", "safety_relay")
            .unwrap();
        assert_eq!(d, Decision::Deny, "no-override invariant must hold");
    }

    #[test]
    fn ast_parser_unquoted_numeric_value() {
        let parsed = parse_ast_action("set_motor(motor=motor_01, speed=100)").expect("parse");
        assert_eq!(parsed.action, "set_motor");
        assert_eq!(parsed.arguments.get("speed"), Some(&"100".to_string()));
    }

    #[test]
    fn ast_parser_unquoted_path_value() {
        let parsed = parse_ast_action("read_file(path=/workspace/main.rs)").expect("parse");
        assert_eq!(parsed.resource, "/workspace/main.rs");
    }

    #[test]
    fn ast_parser_sensor_resource_key() {
        let parsed = parse_ast_action(r#"read_sensor(sensor="imu_01")"#).expect("parse");
        assert_eq!(parsed.resource, "imu_01");
    }

    #[test]
    fn tristate_returns_ask_user_for_sensitive_allowed_action() {
        let eval = evaluator();
        let decision = eval
            .evaluate_tristate(
                "developer",
                "read_file",
                "/workspace/main.rs",
                &["read_file"],
            )
            .expect("evaluate");
        assert_eq!(
            decision,
            Decision::AskUser,
            "allowed but sensitive action should be AskUser"
        );
    }

    #[test]
    fn tristate_returns_allow_when_not_sensitive() {
        let eval = evaluator();
        let decision = eval
            .evaluate_tristate("developer", "read_file", "/workspace/main.rs", &[])
            .expect("evaluate");
        assert_eq!(decision, Decision::Allow);
    }

    #[test]
    fn tristate_returns_deny_regardless_of_sensitive_list() {
        let eval = evaluator();
        let decision = eval
            .evaluate_tristate("developer", "read_file", "/etc/shadow", &["read_file"])
            .expect("evaluate");
        assert_eq!(decision, Decision::Deny, "denied action must stay Deny");
    }

    #[test]
    fn policy_reload_takes_effect() {
        let mut eval = CedarEvaluator::from_policy_str("").expect("empty");
        let d1 = eval
            .evaluate("dev", "read_file", "/workspace/a")
            .expect("eval");
        assert_eq!(d1, Decision::Deny, "empty policy denies");

        eval.reload_from_str(DEFAULT_POLICY).expect("reload");
        let d2 = eval
            .evaluate("dev", "read_file", "/workspace/a")
            .expect("eval");
        assert_eq!(d2, Decision::Allow, "reloaded policy should allow");
    }

    #[test]
    fn policy_reload_keeps_existing_on_parse_failure() {
        let mut eval = evaluator();
        let err = eval.reload_from_str("{{{bad cedar");
        assert!(err.is_err(), "bad policy should fail");
        // original policy still intact
        let d = eval
            .evaluate("dev", "read_file", "/workspace/a")
            .expect("eval");
        assert_eq!(d, Decision::Allow, "original policy should still be active");
    }

    #[test]
    fn error_display() {
        let err = PolicyError::PolicyParseError("bad syntax".into());
        assert!(format!("{}", err).contains("policy parse error"));

        let err = PolicyError::EvaluationError("bad request".into());
        assert!(format!("{}", err).contains("evaluation error"));

        let err = PolicyError::EntityError("bad entity".into());
        assert!(format!("{}", err).contains("entity error"));

        let err = PolicyError::AstParseError("bad ast".into());
        assert!(format!("{}", err).contains("ast parse error"));
    }

    #[test]
    fn parse_ast_action_read_sensor() {
        let parsed = parse_ast_action(r#"read_sensor(node="relay_01")"#).expect("parse");
        assert_eq!(parsed.action, "read_sensor");
        assert_eq!(parsed.resource, "relay_01");
        assert_eq!(parsed.arguments.get("node"), Some(&"relay_01".to_string()));
    }

    #[test]
    fn parse_ast_action_read_file_with_path() {
        let parsed = parse_ast_action(r#"read_file(path="/workspace/main.rs")"#).expect("parse");
        assert_eq!(parsed.action, "read_file");
        assert_eq!(parsed.resource, "/workspace/main.rs");
    }

    #[test]
    fn parse_ast_action_invalid_syntax() {
        let parsed = parse_ast_action("read_file path=/workspace/main.rs");
        assert!(parsed.is_err());
    }

    #[test]
    fn parse_ast_action_handles_commas_inside_quoted_values() {
        let parsed = parse_ast_action(
            r#"schedule_message(task="Random number: 7, Random name: Alex", delay="1m")"#,
        )
        .expect("parse");
        assert_eq!(parsed.action, "schedule_message");
        assert_eq!(
            parsed.arguments.get("task"),
            Some(&"Random number: 7, Random name: Alex".to_string())
        );
        assert_eq!(parsed.arguments.get("delay"), Some(&"1m".to_string()));
    }

    #[test]
    fn parse_ast_action_handles_multiline_quoted_values() {
        let parsed = parse_ast_action(
            "schedule_message(task=\"Line1\nLine2, Name: Alex\", delay=\"1m\")",
        )
        .expect("parse");
        assert_eq!(
            parsed.arguments.get("task"),
            Some(&"Line1\nLine2, Name: Alex".to_string())
        );
    }
}
