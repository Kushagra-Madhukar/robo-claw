use super::*;

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
            tool_runtime_policy: None,
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
            requires_strict_schema: false,
            streaming_safe: false,
            parallel_safe: true,
            modalities: vec![ToolModality::Text],
        }
    }

    fn sample_model_ref() -> ModelRef {
        ModelRef::new("openrouter", "openai/gpt-4o-mini")
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
    fn control_intent_round_trip_json() {
        let intent = ControlIntent::ResolveApproval {
            decision: ApprovalResolutionDecision::Approve,
            target: Some(String::from("approval-1")),
            tool_hint: Some(String::from("browser_download")),
        };
        let json = serde_json::to_string(&intent).expect("to json");
        let decoded: ControlIntent = serde_json::from_str(&json).expect("from json");
        assert_eq!(intent, decoded);
    }

    #[test]
    fn parse_control_intent_supports_aliases() {
        let tg = parse_control_intent("/a 42", GatewayChannel::Telegram)
            .expect("telegram alias should parse");
        assert_eq!(
            tg,
            ControlIntent::ResolveApproval {
                decision: ApprovalResolutionDecision::Approve,
                target: Some(String::from("42")),
                tool_hint: None,
            }
        );

        let cli =
            parse_control_intent(":deny 7", GatewayChannel::Cli).expect("cli alias should parse");
        assert_eq!(
            cli,
            ControlIntent::ResolveApproval {
                decision: ApprovalResolutionDecision::Deny,
                target: Some(String::from("7")),
                tool_hint: None,
            }
        );

        let tg_callback = parse_control_intent(
            "/approve 00000000-0000-0000-0000-000000000001 browser_download",
            GatewayChannel::Telegram,
        )
        .expect("telegram callback format should parse");
        assert_eq!(
            tg_callback,
            ControlIntent::ResolveApproval {
                decision: ApprovalResolutionDecision::Approve,
                target: Some(String::from("00000000-0000-0000-0000-000000000001")),
                tool_hint: Some(String::from("browser_download")),
            }
        );

        let tg_agent =
            parse_control_intent("/ag developer", GatewayChannel::Telegram).expect("tg /ag");
        assert_eq!(
            tg_agent,
            ControlIntent::SwitchAgent {
                agent_id: Some(String::from("developer"))
            }
        );

        let wa_tz =
            parse_control_intent("/tz Asia/Kolkata", GatewayChannel::WhatsApp).expect("wa /tz");
        assert_eq!(
            wa_tz,
            ControlIntent::SetTimezone {
                timezone: Some(String::from("Asia/Kolkata"))
            }
        );

        let ws = parse_control_intent(":approve apv-123", GatewayChannel::WebSocket)
            .expect("ws :approve");
        assert_eq!(
            ws,
            ControlIntent::ResolveApproval {
                decision: ApprovalResolutionDecision::Approve,
                target: Some(String::from("apv-123")),
                tool_hint: None,
            }
        );

        let mailbox = parse_control_intent("/mailbox run-1", GatewayChannel::Cli).expect("mailbox");
        assert_eq!(
            mailbox,
            ControlIntent::InspectMailbox {
                run_id: Some(String::from("run-1"))
            }
        );

        let run_tree =
            parse_control_intent("/run_tree session-1", GatewayChannel::Cli).expect("run_tree");
        assert_eq!(
            run_tree,
            ControlIntent::InspectRunTree {
                session_id: Some(String::from("session-1"))
            }
        );

        let takeover = parse_control_intent("/run_takeover run-1 developer", GatewayChannel::Cli)
            .expect("takeover");
        assert_eq!(
            takeover,
            ControlIntent::TakeoverRun {
                run_id: Some(String::from("run-1")),
                agent_id: Some(String::from("developer"))
            }
        );

        let stop = parse_control_intent("/stop", GatewayChannel::Telegram).expect("stop");
        assert_eq!(stop, ControlIntent::StopCurrent);

        let pivot =
            parse_control_intent("/pivot focus on tests", GatewayChannel::Cli).expect("pivot");
        assert_eq!(
            pivot,
            ControlIntent::Pivot {
                instructions: Some(String::from("focus on tests"))
            }
        );

        let install =
            parse_control_intent("/install_skill {\"bytes\":\"abc\"}", GatewayChannel::Cli)
                .expect("install");
        assert_eq!(
            install,
            ControlIntent::InstallSkill {
                signed_module_json: Some(String::from("{\"bytes\":\"abc\"}"))
            }
        );

        let session_clear = parse_control_intent("/session clear", GatewayChannel::Telegram)
            .expect("session clear");
        assert_eq!(session_clear, ControlIntent::ClearSession);

        let locks = parse_control_intent("/workspace_locks", GatewayChannel::Cli)
            .expect("workspace locks");
        assert_eq!(locks, ControlIntent::ListWorkspaceLocks);

        let provider_health = parse_control_intent("/provider_health", GatewayChannel::Cli)
            .expect("provider health");
        assert_eq!(provider_health, ControlIntent::ListProviderHealth);
    }

    #[test]
    fn derive_scoped_session_id_respects_policy_modes() {
        let original: Uuid = [7u8; 16];
        assert_eq!(
            derive_scoped_session_id(
                original,
                GatewayChannel::Telegram,
                "user-1",
                SessionScopePolicy::Main
            ),
            original
        );
        let peer = derive_scoped_session_id(
            original,
            GatewayChannel::Telegram,
            "user-1",
            SessionScopePolicy::Peer,
        );
        let channel_peer = derive_scoped_session_id(
            original,
            GatewayChannel::Telegram,
            "user-1",
            SessionScopePolicy::ChannelPeer,
        );
        let account_channel_peer = derive_scoped_session_id(
            original,
            GatewayChannel::Telegram,
            "user-1",
            SessionScopePolicy::AccountChannelPeer,
        );
        assert_ne!(peer, original);
        assert_ne!(channel_peer, original);
        assert_ne!(account_channel_peer, original);
        assert_ne!(peer, channel_peer);
        assert_ne!(channel_peer, account_channel_peer);
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

    fn sample_robotics_contract() -> RoboticsCommandContract {
        RoboticsCommandContract {
            intent_id: make_uuid(),
            robot_id: String::from("rover-1"),
            requested_by_agent: String::from("robotics_ctrl"),
            kind: RoboticsIntentKind::MoveActuator,
            actuator_id: Some(7),
            target_velocity: Some(0.25),
            reason: String::from("nudge left wheel for test"),
            execution_mode: RoboticsExecutionMode::Simulation,
            timestamp_us: 123,
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
            tool_runtime_policy: None,
            timestamp_us: 0,
        };
        let bytes = postcard::to_allocvec(&req).expect("serialize");
        let decoded: AgentRequest = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(req, decoded);
    }

    #[test]
    fn channel_capability_profiles_match_expected_behavior() {
        let cli = channel_capability_profile(GatewayChannel::Cli);
        assert!(!cli.supports_rich_media);
        assert!(!cli.supports_callbacks);
        assert!(cli.supports_command_aliases);

        let telegram = channel_capability_profile(GatewayChannel::Telegram);
        assert!(telegram.supports_rich_media);
        assert!(telegram.supports_callbacks);
        assert!(telegram.supports_inline_buttons);
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
            requires_strict_schema: false,
            streaming_safe: false,
            parallel_safe: true,
            modalities: vec![ToolModality::Text],
        };
        let bytes = postcard::to_allocvec(&tool).expect("serialize");
        let decoded: ToolDefinition = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(tool.embedding.len(), decoded.embedding.len());
        assert_eq!(tool, decoded);
    }

    #[test]
    fn model_ref_serializes_and_formats_as_slash_ref() {
        let model_ref = sample_model_ref();
        assert_eq!(model_ref.as_slash_ref(), "openrouter/openai/gpt-4o-mini");
        let bytes = postcard::to_allocvec(&model_ref).expect("serialize");
        let decoded: ModelRef = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(model_ref, decoded);
    }

    #[test]
    fn model_capability_profile_postcard_round_trip() {
        let profile = ModelCapabilityProfile {
            model_ref: sample_model_ref(),
            adapter_family: AdapterFamily::OpenAiCompatible,
            tool_calling: CapabilitySupport::Supported,
            parallel_tool_calling: CapabilitySupport::Degraded,
            streaming: CapabilitySupport::Supported,
            vision: CapabilitySupport::Supported,
            json_mode: CapabilitySupport::Supported,
            max_context_tokens: Some(128_000),
            tool_schema_mode: ToolSchemaMode::StrictJsonSchema,
            tool_result_mode: ToolResultMode::NativeStructured,
            supports_images: CapabilitySupport::Supported,
            supports_audio: CapabilitySupport::Unknown,
            source: CapabilitySourceKind::RuntimeProbe,
            source_detail: Some(String::from("probe")),
            observed_at_us: 42,
            expires_at_us: Some(4242),
        };
        let bytes = postcard::to_allocvec(&profile).expect("serialize");
        let decoded: ModelCapabilityProfile = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(profile, decoded);
    }

    #[test]
    fn provider_capability_profile_postcard_round_trip() {
        let profile = ProviderCapabilityProfile {
            provider_id: String::from("ollama"),
            adapter_family: AdapterFamily::OllamaNative,
            supports_model_listing: CapabilitySupport::Supported,
            supports_runtime_probe: CapabilitySupport::Degraded,
            source: CapabilitySourceKind::ProviderCatalog,
            observed_at_us: 7,
        };
        let bytes = postcard::to_allocvec(&profile).expect("serialize");
        let decoded: ProviderCapabilityProfile = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(profile, decoded);
    }

    #[test]
    fn model_capability_probe_record_postcard_round_trip() {
        let probe = ModelCapabilityProbeRecord {
            probe_id: String::from("probe-1"),
            model_ref: sample_model_ref(),
            adapter_family: AdapterFamily::OpenAiCompatible,
            tool_calling: CapabilitySupport::Supported,
            parallel_tool_calling: CapabilitySupport::Supported,
            streaming: CapabilitySupport::Supported,
            vision: CapabilitySupport::Supported,
            json_mode: CapabilitySupport::Supported,
            max_context_tokens: Some(128000),
            supports_images: CapabilitySupport::Supported,
            supports_audio: CapabilitySupport::Unknown,
            schema_acceptance: Some(CapabilitySupport::Supported),
            native_tool_probe: Some(CapabilitySupport::Supported),
            modality_probe: Some(CapabilitySupport::Supported),
            source: CapabilitySourceKind::RuntimeProbe,
            probe_method: Some(String::from("catalog")),
            probe_status: Some(String::from("success")),
            probe_error: None,
            raw_summary: Some(String::from("tool call probe passed")),
            observed_at_us: 10,
            expires_at_us: Some(20),
        };
        let bytes = postcard::to_allocvec(&probe).expect("serialize");
        let decoded: ModelCapabilityProbeRecord =
            postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(probe, decoded);
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

    #[test]
    fn robotics_command_contract_round_trip() {
        let contract = sample_robotics_contract();
        let bytes = postcard::to_allocvec(&contract).expect("serialize");
        let decoded: RoboticsCommandContract = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(contract, decoded);
    }

    #[test]
    fn robotics_command_contract_validate_requires_move_fields() {
        let mut contract = sample_robotics_contract();
        contract.actuator_id = None;
        let err = contract.validate().expect_err("expected validation error");
        assert!(alloc::format!("{}", err).contains("actuator_id"));

        let mut contract = sample_robotics_contract();
        contract.target_velocity = None;
        let err = contract.validate().expect_err("expected validation error");
        assert!(alloc::format!("{}", err).contains("target_velocity"));
    }

    #[test]
    fn robotics_command_contract_validate_rejects_motion_fields_for_capture() {
        let mut contract = sample_robotics_contract();
        contract.kind = RoboticsIntentKind::CaptureImage;
        let err = contract.validate().expect_err("expected validation error");
        assert!(alloc::format!("{}", err).contains("must not include"));
    }

    #[test]
    fn robotics_safety_event_round_trip() {
        let event = RoboticsSafetyEvent::DegradedLocalModeEntered {
            robot_id: String::from("rover-1"),
            reason: String::from("heartbeat timeout"),
            timestamp_us: 55,
        };
        let bytes = postcard::to_allocvec(&event).expect("serialize");
        let decoded: RoboticsSafetyEvent = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(event, decoded);
    }

    #[test]
    fn ros2_bridge_profile_round_trip_json() {
        let profile = Ros2BridgeProfile {
            profile_id: String::from("ros2-lab"),
            display_name: String::from("Lab ROS2"),
            namespace: String::from("/robots/lab"),
            command_topic: String::from("cmd"),
            telemetry_topic: String::from("telemetry"),
            image_topic: Some(String::from("camera")),
            service_prefix: Some(String::from("svc")),
            requires_approval: true,
            simulation_only: false,
        };
        let json = serde_json::to_string(&profile).expect("to json");
        let decoded: Ros2BridgeProfile = serde_json::from_str(&json).expect("from json");
        assert_eq!(decoded, profile);
    }

    #[test]
    fn agent_capability_profile_round_trip_with_expanded_scopes() {
        let profile = AgentCapabilityProfile {
            agent_id: String::from("omni"),
            class: AgentClass::Generalist,
            tool_allowlist: vec![String::from("read_file")],
            skill_allowlist: vec![String::from("github_review")],
            mcp_server_allowlist: vec![String::from("github")],
            mcp_tool_allowlist: vec![String::from("create_issue")],
            mcp_prompt_allowlist: vec![String::from("review_pr")],
            mcp_resource_allowlist: vec![String::from("repo://issues")],
            filesystem_scopes: vec![FilesystemScope {
                root_path: String::from("/workspace"),
                allow_read: true,
                allow_write: false,
                allow_execute: false,
            }],
            retrieval_scopes: vec![RetrievalScope::Workspace, RetrievalScope::ControlDocument],
            delegation_scope: Some(DelegationScope {
                can_spawn_children: true,
                allowed_agents: vec![String::from("researcher")],
                max_fanout: 3,
                max_runtime_seconds: 600,
            }),
            web_domain_allowlist: vec![String::from("github.com")],
            web_domain_blocklist: vec![String::from("evil.example")],
            browser_profile_allowlist: vec![String::from("work-profile")],
            browser_action_scope: Some(BrowserActionScope::InteractiveNonAuth),
            computer_profile_allowlist: vec![String::from("local-mac")],
            computer_action_scope: Some(ComputerActionScope::PointerAndKeyboard),
            browser_session_scope: Some(BrowserSessionScope::ManagedProfileOnly),
            crawl_scope: Some(CrawlScope::AllowlistedDomains),
            web_approval_policy: Some(WebApprovalPolicy::PromptOnUnknownDomain),
            web_transport_allowlist: vec![BrowserTransportKind::ManagedBrowser],
            requires_elevation: true,
            side_effect_level: SideEffectLevel::Privileged,
            trust_profile: Some(TrustProfile::TrustedWorkspace),
        };

        let json = serde_json::to_string(&profile).expect("serialize");
        let decoded: AgentCapabilityProfile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, profile);
    }

    #[test]
    fn browser_profile_and_domain_access_types_round_trip_json() {
        let profile = BrowserProfile {
            profile_id: String::from("work-profile"),
            display_name: String::from("Work"),
            mode: BrowserProfileMode::ManagedPersistent,
            engine: BrowserEngine::Chromium,
            is_default: false,
            persistent: true,
            managed_by_aria: true,
            attached_source: None,
            extension_binding_id: None,
            allowed_domains: vec![String::from("github.com"), String::from("docs.rs")],
            auth_enabled: true,
            write_enabled: false,
            created_at_us: 10,
        };
        let decision = DomainAccessDecision {
            decision_id: String::from("decision-1"),
            domain: String::from("github.com"),
            agent_id: Some(String::from("developer")),
            session_id: None,
            action_family: WebActionFamily::InteractiveRead,
            decision: DomainDecisionKind::AllowAlways,
            scope: DomainDecisionScope::Domain,
            created_by_user_id: String::from("u1"),
            created_at_us: 11,
            expires_at_us: None,
            reason: Some(String::from("approved for research")),
        };

        let profile_json = serde_json::to_string(&profile).expect("serialize profile");
        let decoded_profile: BrowserProfile =
            serde_json::from_str(&profile_json).expect("deserialize profile");
        assert_eq!(decoded_profile, profile);

        let decision_json = serde_json::to_string(&decision).expect("serialize decision");
        let decoded_decision: DomainAccessDecision =
            serde_json::from_str(&decision_json).expect("deserialize decision");
        assert_eq!(decoded_decision, decision);
    }

    #[test]
    fn computer_runtime_types_round_trip_json() {
        let profile = ComputerExecutionProfile {
            profile_id: String::from("desktop-safe"),
            display_name: String::from("Desktop Safe"),
            runtime_kind: ComputerRuntimeKind::ManagedVm,
            isolated: true,
            headless: false,
            allow_clipboard: false,
            allow_keyboard: true,
            allow_pointer: true,
            allowed_windows: vec![String::from("Calculator"), String::from("Notes")],
            created_at_us: 21,
        };
        let session = ComputerSessionRecord {
            computer_session_id: String::from("computer-session-1"),
            session_id: make_uuid(),
            agent_id: String::from("developer"),
            profile_id: profile.profile_id.clone(),
            runtime_kind: ComputerRuntimeKind::ManagedVm,
            selected_window_id: Some(String::from("Calculator")),
            created_at_us: 22,
            updated_at_us: 23,
        };
        let action = ComputerActionRequest {
            computer_session_id: Some(session.computer_session_id.clone()),
            profile_id: Some(profile.profile_id.clone()),
            target_window_id: Some(String::from("Calculator")),
            action: ComputerActionKind::PointerClick,
            x: Some(100),
            y: Some(200),
            button: Some(ComputerPointerButton::Left),
            text: None,
            key: None,
        };
        let artifact = ComputerArtifactRecord {
            artifact_id: String::from("computer-artifact-1"),
            session_id: make_uuid(),
            agent_id: String::from("developer"),
            computer_session_id: Some(session.computer_session_id.clone()),
            profile_id: Some(profile.profile_id.clone()),
            kind: ComputerArtifactKind::Screenshot,
            mime_type: String::from("image/png"),
            storage_path: String::from("/tmp/computer-shot.png"),
            metadata: serde_json::json!({"window":"Calculator"}),
            created_at_us: 24,
        };

        let payload = serde_json::json!({
            "profile": profile,
            "session": session,
            "action": action,
            "artifact": artifact,
        });
        let json = serde_json::to_string(&payload).expect("serialize computer payload");
        let decoded: serde_json::Value = serde_json::from_str(&json).expect("deserialize computer payload");
        assert_eq!(decoded["profile"]["runtime_kind"], "managed_vm");
        assert_eq!(decoded["action"]["action"], "pointer_click");
        assert_eq!(decoded["artifact"]["kind"], "screenshot");
    }

    #[test]
    fn crawl_and_website_memory_types_round_trip_json() {
        let crawl_job = CrawlJob {
            crawl_id: String::from("crawl-1"),
            seed_url: String::from("https://docs.rs"),
            scope: CrawlScope::SameOrigin,
            allowed_domains: vec![String::from("docs.rs")],
            max_depth: 2,
            max_pages: 20,
            render_js: false,
            capture_screenshots: true,
            change_detection: true,
            initiated_by_agent: String::from("researcher"),
            status: CrawlJobStatus::Queued,
            created_at_us: 12,
            updated_at_us: 12,
        };
        let memory = WebsiteMemoryRecord {
            record_id: String::from("site-1"),
            domain: String::from("github.com"),
            canonical_home_url: String::from("https://github.com"),
            known_paths: vec![String::from("/"), String::from("/pulls")],
            known_selectors: vec![String::from("input[placeholder='Search GitHub']")],
            known_login_entrypoints: vec![String::from("/login")],
            known_search_patterns: vec![String::from("/search?q={query}")],
            last_successful_actions: vec![String::from("open_repo"), String::from("read_pr")],
            page_hashes: BTreeMap::from([
                (String::from("/"), String::from("hash-home")),
                (String::from("/pulls"), String::from("hash-pulls")),
            ]),
            render_required: true,
            challenge_frequency: BrowserChallengeFrequency::Occasional,
            last_seen_at_us: 13,
            updated_at_us: 13,
        };

        let crawl_json = serde_json::to_string(&crawl_job).expect("serialize crawl");
        let decoded_crawl: CrawlJob = serde_json::from_str(&crawl_json).expect("deserialize crawl");
        assert_eq!(decoded_crawl, crawl_job);

        let memory_json = serde_json::to_string(&memory).expect("serialize memory");
        let decoded_memory: WebsiteMemoryRecord =
            serde_json::from_str(&memory_json).expect("deserialize memory");
        assert_eq!(decoded_memory, memory);

        let login_state = BrowserLoginStateRecord {
            login_state_id: String::from("login-1"),
            browser_session_id: String::from("browser-session-1"),
            session_id: make_uuid(),
            agent_id: String::from("researcher"),
            profile_id: String::from("work-profile"),
            domain: String::from("github.com"),
            state: BrowserLoginStateKind::Authenticated,
            credential_key_names: vec![
                String::from("github_username"),
                String::from("github_password"),
            ],
            notes: Some(String::from("manual login completed")),
            last_validated_at_us: Some(14),
            created_at_us: 14,
            updated_at_us: 15,
        };
        let login_json = serde_json::to_string(&login_state).expect("serialize login state");
        let decoded_login: BrowserLoginStateRecord =
            serde_json::from_str(&login_json).expect("deserialize login state");
        assert_eq!(decoded_login, login_state);

        let session_state = BrowserSessionStateRecord {
            state_id: String::from("state-1"),
            browser_session_id: String::from("browser-session-1"),
            session_id: make_uuid(),
            agent_id: String::from("researcher"),
            profile_id: String::from("work-profile"),
            storage_path: String::from("/tmp/browser-state.enc"),
            content_sha256_hex: String::from("abc123"),
            last_restored_at_us: Some(16),
            created_at_us: 15,
            updated_at_us: 16,
        };
        let state_json =
            serde_json::to_string(&session_state).expect("serialize browser session state");
        let decoded_state: BrowserSessionStateRecord =
            serde_json::from_str(&state_json).expect("deserialize browser session state");
        assert_eq!(decoded_state, session_state);
    }

    #[test]
    fn agent_run_record_round_trip_json() {
        let record = AgentRunRecord {
            run_id: String::from("run-1"),
            parent_run_id: Some(String::from("run-parent")),
                    origin_kind: None,
                    lineage_run_id: None,
            session_id: make_uuid(),
            user_id: String::from("u1"),
            requested_by_agent: Some(String::from("omni")),
            agent_id: String::from("researcher"),
            status: AgentRunStatus::Queued,
            request_text: String::from("review the PRs"),
            inbox_on_completion: true,
            max_runtime_seconds: Some(300),
            created_at_us: 10,
            started_at_us: None,
            finished_at_us: None,
            result: None,
        };

        let json = serde_json::to_string(&record).expect("serialize");
        let decoded: AgentRunRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, record);
    }

    #[test]
    fn skill_mcp_control_and_compaction_types_round_trip_json() {
        let snapshot = ControlDocumentSnapshot {
            snapshot_id: String::from("snap-1"),
            workspace_root: String::from("/workspace"),
            entries: vec![ControlDocumentEntry {
                document_id: String::from("doc-1"),
                workspace_root: String::from("/workspace"),
                relative_path: String::from("instructions.md"),
                kind: ControlDocumentKind::Instructions,
                sha256_hex: String::from("abc123"),
                body: String::from("Always write tests first."),
                updated_at_us: 50,
            }],
            created_at_us: 60,
        };
        let skill = SkillPackageManifest {
            skill_id: String::from("github_review"),
            name: String::from("GitHub Review"),
            description: String::from("Review GitHub pull requests"),
            version: String::from("1.0.0"),
            entry_document: String::from("SKILL.md"),
            tool_names: vec![String::from("read_file")],
            mcp_server_dependencies: vec![String::from("github")],
            retrieval_hints: vec![String::from("repo_context")],
            wasm_module_ref: None,
            config_schema: Some(String::from("{}")),
            enabled: true,
            provenance: None,
        };
        let server = McpServerProfile {
            server_id: String::from("github"),
            display_name: String::from("GitHub MCP"),
            transport: String::from("stdio"),
            endpoint: String::from("npx @modelcontextprotocol/server-github"),
            auth_ref: Some(String::from("vault:github_token")),
            enabled: true,
        };
        let mcp_binding = McpBindingRecord {
            binding_id: String::from("mcp-bind-1"),
            agent_id: String::from("developer"),
            server_id: String::from("github"),
            primitive_kind: McpPrimitiveKind::Tool,
            target_name: String::from("create_issue"),
            created_at_us: 70,
        };
        let mcp_cache = McpImportCacheRecord {
            server_id: String::from("github"),
            transport: String::from("stdio"),
            tool_count: 1,
            prompt_count: 1,
            resource_count: 1,
            refreshed_at_us: 80,
        };
        let compaction = CompactionState {
            session_id: make_uuid(),
            status: CompactionStatus::Running,
            last_started_at_us: Some(100),
            last_completed_at_us: None,
            metadata: CompactionMetadata {
                summary_hash: Some(String::from("hash-1")),
                summary_version: 2,
                last_error: None,
            },
        };

        let payload = serde_json::json!({
            "snapshot": snapshot,
            "skill": skill,
            "server": server,
            "mcp_binding": mcp_binding,
            "mcp_cache": mcp_cache,
            "compaction": compaction,
        });
        let encoded = serde_json::to_string(&payload).expect("serialize");
        let decoded: serde_json::Value = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded["skill"]["skill_id"], "github_review");
        assert_eq!(decoded["server"]["server_id"], "github");
        assert_eq!(decoded["mcp_binding"]["target_name"], "create_issue");
        assert_eq!(decoded["mcp_cache"]["tool_count"], 1);
        assert_eq!(decoded["compaction"]["status"], "running");
    }

    #[test]
    fn scope_denial_kind_exposes_stable_codes() {
        assert_eq!(
            ScopeDenialKind::ExecutionProfile.code(),
            "execution_profile"
        );
        assert_eq!(ScopeDenialKind::NetworkEgress.code(), "network_egress");
        assert_eq!(ScopeDenialKind::SecretEgress.code(), "secret_egress");
    }

    #[test]
    fn secret_usage_audit_record_round_trip_json() {
        let record = SecretUsageAuditRecord {
            audit_id: "secret-audit-1".into(),
            agent_id: "developer".into(),
            session_id: Some(make_uuid()),
            tool_name: "browser_login_fill_credentials".into(),
            key_name: "github_token".into(),
            target_domain: "github.com".into(),
            outcome: SecretUsageOutcome::Allowed,
            detail: "retrieved for login".into(),
            created_at_us: 10,
        };
        let json = serde_json::to_string(&record).expect("serialize");
        let decoded: SecretUsageAuditRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, record);
    }

    #[test]
    fn retrieval_trace_record_round_trip_json() {
        let record = RetrievalTraceRecord {
            trace_id: "retrieval-1".into(),
            request_id: make_uuid(),
            session_id: make_uuid(),
            agent_id: "developer".into(),
            query_text: "find rust guidance".into(),
            latency_ms: 12,
            session_hits: 1,
            workspace_hits: 2,
            policy_hits: 1,
            external_hits: 0,
            social_hits: 0,
            document_context_hits: 1,
            history_tokens: 120,
            rag_tokens: 240,
            control_tokens: 60,
            tool_count: 5,
            control_document_conflicts: 1,
            created_at_us: 55,
        };
        let json = serde_json::to_string(&record).expect("serialize");
        let decoded: RetrievalTraceRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, record);
    }

    #[test]
    fn builtin_channel_plugin_manifests_validate() {
        for channel in [
            GatewayChannel::Telegram,
            GatewayChannel::WhatsApp,
            GatewayChannel::Cli,
            GatewayChannel::WebSocket,
        ] {
            let manifest = builtin_channel_plugin_manifest(channel);
            validate_channel_plugin_manifest(&manifest).expect("builtin manifest should validate");
        }
    }

    #[test]
    fn invalid_channel_plugin_manifest_is_rejected() {
        let mut manifest = builtin_channel_plugin_manifest(GatewayChannel::Telegram);
        manifest.control_capabilities.supports_callbacks = false;
        assert!(validate_channel_plugin_manifest(&manifest).is_err());
    }

    #[test]
    fn canonical_tool_spec_round_trips_json() {
        let spec = CanonicalToolSpec {
            tool_id: "tool.search_web".into(),
            name: "search_web".into(),
            description_short: "Search the web".into(),
            description_long: "Search the public web and return ranked results.".into(),
            schema: CanonicalToolSchema {
                parameters_json_schema:
                    r#"{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}"#
                        .into(),
                result_json_schema: Some(
                    r#"{"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"]}"#
                        .into(),
                ),
            },
            execution_kind: ToolExecutionKind::Native,
            requires_approval: ToolApprovalClass::LowRisk,
            side_effect_level: ToolSideEffectLevel::ReadOnly,
            streaming_safe: false,
            parallel_safe: true,
            modalities: vec![ToolModality::Text],
            provider_hints: ProviderCompatibilityHints {
                provider_names: vec!["openai".into(), "gemini".into()],
                requires_strict_schema: false,
                prefers_reduced_schema: true,
                supports_parallel_calls: true,
            },
        };
        let json = serde_json::to_string(&spec).expect("serialize");
        let decoded: CanonicalToolSpec = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, spec);
    }

    #[test]
    fn tool_result_envelope_builds_provider_payload() {
        let envelope = ToolResultEnvelope::success(
            "Search completed",
            "search_web",
            serde_json::json!({
                "results": [{"title":"Example","url":"https://example.com"}]
            }),
        );
        let payload = envelope.as_provider_payload();
        assert_eq!(payload["ok"], serde_json::json!(true));
        assert_eq!(payload["summary"], serde_json::json!("Search completed"));
        assert_eq!(payload["kind"], serde_json::json!("search_web"));
        assert!(payload["data"]["results"].is_array());
    }

    #[test]
    fn tool_catalog_entry_round_trips_json() {
        let entry = ToolCatalogEntry {
            tool_id: "tool.github.create_issue".into(),
            public_name: "create_issue".into(),
            description: "Create a GitHub issue".into(),
            parameters_json_schema:
                r#"{"type":"object","properties":{"title":{"type":"string"}},"required":["title"]}"#
                    .into(),
            execution_kind: ToolExecutionKind::McpImported,
            provider_kind: ToolProviderKind::Mcp,
            runner_class: ToolRunnerClass::Mcp,
            origin: ToolOrigin {
                provider_kind: ToolProviderKind::Mcp,
                provider_id: "github".into(),
                origin_id: Some("import:create_issue".into()),
                display_name: Some("GitHub MCP".into()),
            },
            artifact_kind: Some("mcp".into()),
            requires_approval: ToolApprovalClass::LowRisk,
            side_effect_level: ToolSideEffectLevel::StateChanging,
            streaming_safe: false,
            parallel_safe: true,
            modalities: vec![ToolModality::Text],
            capability_requirements: vec!["mcp_tool_allowlist:create_issue".into()],
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let decoded: ToolCatalogEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, entry);
    }

    #[test]
    fn execution_context_pack_round_trips_contract_and_retrieval_bundle() {
        let pack = ExecutionContextPack {
            system_prompt: "system".into(),
            history_messages: vec![PromptContextMessage {
                role: "user".into(),
                content: "hello".into(),
                timestamp_us: 1,
            }],
            context_blocks: vec![ContextBlock {
                kind: ContextBlockKind::ContractRequirements,
                label: "contract".into(),
                content: "must create schedule artifact".into(),
                token_estimate: 4,
            }],
            user_request: "remind me".into(),
            channel: GatewayChannel::Telegram,
            execution_contract: Some(ExecutionContract {
                kind: ExecutionContractKind::ScheduleCreate,
                allowed_tool_classes: vec!["schedule".into()],
                required_artifact_kinds: vec![ExecutionArtifactKind::Schedule],
                forbidden_completion_modes: vec!["plain_text_only".into()],
                fallback_mode: Some("compat_tools".into()),
                approval_required: false,
            }),
            retrieved_context: Some(RetrievedContextBundle {
                plan_summary: Some("session+control".into()),
                blocks: vec![RetrievedContextBlock {
                    source_kind: RetrievalSourceKind::SessionHistory,
                    source_id: "session:1".into(),
                    label: "recent_history".into(),
                    content: "user: remind me later".into(),
                    trust_class: Some("trusted".into()),
                    score: Some(0.91),
                    rank: Some(1),
                    dedupe_key: Some("session:recent".into()),
                    recency_us: Some(1),
                    token_estimate: 5,
                }],
                dropped_blocks: vec![],
            }),
            working_set: None,
            context_plan: None,
        };
        let json = serde_json::to_string(&pack).expect("serialize");
        let decoded: ExecutionContextPack = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, pack);
    }
}
