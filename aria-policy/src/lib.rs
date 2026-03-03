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
        for pair in args_raw.split(',') {
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
            let value = raw_val.trim_matches('"').trim_matches('\'').to_string();
            args.insert(key.to_string(), value);
        }
    }

    let resource = args
        .get("resource")
        .or_else(|| args.get("path"))
        .or_else(|| args.get("node"))
        .or_else(|| args.get("target"))
        .or_else(|| args.get("file"))
        .cloned()
        .unwrap_or_else(|| "global".to_string());

    Ok(ParsedAstAction {
        action,
        resource,
        arguments: args,
    })
}

// ---------------------------------------------------------------------------
// Decision
// ---------------------------------------------------------------------------

/// Authorization decision returned by the evaluator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// The request is authorized.
    Allow,
    /// The request is denied.
    Deny,
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
}
