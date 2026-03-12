//! # aria-core
//!
//! Foundational types for the ARIA-X architecture.
//! This crate is `#![no_std]` compatible with the `alloc` crate.
//!
//! ## Types
//! - [`AgentRequest`] — Inbound user request normalized across all channels
//! - [`AgentResponse`] — Outbound agent response with skill trace
//! - [`ToolDefinition`] — Tool metadata including JSON schema and embedding vector
//! - [`HardwareIntent`] — Low-level motor/actuator command for HAL layer

#![no_std]

extern crate alloc;

use alloc::collections::{BTreeSet, VecDeque};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Unified error type for aria-core operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AriaError {
    /// Serialization or deserialization failed.
    SerializationError(String),
    /// A required field was invalid or missing.
    ValidationError(String),
}

impl fmt::Display for AriaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AriaError::SerializationError(msg) => write!(f, "serialization error: {}", msg),
            AriaError::ValidationError(msg) => write!(f, "validation error: {}", msg),
        }
    }
}

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// A universally-unique identifier stored as 16 raw bytes.
/// This representation is `no_std`-safe — no heap allocation required
/// for the ID itself.
pub type Uuid = [u8; 16];

/// Inbound user request normalized across all gateway channels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRequest {
    /// Unique identifier for this request.
    pub request_id: Uuid,
    /// Session this request belongs to.
    pub session_id: Uuid,
    /// Gateway channel that produced this request.
    pub channel: GatewayChannel,
    /// Identifier of the requesting user.
    pub user_id: String,
    /// Normalized request payload content.
    pub content: MessageContent,
    /// Timestamp in microseconds since UNIX epoch.
    pub timestamp_us: u64,
}

/// Outbound agent response sent back through the gateway.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentResponse {
    /// The request this response corresponds to.
    pub request_id: Uuid,
    /// Generated response content.
    pub content: MessageContent,
    /// Ordered records of tools that contributed to this response.
    pub skill_trace: Vec<SkillExecutionRecord>,
    /// End-to-end response latency in milliseconds.
    pub latency_ms: u32,
}

/// Unified payload model used by both requests and responses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MessageContent {
    Text(String),
    Image {
        url: String,
        caption: Option<String>,
    },
    Audio {
        url: String,
        transcript: Option<String>,
    },
    Location {
        lat: f64,
        lng: f64,
    },
}

impl MessageContent {
    /// Returns inner text if this is a `Text` payload.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            MessageContent::Text(text) => Some(text.as_str()),
            _ => None,
        }
    }
}

/// Source channel type used by the gateway layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GatewayChannel {
    Telegram,
    WhatsApp,
    Discord,
    Slack,
    IMessage,
    Cli,
    WebSocket,
    Ros2,
    Unknown,
}

/// Policy decision associated with each tool execution record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyDecision {
    Allow,
    Deny,
    AskUser,
}

/// Telemetry record for an executed tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillExecutionRecord {
    /// Tool name (e.g. `read_file`).
    pub tool_name: String,
    /// JSON-encoded arguments used for invocation.
    pub arguments_json: String,
    /// Short summary of the result returned by runtime.
    pub result_summary: String,
    /// Runtime duration in milliseconds for this tool call.
    pub duration_ms: u32,
    /// Authorization decision observed before execution.
    pub policy_decision: PolicyDecision,
}

/// High-level telemetry log entry used by the distillation engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetryLog {
    /// Encoded state vector for the trajectory at this step.
    pub state_vector: Vec<f32>,
    /// Tool or action identifier taken by the orchestrator.
    pub mcp_action: String,
    /// Reward score in \[-1.0, 1.0\] assigned by the reward model.
    pub reward_score: f32,
    /// Timestamp in microseconds since UNIX epoch.
    pub timestamp_us: u64,
}

/// Constraint violation emitted by the HAL safety envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstraintViolation {
    /// Node that attempted the unsafe actuation.
    pub node_id: String,
    /// Motor/actuator identifier that was targeted.
    pub motor_id: u8,
    /// Velocity requested by the upstream controller.
    pub requested_velocity: f32,
    /// Maximum safe envelope velocity.
    pub envelope_max: f32,
    /// Timestamp in microseconds since UNIX epoch.
    pub timestamp_us: u64,
}

/// Runtime agent profile as described in the architecture blueprint.
///
/// This struct is intentionally more general than the TOML-backed
/// configuration used by `aria-intelligence`; downstream crates are free
/// to project into lighter-weight views.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub description: String,
    /// Optional pre-computed embedding of `description`.
    pub description_vec: Vec<f32>,
    /// Logical backend identifier (e.g. `"local"`, `"remote-llama"`).
    pub llm_backend: String,
    pub system_prompt: String,
    pub base_tool_names: Vec<String>,
    /// Maximum tools visible to the LLM at any time.
    pub context_cap: u8,
    /// Maximum unique tools ever loaded in this session.
    pub session_tool_ceiling: u8,
    /// Maximum LLM→tool cycles per request.
    pub max_tool_rounds: u8,
    /// Optional fallback agent id to delegate to on failure.
    pub fallback_agent: Option<String>,
}

/// Per-session dynamic tool cache state model.
///
/// The Intelligence layer maintains an in-memory implementation that uses
/// an LRU-backed cache; this struct captures the logical state for
/// telemetry and persistence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DynamicToolCacheState {
    /// Session this cache belongs to.
    pub session_id: Uuid,
    /// LRU-ordered tool names currently in the model context.
    pub active_tools: VecDeque<String>,
    /// All tools that have ever been loaded in this session.
    pub session_loaded: BTreeSet<String>,
    /// Whether `session_tool_ceiling` has been reached.
    pub ceiling_reached: bool,
}

/// Registration metadata for a single skill implementation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillRegistration {
    /// Globally-unique skill identifier.
    pub skill_id: String,
    /// Name of the tool exposed to the LLM.
    pub tool_name: String,
    /// Name of the host that owns this skill (e.g. node id).
    pub host_node_id: String,
}

/// Manifest snapshot of all skills available across the mesh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillManifest {
    /// All registered skills, regardless of node.
    pub registrations: Vec<SkillRegistration>,
}

/// Metadata describing a tool available to the agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Machine-readable tool name (e.g. `"read_file"`).
    pub name: String,
    /// Human-readable description of the tool's purpose.
    pub description: String,
    /// JSON Schema string describing the tool's parameters.
    pub parameters: String,
    /// Pre-computed embedding vector for semantic routing.
    pub embedding: Vec<f32>,
}

/// Low-level hardware actuator command for the HAL layer.
///
/// This struct is intentionally small and fixed-size so it can be
/// serialized without dynamic allocation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HardwareIntent {
    /// Unique identifier for this intent batch.
    pub intent_id: u32,
    /// Target motor/actuator identifier (0–255).
    pub motor_id: u8,
    /// Desired velocity set-point.
    pub target_velocity: f32,
}

// ---------------------------------------------------------------------------
// Helper constructors (available in std-enabled test / downstream crates)
// ---------------------------------------------------------------------------

impl AgentRequest {
    /// Validate that a `Uuid` field contains a plausible value
    /// (not all-zeros).
    pub fn validate_uuid(id: &Uuid) -> Result<(), AriaError> {
        if id.iter().all(|&b| b == 0) {
            return Err(AriaError::ValidationError(String::from(
                "UUID must not be all zeros",
            )));
        }
        Ok(())
    }
}

impl HardwareIntent {
    /// Serialize to postcard bytes without requiring std.
    pub fn to_postcard_bytes(&self) -> Result<Vec<u8>, AriaError> {
        postcard::to_allocvec(self)
            .map_err(|e| AriaError::SerializationError(alloc::format!("{}", e)))
    }

    /// Deserialize from postcard bytes.
    pub fn from_postcard_bytes(bytes: &[u8]) -> Result<Self, AriaError> {
        postcard::from_bytes(bytes)
            .map_err(|e| AriaError::SerializationError(alloc::format!("{}", e)))
    }
}

// ---------------------------------------------------------------------------
// Legacy migration helpers
// ---------------------------------------------------------------------------

/// Legacy v0 representation of `AgentRequest` before `MessageContent`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegacyAgentRequestV0 {
    pub request_id: Uuid,
    pub session_id: Uuid,
    pub channel: GatewayChannel,
    pub user_id: String,
    /// Plain text content field in older payloads.
    pub content: String,
    pub timestamp_us: u64,
}

/// Legacy v0 representation of `AgentResponse` before structured traces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegacyAgentResponseV0 {
    pub request_id: Uuid,
    /// Plain text content field in older payloads.
    pub content: String,
    /// Free-form trace strings in older recordings.
    pub skill_trace: Vec<String>,
    pub latency_ms: u32,
}

impl From<LegacyAgentRequestV0> for AgentRequest {
    fn from(v0: LegacyAgentRequestV0) -> Self {
        AgentRequest {
            request_id: v0.request_id,
            session_id: v0.session_id,
            channel: v0.channel,
            user_id: v0.user_id,
            content: MessageContent::Text(v0.content),
            timestamp_us: v0.timestamp_us,
        }
    }
}

impl From<LegacyAgentResponseV0> for AgentResponse {
    fn from(v0: LegacyAgentResponseV0) -> Self {
        AgentResponse {
            request_id: v0.request_id,
            content: MessageContent::Text(v0.content),
            skill_trace: v0
                .skill_trace
                .into_iter()
                .map(|trace| SkillExecutionRecord {
                    tool_name: String::from("legacy"),
                    arguments_json: String::new(),
                    result_summary: trace,
                    duration_ms: 0,
                    policy_decision: PolicyDecision::Allow,
                })
                .collect(),
            latency_ms: v0.latency_ms,
        }
    }
}

impl AgentRequest {
    /// Attempt to parse either a v0 or v1 JSON-encoded request.
    pub fn from_json_any_version(json: &str) -> Result<Self, AriaError> {
        // First try the current format.
        if let Ok(current) = serde_json::from_str::<AgentRequest>(json) {
            return Ok(current);
        }
        // Fallback to legacy v0.
        let legacy: LegacyAgentRequestV0 = serde_json::from_str(json)
            .map_err(|e: serde_json::Error| AriaError::SerializationError(e.to_string()))?;
        Ok(legacy.into())
    }
}

impl AgentResponse {
    /// Attempt to parse either a v0 or v1 JSON-encoded response.
    pub fn from_json_any_version(json: &str) -> Result<Self, AriaError> {
        // First try the current format.
        if let Ok(current) = serde_json::from_str::<AgentResponse>(json) {
            return Ok(current);
        }
        // Fallback to legacy v0.
        let legacy: LegacyAgentResponseV0 = serde_json::from_str(json)
            .map_err(|e: serde_json::Error| AriaError::SerializationError(e.to_string()))?;
        Ok(legacy.into())
    }
}

// ---------------------------------------------------------------------------
// Tests — written FIRST per TDD mandate
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    extern crate alloc;
    extern crate std;

    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    // -- Helpers -----------------------------------------------------------

    /// Build a deterministic 16-byte UUID from a `uuid` crate v4 value.
    fn make_uuid() -> Uuid {
        *uuid::Uuid::new_v4().as_bytes()
    }

    /// Build a sample `AgentRequest` for testing.
    fn sample_request() -> AgentRequest {
        AgentRequest {
            request_id: make_uuid(),
            session_id: make_uuid(),
            channel: GatewayChannel::Telegram,
            user_id: String::from("user_42"),
            content: MessageContent::Text(String::from("Summarize my notes")),
            timestamp_us: 1_700_000_000_000_000,
        }
    }

    /// Build a sample `AgentResponse`.
    fn sample_response() -> AgentResponse {
        AgentResponse {
            request_id: make_uuid(),
            content: MessageContent::Text(String::from("Here is your summary.")),
            skill_trace: vec![SkillExecutionRecord {
                tool_name: String::from("read_file"),
                arguments_json: String::from(r#"{"path":"/workspace/notes.md"}"#),
                result_summary: String::from("ok"),
                duration_ms: 12,
                policy_decision: PolicyDecision::Allow,
            }],
            latency_ms: 78,
        }
    }

    /// Build a sample `ToolDefinition`.
    fn sample_tool() -> ToolDefinition {
        ToolDefinition {
            name: String::from("read_file"),
            description: String::from("Read contents of a file"),
            parameters: String::from(
                r#"{"type":"object","properties":{"path":{"type":"string"}}}"#,
            ),
            embedding: vec![0.1, 0.2, 0.3, -0.5],
        }
    }

    #[test]
    fn telemetry_log_round_trip_json() {
        let log = TelemetryLog {
            state_vector: vec![0.1, -0.2, 0.3],
            mcp_action: String::from("read_file"),
            reward_score: 0.75,
            timestamp_us: 1_700_000_000_000_000,
        };
        let json = serde_json::to_string(&log).expect("to json");
        let decoded: TelemetryLog = serde_json::from_str(&json).expect("from json");
        assert_eq!(log, decoded);
    }

    #[test]
    fn constraint_violation_round_trip_postcard() {
        let cv = ConstraintViolation {
            node_id: String::from("orchestrator-01"),
            motor_id: 7,
            requested_velocity: 3.0,
            envelope_max: 1.5,
            timestamp_us: 1_700_000_000_000_123,
        };
        let bytes = postcard::to_allocvec(&cv).expect("serialize");
        let decoded: ConstraintViolation = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(cv, decoded);
    }

    #[test]
    fn dynamic_tool_cache_state_serialization() {
        let mut active = VecDeque::new();
        active.push_back(String::from("read_file"));
        active.push_back(String::from("write_note"));
        let mut loaded = BTreeSet::new();
        loaded.insert(String::from("read_file"));
        loaded.insert(String::from("write_note"));

        let state = DynamicToolCacheState {
            session_id: make_uuid(),
            active_tools: active,
            session_loaded: loaded,
            ceiling_reached: true,
        };

        let json = serde_json::to_string(&state).expect("to json");
        let decoded: DynamicToolCacheState = serde_json::from_str(&json).expect("from json");
        assert_eq!(state.session_id, decoded.session_id);
        assert_eq!(state.ceiling_reached, decoded.ceiling_reached);
        assert_eq!(state.active_tools.len(), decoded.active_tools.len());
        assert_eq!(state.session_loaded.len(), decoded.session_loaded.len());
    }

    #[test]
    fn skill_manifest_and_registration_round_trip() {
        let reg = SkillRegistration {
            skill_id: String::from("skill-1"),
            tool_name: String::from("read_file"),
            host_node_id: String::from("node-a"),
        };
        let manifest = SkillManifest {
            registrations: vec![reg],
        };
        let json = serde_json::to_string(&manifest).expect("to json");
        let decoded: SkillManifest = serde_json::from_str(&json).expect("from json");
        assert_eq!(decoded.registrations.len(), 1);
        assert_eq!(decoded.registrations[0].tool_name, "read_file");
    }

    #[test]
    fn legacy_agent_request_v0_migrates_to_current() {
        let v0 = LegacyAgentRequestV0 {
            request_id: make_uuid(),
            session_id: make_uuid(),
            channel: GatewayChannel::Cli,
            user_id: String::from("legacy-user"),
            content: String::from("hello"),
            timestamp_us: 42,
        };
        let json = serde_json::to_string(&v0).expect("to json");
        let migrated = AgentRequest::from_json_any_version(&json).expect("migrate");
        assert_eq!(migrated.user_id, "legacy-user");
        assert_eq!(
            migrated.content,
            MessageContent::Text(String::from("hello"))
        );
    }

    #[test]
    fn legacy_agent_response_v0_migrates_to_current() {
        let v0 = LegacyAgentResponseV0 {
            request_id: make_uuid(),
            content: String::from("done"),
            skill_trace: vec![String::from("called:read_file")],
            latency_ms: 10,
        };
        let json = serde_json::to_string(&v0).expect("to json");
        let migrated = AgentResponse::from_json_any_version(&json).expect("migrate");
        assert_eq!(migrated.content, MessageContent::Text(String::from("done")));
        assert_eq!(migrated.skill_trace.len(), 1);
        assert_eq!(migrated.skill_trace[0].result_summary, "called:read_file");
    }

    /// Build a sample `HardwareIntent`.
    fn sample_hw_intent() -> HardwareIntent {
        HardwareIntent {
            intent_id: 1001,
            motor_id: 7,
            target_velocity: core::f32::consts::PI,
        }
    }

    // =====================================================================
    // AgentRequest tests
    // =====================================================================

    #[test]
    fn agent_request_postcard_round_trip() {
        let req = sample_request();
        let bytes = postcard::to_allocvec(&req).expect("serialize");
        let decoded: AgentRequest = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(req, decoded);
    }

    #[test]
    fn agent_request_json_round_trip() {
        let req = sample_request();
        let json = serde_json::to_string(&req).expect("to json");
        let decoded: AgentRequest = serde_json::from_str(&json).expect("from json");
        assert_eq!(req, decoded);
    }

    #[test]
    fn agent_request_empty_content() {
        let req = AgentRequest {
            request_id: make_uuid(),
            session_id: make_uuid(),
            channel: GatewayChannel::Cli,
            user_id: String::from(""),
            content: MessageContent::Text(String::from("")),
            timestamp_us: 0,
        };
        let bytes = postcard::to_allocvec(&req).expect("serialize");
        let decoded: AgentRequest = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(req, decoded);
    }

    // =====================================================================
    // AgentResponse tests
    // =====================================================================

    #[test]
    fn agent_response_postcard_round_trip() {
        let resp = sample_response();
        let bytes = postcard::to_allocvec(&resp).expect("serialize");
        let decoded: AgentResponse = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(resp, decoded);
    }

    #[test]
    fn agent_response_empty_skill_trace() {
        let resp = AgentResponse {
            request_id: make_uuid(),
            content: MessageContent::Text(String::from("done")),
            skill_trace: vec![],
            latency_ms: 1,
        };
        let bytes = postcard::to_allocvec(&resp).expect("serialize");
        let decoded: AgentResponse = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(resp, decoded);
        assert!(decoded.skill_trace.is_empty());
    }

    #[test]
    fn message_content_text_helper() {
        let c = MessageContent::Text("hello".to_string());
        assert_eq!(c.as_text(), Some("hello"));

        let c = MessageContent::Location { lat: 1.0, lng: 2.0 };
        assert_eq!(c.as_text(), None);
    }

    // =====================================================================
    // ToolDefinition tests
    // =====================================================================

    #[test]
    fn tool_definition_postcard_round_trip() {
        let tool = sample_tool();
        let bytes = postcard::to_allocvec(&tool).expect("serialize");
        let decoded: ToolDefinition = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(tool, decoded);
    }

    #[test]
    fn tool_definition_large_embedding() {
        let tool = ToolDefinition {
            name: String::from("big_embed"),
            description: String::from("tool with large embedding"),
            parameters: String::from("{}"),
            embedding: vec![0.0_f32; 1024], // BGE-m3 can output 1024-dim
        };
        let bytes = postcard::to_allocvec(&tool).expect("serialize");
        let decoded: ToolDefinition = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(tool.embedding.len(), decoded.embedding.len());
        assert_eq!(tool, decoded);
    }

    // =====================================================================
    // HardwareIntent tests
    // =====================================================================

    #[test]
    fn hardware_intent_postcard_round_trip() {
        let hw = sample_hw_intent();
        let bytes = hw.to_postcard_bytes().expect("serialize");
        let decoded = HardwareIntent::from_postcard_bytes(&bytes).expect("deserialize");
        assert_eq!(hw, decoded);
    }

    #[test]
    fn hardware_intent_no_alloc_errors() {
        // HardwareIntent is Copy + fixed-size.  Verify the serialized form
        // is compact (no variable-length overhead beyond varint encoding).
        let hw = HardwareIntent {
            intent_id: 0,
            motor_id: 0,
            target_velocity: 0.0,
        };
        let bytes = hw.to_postcard_bytes().expect("serialize");
        // postcard uses varint → intent_id=0 is 1 byte, motor_id=0 is 1 byte,
        // f32 is always 4 bytes → minimum ~6 bytes.
        assert!(
            bytes.len() <= 16,
            "serialized size unexpectedly large: {}",
            bytes.len()
        );
        let decoded = HardwareIntent::from_postcard_bytes(&bytes).expect("deserialize");
        assert_eq!(hw, decoded);
    }

    #[test]
    fn hardware_intent_boundary_motor_id() {
        for motor in [0u8, 1, 127, 255] {
            let hw = HardwareIntent {
                intent_id: 999,
                motor_id: motor,
                target_velocity: -1.0,
            };
            let bytes = hw.to_postcard_bytes().expect("serialize");
            let decoded = HardwareIntent::from_postcard_bytes(&bytes).expect("deserialize");
            assert_eq!(hw, decoded);
        }
    }

    #[test]
    fn hardware_intent_extreme_velocity() {
        for vel in [f32::MIN, f32::MAX, f32::EPSILON, -0.0, 0.0] {
            let hw = HardwareIntent {
                intent_id: 1,
                motor_id: 1,
                target_velocity: vel,
            };
            let bytes = hw.to_postcard_bytes().expect("serialize");
            let decoded = HardwareIntent::from_postcard_bytes(&bytes).expect("deserialize");
            assert_eq!(hw.intent_id, decoded.intent_id);
            assert_eq!(hw.motor_id, decoded.motor_id);
            // f32 bitwise comparison handles -0.0 vs 0.0
            assert_eq!(
                hw.target_velocity.to_bits(),
                decoded.target_velocity.to_bits()
            );
        }
    }

    // =====================================================================
    // UUID validation tests
    // =====================================================================

    #[test]
    fn uuid_format_valid() {
        let id = make_uuid();
        assert!(AgentRequest::validate_uuid(&id).is_ok());
        // Should be 16 bytes
        assert_eq!(id.len(), 16);
    }

    #[test]
    fn uuid_all_zeros_invalid() {
        let id: Uuid = [0u8; 16];
        let result = AgentRequest::validate_uuid(&id);
        assert!(result.is_err());
        match result {
            Err(AriaError::ValidationError(msg)) => {
                assert!(msg.contains("zeros"), "unexpected message: {}", msg);
            }
            _ => panic!("expected ValidationError"),
        }
    }

    #[test]
    fn uuid_single_nonzero_valid() {
        let mut id = [0u8; 16];
        id[15] = 1;
        assert!(AgentRequest::validate_uuid(&id).is_ok());
    }

    // =====================================================================
    // Error type tests
    // =====================================================================

    #[test]
    fn error_display_serialization() {
        let err = AriaError::SerializationError("bad data".to_string());
        let msg = alloc::format!("{}", err);
        assert!(msg.contains("serialization error"));
        assert!(msg.contains("bad data"));
    }

    #[test]
    fn error_display_validation() {
        let err = AriaError::ValidationError("missing field".to_string());
        let msg = alloc::format!("{}", err);
        assert!(msg.contains("validation error"));
        assert!(msg.contains("missing field"));
    }

    #[test]
    fn hardware_intent_corrupt_bytes() {
        let result = HardwareIntent::from_postcard_bytes(&[0xFF, 0xFF]);
        assert!(result.is_err());
        match result {
            Err(AriaError::SerializationError(_)) => {} // expected
            other => panic!("expected SerializationError, got: {:?}", other),
        }
    }
}
