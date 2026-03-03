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

use alloc::string::String;
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
    /// Identifier of the requesting user.
    pub user_id: String,
    /// Natural-language content of the request.
    pub content: String,
    /// Timestamp in microseconds since UNIX epoch.
    pub timestamp_us: u64,
}

/// Outbound agent response sent back through the gateway.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentResponse {
    /// The request this response corresponds to.
    pub request_id: Uuid,
    /// Generated response content.
    pub content: String,
    /// Ordered list of skills that contributed to this response.
    pub skill_trace: Vec<String>,
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
            user_id: String::from("user_42"),
            content: String::from("Summarize my notes"),
            timestamp_us: 1_700_000_000_000_000,
        }
    }

    /// Build a sample `AgentResponse`.
    fn sample_response() -> AgentResponse {
        AgentResponse {
            request_id: make_uuid(),
            content: String::from("Here is your summary."),
            skill_trace: vec![String::from("read_file"), String::from("summarize")],
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

    /// Build a sample `HardwareIntent`.
    fn sample_hw_intent() -> HardwareIntent {
        HardwareIntent {
            intent_id: 1001,
            motor_id: 7,
            target_velocity: 3.14,
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
            user_id: String::from(""),
            content: String::from(""),
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
            content: String::from("done"),
            skill_trace: vec![],
        };
        let bytes = postcard::to_allocvec(&resp).expect("serialize");
        let decoded: AgentResponse = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(resp, decoded);
        assert!(decoded.skill_trace.is_empty());
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
