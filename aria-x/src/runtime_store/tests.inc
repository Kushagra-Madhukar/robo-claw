#[cfg(test)]
mod tests {
    use super::*;
    use aria_core::{
        AgentMailboxMessage, AgentRunEvent, AgentRunEventKind, AgentRunRecord, AgentRunStatus,
        AdapterFamily, CapabilitySourceKind, CapabilitySupport, CompactionMetadata,
        CompactionState, CompactionStatus, ControlDocumentEntry, ControlDocumentKind,
        GatewayChannel, McpImportedTool, McpServerProfile, MessageContent,
        ModelCapabilityProbeRecord, ModelCapabilityProfile, ModelRef, ProviderCapabilityProfile,
        SkillActivationPolicy, SkillActivationRecord, SkillBinding, SkillPackageManifest,
        ToolResultMode, ToolSchemaMode,
    };
    use aria_intelligence::{
        CachedTool, ScheduleSpec, ScheduledJobKind, ScheduledJobStatus, ScheduledPromptJob,
    };
    use aria_learning::{
        CandidateArtifactKind, CandidateArtifactRecord, CandidateArtifactStatus,
        CandidatePromotionAction, TaskFingerprint,
    };

    fn temp_store() -> (tempfile::TempDir, RuntimeStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = RuntimeStore::for_sessions_dir(dir.path());
        (dir, store)
    }

    fn sample_tool() -> CachedTool {
        CachedTool {
            name: "read_file".into(),
            description: "Read file".into(),
            parameters_schema: "{}".into(),
            embedding: vec![0.1, 0.2],
            requires_strict_schema: false,
            streaming_safe: false,
            parallel_safe: true,
            modalities: vec![aria_core::ToolModality::Text],
        }
    }

    #[test]
    fn runtime_store_reuses_shared_connection_per_database_path() {
        let (_dir, store) = temp_store();
        store.list_browser_profiles().expect("first query");
        store.list_browser_profiles().expect("second query");
        store.list_watch_jobs().expect("third query");
        assert_eq!(
            RuntimeStore::cached_connection_total_for_path(&store.path),
            Some(1)
        );
    }

    #[test]
    fn runtime_store_common_operations_smoke() {
        let (_dir, store) = temp_store();
        let started = std::time::Instant::now();
        let session_id = *uuid::Uuid::new_v4().as_bytes();
        for idx in 0..200u64 {
            let approval = ApprovalRecord {
                approval_id: format!("approval-{idx}"),
                session_id,
                user_id: "u1".into(),
                channel: GatewayChannel::Cli,
                agent_id: "researcher".into(),
                tool_name: "browser_open".into(),
                arguments_json: "{}".into(),
                pending_prompt: "approve?".into(),
                original_request: "smoke".into(),
                status: aria_core::ApprovalStatus::Pending,
                created_at_us: idx,
                resolved_at_us: None,
            };
            store.upsert_approval(&approval).expect("upsert approval");
        }
        let approvals = store
            .list_approvals(
                Some(session_id),
                Some("u1"),
                Some(aria_core::ApprovalStatus::Pending),
            )
            .expect("list approvals");
        assert_eq!(approvals.len(), 200);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "runtime store smoke test exceeded budget: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn runtime_store_round_trips_streaming_decision_audits() {
        let (_dir, store) = temp_store();
        let audit = StreamingDecisionAuditRecord {
            audit_id: "stream-1".into(),
            request_id: "req-1".into(),
            session_id: "sess-1".into(),
            user_id: "u1".into(),
            agent_id: "developer".into(),
            phase: "initial".into(),
            mode: "stream_used".into(),
            model_ref: Some("openai/gpt-4o-mini".into()),
            created_at_us: 1,
        };
        store
            .append_streaming_decision_audit(&audit)
            .expect("append");
        assert_eq!(
            store
                .list_streaming_decision_audits(Some("sess-1"), Some("developer"))
                .expect("list"),
            vec![audit]
        );
    }

    #[test]
    fn runtime_store_round_trips_browser_profiles() {
        let (_dir, store) = temp_store();
        let profile = aria_core::BrowserProfile {
            profile_id: "work-profile".into(),
            display_name: "Work".into(),
            mode: aria_core::BrowserProfileMode::ManagedPersistent,
            engine: aria_core::BrowserEngine::Chromium,
            persistent: true,
            managed_by_aria: true,
            attached_source: None,
            extension_binding_id: None,
            allowed_domains: vec!["github.com".into(), "docs.rs".into()],
            auth_enabled: true,
            write_enabled: false,
            created_at_us: 10,
        };
        store
            .upsert_browser_profile(&profile, 10)
            .expect("upsert browser profile");
        assert_eq!(store.list_browser_profiles().expect("list"), vec![profile]);
    }

    #[test]
    fn runtime_store_round_trips_browser_profile_bindings() {
        let (_dir, store) = temp_store();
        let session_id = *uuid::Uuid::new_v4().as_bytes();
        let binding = aria_core::BrowserProfileBindingRecord {
            binding_id: "binding-1".into(),
            session_id,
            agent_id: "researcher".into(),
            profile_id: "work-profile".into(),
            created_at_us: 12,
            updated_at_us: 13,
        };
        store
            .upsert_browser_profile_binding(&binding, 13)
            .expect("upsert browser profile binding");
        assert_eq!(
            store
                .list_browser_profile_bindings(Some(session_id), Some("researcher"))
                .expect("list bindings"),
            vec![binding]
        );
    }

    #[test]
    fn runtime_store_round_trips_browser_sessions() {
        let (_dir, store) = temp_store();
        let session_id = *uuid::Uuid::new_v4().as_bytes();
        let browser_session = aria_core::BrowserSessionRecord {
            browser_session_id: "browser-session-1".into(),
            session_id,
            agent_id: "researcher".into(),
            profile_id: "work-profile".into(),
            engine: aria_core::BrowserEngine::Chromium,
                    transport: aria_core::BrowserTransportKind::ManagedBrowser,
            status: aria_core::BrowserSessionStatus::Launched,
            pid: Some(1234),
            profile_dir: "/tmp/work-profile".into(),
            start_url: Some("https://github.com".into()),
            launch_command: vec!["/usr/bin/open".into(), "--args".into()],
            error: None,
            created_at_us: 20,
            updated_at_us: 21,
        };
        store
            .upsert_browser_session(&browser_session, 21)
            .expect("upsert browser session");
        assert_eq!(
            store
                .list_browser_sessions(Some(session_id), Some("researcher"))
                .expect("list browser sessions"),
            vec![browser_session]
        );
    }

    #[test]
    fn runtime_store_round_trips_browser_session_states() {
        let (_dir, store) = temp_store();
        let session_id = *uuid::Uuid::new_v4().as_bytes();
        let state = aria_core::BrowserSessionStateRecord {
            state_id: "state-1".into(),
            browser_session_id: "browser-session-1".into(),
            session_id,
            agent_id: "researcher".into(),
            profile_id: "work-profile".into(),
            storage_path: "/tmp/browser-state.enc".into(),
            content_sha256_hex: "abc123".into(),
            last_restored_at_us: Some(41),
            created_at_us: 40,
            updated_at_us: 41,
        };
        store
            .upsert_browser_session_state(&state, 41)
            .expect("upsert browser session state");
        assert_eq!(
            store
                .list_browser_session_states(Some(session_id), Some("browser-session-1"))
                .expect("list browser session states"),
            vec![state]
        );
    }

    #[test]
    fn runtime_store_round_trips_browser_login_states() {
        let (_dir, store) = temp_store();
        let session_id = *uuid::Uuid::new_v4().as_bytes();
        let login_state = aria_core::BrowserLoginStateRecord {
            login_state_id: "login-state-1".into(),
            browser_session_id: "browser-session-1".into(),
            session_id,
            agent_id: "researcher".into(),
            profile_id: "work-profile".into(),
            domain: "github.com".into(),
            state: aria_core::BrowserLoginStateKind::Authenticated,
            credential_key_names: vec!["github_user".into(), "github_password".into()],
            notes: Some("manual login completed".into()),
            last_validated_at_us: Some(42),
            created_at_us: 40,
            updated_at_us: 41,
        };
        store
            .upsert_browser_login_state(&login_state, 41)
            .expect("upsert browser login state");
        assert_eq!(
            store
                .list_browser_login_states(Some(session_id), Some("researcher"), Some("github.com"))
                .expect("list browser login states"),
            vec![login_state]
        );
    }

    #[test]
    fn runtime_store_round_trips_browser_artifacts_audits_and_challenges() {
        let (_dir, store) = temp_store();
        let session_id = *uuid::Uuid::new_v4().as_bytes();
        let artifact = aria_core::BrowserArtifactRecord {
            artifact_id: "artifact-1".into(),
            browser_session_id: "browser-session-1".into(),
            session_id,
            agent_id: "researcher".into(),
            profile_id: "work-profile".into(),
            kind: aria_core::BrowserArtifactKind::LaunchMetadata,
            mime_type: "application/json".into(),
            storage_path: "/tmp/browser_profiles/work-profile/launch.json".into(),
            metadata: serde_json::json!({"url":"https://github.com"}),
            created_at_us: 30,
        };
        let audit = aria_core::BrowserActionAuditRecord {
            audit_id: "audit-1".into(),
            browser_session_id: Some("browser-session-1".into()),
            session_id,
            agent_id: "researcher".into(),
            profile_id: Some("work-profile".into()),
            action: aria_core::BrowserActionKind::SessionStart,
            target: Some("https://github.com".into()),
            metadata: serde_json::json!({"pid":123}),
            created_at_us: 31,
        };
        let challenge = aria_core::BrowserChallengeEvent {
            event_id: "challenge-1".into(),
            browser_session_id: "browser-session-1".into(),
            session_id,
            agent_id: "researcher".into(),
            profile_id: "work-profile".into(),
            challenge: aria_core::BrowserChallengeKind::BotDefense,
            url: Some("https://github.com/login".into()),
            message: Some("bot check".into()),
            created_at_us: 32,
        };
        store
            .append_browser_artifact(&artifact)
            .expect("append browser artifact");
        store
            .append_browser_action_audit(&audit)
            .expect("append browser action audit");
        store
            .append_browser_challenge_event(&challenge)
            .expect("append browser challenge event");
        assert_eq!(
            store
                .list_browser_artifacts(Some(session_id), Some("browser-session-1"))
                .expect("list browser artifacts"),
            vec![artifact]
        );
        assert_eq!(
            store
                .list_browser_action_audits(Some(session_id), Some("researcher"))
                .expect("list browser action audits"),
            vec![audit]
        );
        assert_eq!(
            store
                .list_browser_challenge_events(Some(session_id), Some("researcher"))
                .expect("list browser challenge events"),
            vec![challenge]
        );
    }

    #[test]
    fn runtime_store_round_trips_watch_jobs() {
        let (_dir, store) = temp_store();
        let job = aria_core::WatchJobRecord {
            watch_id: "watch-1".into(),
            target_url: "https://docs.rs".into(),
            target_kind: aria_core::WatchTargetKind::Page,
            schedule_str: "every:300s".into(),
            agent_id: "researcher".into(),
            session_id: Some(*uuid::Uuid::new_v4().as_bytes()),
            user_id: Some("u1".into()),
            allowed_domains: vec!["docs.rs".into()],
            capture_screenshots: false,
            change_detection: true,
            status: aria_core::WatchJobStatus::Scheduled,
            last_checked_at_us: None,
            next_check_at_us: None,
            created_at_us: 40,
            updated_at_us: 41,
        };
        store.upsert_watch_job(&job, 41).expect("upsert watch job");
        assert_eq!(store.list_watch_jobs().expect("list watch jobs"), vec![job]);
    }

    #[test]
    fn runtime_store_lists_approvals_with_filters() {
        let (_dir, store) = temp_store();
        let session_id = *uuid::Uuid::new_v4().as_bytes();
        for (approval_id, user_id, status, created_at_us) in [
            ("a1", "u1", aria_core::ApprovalStatus::Pending, 1_u64),
            ("a2", "u2", aria_core::ApprovalStatus::Approved, 2_u64),
        ] {
            store
                .upsert_approval(&ApprovalRecord {
                    approval_id: approval_id.into(),
                    session_id,
                    user_id: user_id.into(),
                    channel: aria_core::GatewayChannel::Cli,
                    agent_id: "developer".into(),
                    tool_name: "run_shell".into(),
                    arguments_json: "{}".into(),
                    pending_prompt: String::new(),
                    original_request: String::new(),
                    status,
                    created_at_us,
                    resolved_at_us: None,
                })
                .expect("upsert approval");
        }
        let pending = store
            .list_approvals(Some(session_id), Some("u1"), Some(aria_core::ApprovalStatus::Pending))
            .expect("list approvals");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].approval_id, "a1");
    }

    #[test]
    fn runtime_store_round_trips_domain_access_decisions() {
        let (_dir, store) = temp_store();
        let decision = aria_core::DomainAccessDecision {
            decision_id: "decision-1".into(),
            domain: "github.com".into(),
            agent_id: Some("developer".into()),
            session_id: None,
            action_family: aria_core::WebActionFamily::InteractiveRead,
            decision: aria_core::DomainDecisionKind::AllowAlways,
            scope: aria_core::DomainDecisionScope::Domain,
            created_by_user_id: "u1".into(),
            created_at_us: 11,
            expires_at_us: None,
            reason: Some("approved".into()),
        };
        store
            .upsert_domain_access_decision(&decision, 11)
            .expect("upsert domain decision");
        assert_eq!(
            store
                .list_domain_access_decisions(Some("github.com"), Some("developer"))
                .expect("list"),
            vec![decision]
        );
    }

    #[test]
    fn runtime_store_round_trips_agent_runs_events_and_mailbox() {
        let (_dir, store) = temp_store();
        let session_id = uuid::Uuid::new_v4();
        let run = AgentRunRecord {
            run_id: "run-1".into(),
            parent_run_id: Some("parent-1".into()),
                    origin_kind: None,
                    lineage_run_id: None,
            session_id: *session_id.as_bytes(),
            user_id: "u1".into(),
            requested_by_agent: Some("omni".into()),
            agent_id: "researcher".into(),
            status: AgentRunStatus::Queued,
            request_text: "review the pull request".into(),
            inbox_on_completion: true,
            max_runtime_seconds: Some(300),
            created_at_us: 10,
            started_at_us: None,
            finished_at_us: None,
            result: None,
        };
        store.upsert_agent_run(&run, 10).expect("upsert agent run");
        assert_eq!(store.read_agent_run("run-1").expect("read run"), run);
        assert_eq!(
            store
                .list_agent_runs_for_session(session_id)
                .expect("list runs")
                .len(),
            1
        );

        let event = AgentRunEvent {
            event_id: "evt-1".into(),
            run_id: "run-1".into(),
            kind: AgentRunEventKind::Queued,
            summary: "queued".into(),
            created_at_us: 11,
        };
        store
            .append_agent_run_event(&event)
            .expect("append run event");
        assert_eq!(
            store
                .list_agent_run_events("run-1")
                .expect("list run events"),
            vec![event]
        );

        let message = AgentMailboxMessage {
            message_id: "msg-1".into(),
            run_id: "run-1".into(),
            session_id: *session_id.as_bytes(),
            from_agent_id: Some("researcher".into()),
            to_agent_id: Some("omni".into()),
            body: "background review queued".into(),
            created_at_us: 12,
            delivered: false,
        };
        store
            .append_agent_mailbox_message(&message)
            .expect("append mailbox message");
        assert_eq!(
            store
                .list_agent_mailbox_messages("run-1")
                .expect("list mailbox"),
            vec![message]
        );
    }

    #[test]
    fn runtime_store_claims_next_queued_agent_run_and_marks_it_running() {
        let (_dir, store) = temp_store();
        let first = AgentRunRecord {
            run_id: "run-1".into(),
            parent_run_id: Some("parent".into()),
                    origin_kind: None,
                    lineage_run_id: None,
            session_id: *uuid::Uuid::new_v4().as_bytes(),
            user_id: "u1".into(),
            requested_by_agent: Some("omni".into()),
            agent_id: "researcher".into(),
            status: AgentRunStatus::Queued,
            request_text: "first".into(),
            inbox_on_completion: true,
            max_runtime_seconds: Some(60),
            created_at_us: 10,
            started_at_us: None,
            finished_at_us: None,
            result: None,
        };
        let second = AgentRunRecord {
            run_id: "run-2".into(),
            parent_run_id: Some("parent".into()),
                    origin_kind: None,
                    lineage_run_id: None,
            session_id: *uuid::Uuid::new_v4().as_bytes(),
            user_id: "u1".into(),
            requested_by_agent: Some("omni".into()),
            agent_id: "developer".into(),
            status: AgentRunStatus::Queued,
            request_text: "second".into(),
            inbox_on_completion: true,
            max_runtime_seconds: Some(60),
            created_at_us: 20,
            started_at_us: None,
            finished_at_us: None,
            result: None,
        };
        store
            .upsert_agent_run(&second, 20)
            .expect("upsert second run");
        store
            .upsert_agent_run(&first, 10)
            .expect("upsert first run");

        let claimed = store
            .claim_next_queued_agent_run(25)
            .expect("claim next queued run")
            .expect("queued run");
        assert_eq!(claimed.run_id, "run-1");
        assert_eq!(claimed.status, AgentRunStatus::Running);
        assert_eq!(claimed.started_at_us, Some(25));

        let persisted = store.read_agent_run("run-1").expect("read claimed run");
        assert_eq!(persisted.status, AgentRunStatus::Running);
        assert_eq!(persisted.started_at_us, Some(25));

        let untouched = store.read_agent_run("run-2").expect("read untouched run");
        assert_eq!(untouched.status, AgentRunStatus::Queued);
        assert_eq!(untouched.started_at_us, None);
    }

    #[test]
    fn runtime_store_counts_active_agent_runs_for_parent_and_cancels_run() {
        let (_dir, store) = temp_store();
        let session_id = *uuid::Uuid::new_v4().as_bytes();
        for (run_id, status) in [
            ("run-1", AgentRunStatus::Queued),
            ("run-2", AgentRunStatus::Running),
            ("run-3", AgentRunStatus::Completed),
        ] {
            store
                .upsert_agent_run(
                    &AgentRunRecord {
                        run_id: run_id.into(),
                        parent_run_id: Some("parent-1".into()),
                    origin_kind: None,
                    lineage_run_id: None,
                        session_id,
                        user_id: "u1".into(),
                        requested_by_agent: Some("omni".into()),
                        agent_id: "researcher".into(),
                        status,
                        request_text: run_id.into(),
                        inbox_on_completion: true,
                        max_runtime_seconds: Some(60),
                        created_at_us: 1,
                        started_at_us: None,
                        finished_at_us: None,
                        result: None,
                    },
                    1,
                )
                .expect("upsert run");
        }

        assert_eq!(
            store
                .count_active_agent_runs_for_parent("parent-1")
                .expect("count active runs"),
            2
        );

        let cancelled = store
            .cancel_agent_run("run-2", "cancelled by parent", 10)
            .expect("cancel run")
            .expect("existing run");
        assert_eq!(cancelled.status, AgentRunStatus::Cancelled);

        let persisted = store.read_agent_run("run-2").expect("read cancelled run");
        assert_eq!(persisted.status, AgentRunStatus::Cancelled);
        assert_eq!(
            store
                .count_active_agent_runs_for_parent("parent-1")
                .expect("count active runs after cancel"),
            1
        );
        let events = store
            .list_agent_run_events("run-2")
            .expect("list cancelled events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AgentRunEventKind::Cancelled);
    }

    #[test]
    fn runtime_store_round_trips_skill_mcp_control_and_compaction_records() {
        let (_dir, store) = temp_store();
        let skill = SkillPackageManifest {
            skill_id: "github_review".into(),
            name: "GitHub Review".into(),
            description: "Review GitHub pull requests".into(),
            version: "1.0.0".into(),
            entry_document: "SKILL.md".into(),
            tool_names: vec!["read_file".into()],
            mcp_server_dependencies: vec!["github".into()],
            retrieval_hints: vec!["repo_context".into()],
            wasm_module_ref: None,
            config_schema: Some("{}".into()),
            enabled: true,
            provenance: None,
        };
        store
            .upsert_skill_package(&skill, 20)
            .expect("upsert skill package");
        assert_eq!(
            store.list_skill_packages().expect("list skill packages"),
            vec![skill.clone()]
        );

        let binding = SkillBinding {
            binding_id: "bind-1".into(),
            agent_id: "developer".into(),
            skill_id: "github_review".into(),
            activation_policy: SkillActivationPolicy::AutoSuggest,
            created_at_us: 21,
        };
        store
            .upsert_skill_binding(&binding)
            .expect("upsert skill binding");
        assert_eq!(
            store
                .list_skill_bindings_for_agent("developer")
                .expect("list skill bindings"),
            vec![binding]
        );

        let activation = SkillActivationRecord {
            activation_id: "act-1".into(),
            skill_id: "github_review".into(),
            agent_id: "developer".into(),
            run_id: Some("run-1".into()),
            session_id: None,
            active: true,
            activated_at_us: 22,
            deactivated_at_us: None,
        };
        store
            .append_skill_activation(&activation)
            .expect("append skill activation");
        assert_eq!(
            store
                .list_skill_activations_for_agent("developer")
                .expect("list skill activations"),
            vec![activation]
        );

        let signature = SkillSignatureRecord {
            record_id: "sig-1".into(),
            skill_id: "github_review".into(),
            version: "1.0.0".into(),
            algorithm: "ed25519-sha256".into(),
            payload_sha256_hex: "abc123".into(),
            public_key_hex: "key".into(),
            signature_hex: "sig".into(),
            source: "export".into(),
            verified: true,
            created_at_us: 23,
        };
        store
            .append_skill_signature(&signature)
            .expect("append skill signature");
        assert_eq!(
            store
                .list_skill_signatures(Some("github_review"))
                .expect("list skill signatures"),
            vec![signature]
        );

        let server = McpServerProfile {
            server_id: "github".into(),
            display_name: "GitHub".into(),
            transport: "stdio".into(),
            endpoint: "npx @modelcontextprotocol/server-github".into(),
            auth_ref: Some("vault:github".into()),
            enabled: true,
        provenance: None,
        };
        store
            .upsert_mcp_server(&server, 30)
            .expect("upsert mcp server");
        assert_eq!(
            store.list_mcp_servers().expect("list mcp servers"),
            vec![server]
        );

        let tool = McpImportedTool {
            import_id: "github-tool-1".into(),
            server_id: "github".into(),
            tool_name: "create_issue".into(),
            description: "Create issue".into(),
            parameters_schema: "{}".into(),
        };
        store
            .upsert_mcp_imported_tool(&tool, 31)
            .expect("upsert mcp tool");
        assert_eq!(
            store
                .list_mcp_imported_tools("github")
                .expect("list mcp tools"),
            vec![tool]
        );

        let mcp_binding = McpBindingRecord {
            binding_id: "mcp-bind-1".into(),
            agent_id: "developer".into(),
            server_id: "github".into(),
            primitive_kind: aria_core::McpPrimitiveKind::Tool,
            target_name: "create_issue".into(),
            created_at_us: 32,
        };
        store
            .upsert_mcp_binding(&mcp_binding)
            .expect("upsert mcp binding");
        assert_eq!(
            store
                .list_mcp_bindings_for_agent("developer")
                .expect("list mcp bindings"),
            vec![mcp_binding]
        );

        let mcp_cache = McpImportCacheRecord {
            server_id: "github".into(),
            transport: "stdio".into(),
            tool_count: 1,
            prompt_count: 0,
            resource_count: 0,
            refreshed_at_us: 33,
        };
        store
            .upsert_mcp_import_cache_record(&mcp_cache)
            .expect("upsert mcp cache");
        assert_eq!(
            store
                .read_mcp_import_cache_record("github")
                .expect("read mcp cache"),
            mcp_cache
        );

        let control = ControlDocumentEntry {
            document_id: "doc-1".into(),
            workspace_root: "/workspace".into(),
            relative_path: "instructions.md".into(),
            kind: ControlDocumentKind::Instructions,
            sha256_hex: "abc123".into(),
            body: "Always test first.".into(),
            updated_at_us: 40,
        };
        store
            .upsert_control_document(&control, 40)
            .expect("upsert control document");
        assert_eq!(
            store
                .list_control_documents("/workspace")
                .expect("list control docs"),
            vec![control]
        );

        let session_id = uuid::Uuid::new_v4();
        let compaction = CompactionState {
            session_id: *session_id.as_bytes(),
            status: CompactionStatus::Running,
            last_started_at_us: Some(50),
            last_completed_at_us: None,
            metadata: CompactionMetadata {
                summary_hash: Some("hash-1".into()),
                summary_version: 1,
                last_error: None,
            },
        };
        store
            .upsert_compaction_state(&compaction, 50)
            .expect("upsert compaction state");
        assert_eq!(
            store
                .read_compaction_state(session_id)
                .expect("read compaction state"),
            compaction
        );

        let denial = aria_core::ScopeDenialRecord {
            denial_id: "denial-1".into(),
            kind: aria_core::ScopeDenialKind::FilesystemScope,
            agent_id: "developer".into(),
            session_id: Some(session_id.into_bytes()),
            target: "/tmp/outside".into(),
            reason: "read_file not permitted for path '/tmp/outside'".into(),
            created_at_us: 51,
        };
        store
            .append_scope_denial(&denial)
            .expect("append scope denial");
        assert_eq!(
            store
                .list_scope_denials(Some("developer"), Some(&session_id.to_string()))
                .expect("list scope denials"),
            vec![denial]
        );

        let shell_audit = ShellExecutionAuditRecord {
            audit_id: "shell-1".into(),
            session_id: Some(session_id.to_string()),
            agent_id: Some("developer".into()),
            execution_backend_id: Some("local-default".into()),
            command: "echo hello".into(),
            cwd: Some("/workspace".into()),
            os_containment_requested: true,
            containment_backend: Some("bwrap".into()),
            timeout_seconds: 10,
            cpu_seconds: 3,
            memory_kb: 131072,
            exit_code: Some(0),
            timed_out: false,
            output_truncated: false,
            error: None,
            duration_ms: 12,
            created_at_us: 52,
        };
        store
            .append_shell_exec_audit(&shell_audit)
            .expect("append shell audit");
        assert_eq!(
            store
                .list_shell_exec_audits(Some(&session_id.to_string()), Some("developer"))
                .expect("list shell audits"),
            vec![shell_audit]
        );

        let request_policy_audit = RequestPolicyAuditRecord {
            audit_id: "reqpol-1".into(),
            request_id: "req-1".into(),
            session_id: session_id.to_string(),
            user_id: "u1".into(),
            agent_id: Some("developer".into()),
            channel: "Cli".into(),
            tool_runtime_policy: aria_core::ToolRuntimePolicy {
                tool_choice: aria_core::ToolChoicePolicy::Required,
                allow_parallel_tool_calls: false,
            },
            created_at_us: 33,
        };
        store
            .append_request_policy_audit(&request_policy_audit)
            .expect("append request policy audit");
        assert_eq!(
            store
                .list_request_policy_audits(Some(&session_id.to_string()), Some("developer"))
                .expect("list request policy audits"),
            vec![request_policy_audit]
        );
    }

    #[test]
    fn runtime_store_round_trips_provider_and_model_capabilities() {
        let (_dir, store) = temp_store();
        let provider = ProviderCapabilityProfile {
            provider_id: "openrouter".into(),
            adapter_family: AdapterFamily::OpenAiCompatible,
            supports_model_listing: CapabilitySupport::Supported,
            supports_runtime_probe: CapabilitySupport::Degraded,
            source: CapabilitySourceKind::ProviderCatalog,
            observed_at_us: 10,
        };
        store
            .upsert_provider_capability(&provider, 10)
            .expect("upsert provider capability");
        assert_eq!(
            store
                .read_provider_capability("openrouter")
                .expect("read provider capability"),
            provider
        );

        let model = ModelCapabilityProfile {
            model_ref: ModelRef::new("openrouter", "openai/gpt-4o-mini"),
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
            source_detail: Some("scan".into()),
            observed_at_us: 11,
            expires_at_us: Some(111),
        };
        store
            .upsert_model_capability(&model, 11)
            .expect("upsert model capability");
        assert_eq!(
            store
                .read_model_capability("openrouter/openai/gpt-4o-mini")
                .expect("read model capability"),
            model
        );
        assert_eq!(
            store
                .list_model_capabilities_for_provider("openrouter")
                .expect("list model capabilities"),
            vec![model.clone()]
        );

        let probe = ModelCapabilityProbeRecord {
            probe_id: "probe-1".into(),
            model_ref: model.model_ref.clone(),
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
            probe_method: Some("catalog_lookup".into()),
            probe_status: Some("success".into()),
            probe_error: None,
            raw_summary: Some("probe ok".into()),
            observed_at_us: 12,
            expires_at_us: Some(112),
        };
        store
            .append_model_capability_probe(&probe)
            .expect("append model capability probe");
        assert_eq!(
            store
                .list_model_capability_probes("openrouter/openai/gpt-4o-mini")
                .expect("list model capability probes"),
            vec![probe]
        );
    }

    #[test]
    fn runtime_store_prunes_operator_records_to_retention_limits() {
        let (_dir, store) = temp_store();
        for idx in 0..5 {
            store
                .append_skill_signature(&SkillSignatureRecord {
                    record_id: format!("sig-{}", idx),
                    skill_id: "skill-a".into(),
                    version: "1.0.0".into(),
                    algorithm: "ed25519-sha256".into(),
                    payload_sha256_hex: format!("hash-{}", idx),
                    public_key_hex: "pk".into(),
                    signature_hex: "sig".into(),
                    source: "test".into(),
                    verified: true,
                    created_at_us: idx,
                })
                .expect("append skill signature");
            store
                .append_shell_exec_audit(&ShellExecutionAuditRecord {
                    audit_id: format!("shell-{}", idx),
                    session_id: Some("sess-1".into()),
                    agent_id: Some("agent-1".into()),
                    execution_backend_id: Some("local-default".into()),
                    command: format!("echo {}", idx),
                    cwd: Some("/workspace".into()),
                    os_containment_requested: false,
                    containment_backend: None,
                    timeout_seconds: 10,
                    cpu_seconds: 2,
                    memory_kb: 131072,
                    exit_code: Some(0),
                    timed_out: false,
                    output_truncated: false,
                    error: None,
                    duration_ms: 1,
                    created_at_us: idx,
                })
                .expect("append shell audit");
        }

        store
            .prune_operator_records(
                2,
                3,
                DEFAULT_SCOPE_DENIAL_RETENTION_ROWS,
                DEFAULT_REQUEST_POLICY_AUDIT_RETENTION_ROWS,
                DEFAULT_REPAIR_FALLBACK_AUDIT_RETENTION_ROWS,
                DEFAULT_STREAMING_DECISION_AUDIT_RETENTION_ROWS,
                DEFAULT_BROWSER_ACTION_AUDIT_RETENTION_ROWS,
                DEFAULT_BROWSER_CHALLENGE_EVENT_RETENTION_ROWS,
            )
            .expect("prune operator records");

        let signatures = store
            .list_skill_signatures(Some("skill-a"))
            .expect("list signatures");
        assert_eq!(signatures.len(), 2);
        assert_eq!(signatures[0].record_id, "sig-3");
        assert_eq!(signatures[1].record_id, "sig-4");

        let audits = store
            .list_shell_exec_audits(Some("sess-1"), Some("agent-1"))
            .expect("list audits");
        assert_eq!(audits.len(), 3);
        assert_eq!(audits[0].audit_id, "shell-4");
        assert_eq!(audits[2].audit_id, "shell-2");
    }

    #[test]
    fn runtime_store_honors_configured_operator_retention_on_append() {
        RuntimeStore::configure_operator_retention(2, 2, 2, 2, 2, 2, 2, 2);
        let (_dir, store) = temp_store();
        let session_id = uuid::Uuid::new_v4();

        for idx in 0..3 {
            store
                .append_skill_signature(&SkillSignatureRecord {
                    record_id: format!("sig-cfg-{}", idx),
                    skill_id: "skill-b".into(),
                    version: "1.0.0".into(),
                    algorithm: "ed25519-sha256".into(),
                    payload_sha256_hex: format!("hash-{}", idx),
                    public_key_hex: "pk".into(),
                    signature_hex: "sig".into(),
                    source: "test".into(),
                    verified: true,
                    created_at_us: idx,
                })
                .expect("append skill signature");
            store
                .append_shell_exec_audit(&ShellExecutionAuditRecord {
                    audit_id: format!("shell-cfg-{}", idx),
                    session_id: Some("sess-2".into()),
                    agent_id: Some("agent-2".into()),
                    execution_backend_id: Some("local-default".into()),
                    command: format!("echo {}", idx),
                    cwd: Some("/workspace".into()),
                    os_containment_requested: false,
                    containment_backend: None,
                    timeout_seconds: 10,
                    cpu_seconds: 2,
                    memory_kb: 131072,
                    exit_code: Some(0),
                    timed_out: false,
                    output_truncated: false,
                    error: None,
                    duration_ms: 1,
                    created_at_us: idx,
                })
                .expect("append shell audit");
            store
                .append_scope_denial(&aria_core::ScopeDenialRecord {
                    denial_id: format!("denial-cfg-{}", idx),
                    kind: aria_core::ScopeDenialKind::DomainPolicy,
                    agent_id: "agent-2".into(),
                    session_id: Some(*session_id.as_bytes()),
                    target: "https://example.com".into(),
                    reason: "blocked".into(),
                    created_at_us: idx,
                })
                .expect("append scope denial");
            store
                .append_request_policy_audit(&RequestPolicyAuditRecord {
                    audit_id: format!("reqpol-cfg-{}", idx),
                    request_id: format!("req-cfg-{}", idx),
                    session_id: session_id.to_string(),
                    user_id: "u1".into(),
                    agent_id: Some("agent-2".into()),
                    channel: "Cli".into(),
                    tool_runtime_policy: aria_core::ToolRuntimePolicy {
                        tool_choice: aria_core::ToolChoicePolicy::Required,
                        allow_parallel_tool_calls: false,
                    },
                    created_at_us: idx,
                })
                .expect("append request policy audit");
            store
                .append_repair_fallback_audit(&RepairFallbackAuditRecord {
                    audit_id: format!("repair-cfg-{}", idx),
                    request_id: format!("req-cfg-{}", idx),
                    session_id: session_id.to_string(),
                    user_id: "u1".into(),
                    agent_id: "agent-2".into(),
                    provider_id: Some("openrouter".into()),
                    model_id: Some("model".into()),
                    tool_name: "register_mcp_server".into(),
                    created_at_us: idx,
                })
                .expect("append repair fallback audit");
            store
                .append_streaming_decision_audit(&StreamingDecisionAuditRecord {
                    audit_id: format!("stream-cfg-{}", idx),
                    request_id: format!("req-cfg-{}", idx),
                    session_id: session_id.to_string(),
                    user_id: "u1".into(),
                    agent_id: "agent-2".into(),
                    phase: "initial".into(),
                    mode: "fallback_used".into(),
                    model_ref: Some("openrouter/model".into()),
                    created_at_us: idx,
                })
                .expect("append streaming decision audit");
            store
                .append_browser_action_audit(&aria_core::BrowserActionAuditRecord {
                    audit_id: format!("browser-audit-cfg-{}", idx),
                    browser_session_id: Some("browser-session-1".into()),
                    session_id: *session_id.as_bytes(),
                    agent_id: "agent-2".into(),
                    profile_id: Some("profile-1".into()),
                    action: aria_core::BrowserActionKind::SessionStart,
                    target: Some("https://example.com".into()),
                    metadata: serde_json::json!({}),
                    created_at_us: idx,
                })
                .expect("append browser action audit");
            store
                .append_browser_challenge_event(&aria_core::BrowserChallengeEvent {
                    event_id: format!("challenge-cfg-{}", idx),
                    browser_session_id: "browser-session-1".into(),
                    session_id: *session_id.as_bytes(),
                    agent_id: "agent-2".into(),
                    profile_id: "profile-1".into(),
                    challenge: aria_core::BrowserChallengeKind::BotDefense,
                    url: Some("https://example.com".into()),
                    message: Some("manual".into()),
                    created_at_us: idx,
                })
                .expect("append browser challenge event");
        }

        let signatures = store
            .list_skill_signatures(Some("skill-b"))
            .expect("list signatures");
        assert_eq!(signatures.len(), 2);
        let audits = store
            .list_shell_exec_audits(Some("sess-2"), Some("agent-2"))
            .expect("list audits");
        assert_eq!(audits.len(), 2);
        assert_eq!(
            store
                .list_scope_denials(Some("agent-2"), Some(&session_id.to_string()))
                .expect("list denials")
                .len(),
            2
        );
        assert_eq!(
            store
                .list_request_policy_audits(Some(&session_id.to_string()), Some("agent-2"))
                .expect("list request policy audits")
                .len(),
            2
        );
        assert_eq!(
            store
                .list_repair_fallback_audits(Some(&session_id.to_string()), Some("agent-2"))
                .expect("list repair fallback audits")
                .len(),
            2
        );
        assert_eq!(
            store
                .list_streaming_decision_audits(Some(&session_id.to_string()), Some("agent-2"))
                .expect("list streaming decision audits")
                .len(),
            2
        );
        assert_eq!(
            store
                .list_browser_action_audits(Some(*session_id.as_bytes()), Some("agent-2"))
                .expect("list browser action audits")
                .len(),
            2
        );
        assert_eq!(
            store
                .list_browser_challenge_events(Some(*session_id.as_bytes()), Some("agent-2"))
                .expect("list browser challenge events")
                .len(),
            2
        );

        // Restore defaults so subsequent tests are unaffected.
        RuntimeStore::configure_operator_retention(
            DEFAULT_SKILL_SIGNATURE_RETENTION_ROWS,
            DEFAULT_SHELL_EXEC_AUDIT_RETENTION_ROWS,
            DEFAULT_SCOPE_DENIAL_RETENTION_ROWS,
            DEFAULT_REQUEST_POLICY_AUDIT_RETENTION_ROWS,
            DEFAULT_REPAIR_FALLBACK_AUDIT_RETENTION_ROWS,
            DEFAULT_STREAMING_DECISION_AUDIT_RETENTION_ROWS,
            DEFAULT_BROWSER_ACTION_AUDIT_RETENTION_ROWS,
            DEFAULT_BROWSER_CHALLENGE_EVENT_RETENTION_ROWS,
        );
    }

    #[test]
    fn runtime_store_round_trips_approval_and_elevation() {
        let (_dir, store) = temp_store();
        let approval = ApprovalRecord {
            approval_id: "a1".into(),
            session_id: [1; 16],
            user_id: "u1".into(),
            channel: GatewayChannel::Cli,
            agent_id: "developer".into(),
            tool_name: "run_shell".into(),
            arguments_json: "{}".into(),
            pending_prompt: "p".into(),
            original_request: "r".into(),
            status: aria_core::ApprovalStatus::Pending,
            created_at_us: 1,
            resolved_at_us: None,
        };
        store.upsert_approval(&approval).expect("write approval");
        assert_eq!(store.read_approval("a1").expect("read approval"), approval);

        let grant = ElevationGrant {
            session_id: [2; 16],
            user_id: "u1".into(),
            agent_id: "omni".into(),
            granted_at_us: 10,
            expires_at_us: Some(20),
        };
        store.upsert_elevation(&grant).expect("write elevation");
        assert_eq!(
            store
                .read_elevation(uuid::Uuid::from_bytes([2; 16]), "omni")
                .expect("read elevation"),
            grant
        );
        assert!(store
            .has_active_elevation(uuid::Uuid::from_bytes([2; 16]), "u1", "omni", 15)
            .expect("active grant"));
    }

    #[test]
    fn runtime_store_tracks_dedupe_delivery_cache_and_jobs() {
        let (_dir, store) = temp_store();
        assert!(store
            .record_dedupe_key_if_new("u:1", 1)
            .expect("insert dedupe"));
        assert!(!store
            .record_dedupe_key_if_new("u:1", 2)
            .expect("duplicate dedupe"));

        let envelope = OutboundEnvelope {
            envelope_id: [3; 16],
            session_id: [4; 16],
            channel: GatewayChannel::Cli,
            recipient_id: "u1".into(),
            provider_message_id: None,
            content: MessageContent::Text("hello".into()),
            attachments: Vec::new(),
            timestamp_us: 30,
        };
        store
            .record_outbound_delivery(&envelope, "sent", None)
            .expect("record delivery");
        let delivery = store
            .read_outbound_delivery([3; 16])
            .expect("read delivery");
        assert_eq!(delivery.status, "sent");

        let session_uuid = uuid::Uuid::new_v4();
        store
            .upsert_cache_snapshot(session_uuid, "developer", &[sample_tool()], 40)
            .expect("write cache snapshot");
        let snapshot = store
            .read_cache_snapshot(session_uuid, "developer")
            .expect("read cache snapshot");
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].embedding, vec![0.1, 0.2]);

        let job = ScheduledPromptJob {
            id: "job-1".into(),
            agent_id: "developer".into(),
            creator_agent: Some("developer".into()),
            executor_agent: Some("developer".into()),
            notifier_agent: None,
            prompt: "ping".into(),
            schedule_str: "every:60s".into(),
            kind: ScheduledJobKind::Notify,
            session_id: Some(*session_uuid.as_bytes()),
            user_id: Some("u1".into()),
            channel: Some(GatewayChannel::Cli),
            status: ScheduledJobStatus::Scheduled,
            last_run_at_us: None,
            last_error: None,
            audit_log: Vec::new(),
            schedule: ScheduleSpec::EverySeconds(60),
        };
        store
            .upsert_job_snapshot(&job.id, &job, 50)
            .expect("write job snapshot");
        let jobs: Vec<ScheduledPromptJob> = store.list_job_snapshots().expect("list job snapshots");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, "job-1");
        store
            .delete_job_snapshot(&job.id)
            .expect("delete job snapshot");
    }

    #[test]
    fn runtime_store_job_leases_are_exclusive_until_expiry() {
        let (_dir, store) = temp_store();
        assert!(store
            .try_acquire_job_lease("job-1", "worker-a", 100, 200)
            .expect("acquire initial lease"));
        assert!(!store
            .try_acquire_job_lease("job-1", "worker-b", 150, 250)
            .expect("reject competing lease"));
        assert!(store
            .try_acquire_job_lease("job-1", "worker-b", 201, 300)
            .expect("expired lease should be acquirable"));
        store
            .release_job_lease("job-1", "worker-b")
            .expect("release job lease");
        assert!(store
            .try_acquire_job_lease("job-1", "worker-a", 202, 400)
            .expect("lease should be free after release"));
    }

    #[test]
    fn runtime_store_round_trips_execution_traces_by_fingerprint() {
        let (_dir, store) = temp_store();
        let fingerprint = TaskFingerprint::from_parts(
            "researcher",
            "execution",
            "summarize the docs",
            &["read_file".into(), "search_tool_registry".into()],
        );
        let trace = ExecutionTrace {
            request_id: "req-1".into(),
            session_id: "sess-1".into(),
            user_id: "u1".into(),
            agent_id: "researcher".into(),
            channel: GatewayChannel::Cli,
            prompt_mode: "execution".into(),
            task_fingerprint: fingerprint.clone(),
            user_input_summary: "summarize the docs".into(),
            tool_names: vec!["read_file".into(), "search_tool_registry".into()],
            retrieved_corpora: vec!["workspace".into()],
            outcome: TraceOutcome::Succeeded,
            latency_ms: 120,
            response_summary: "summary complete".into(),
            tool_runtime_policy: None,
            recorded_at_us: 1000,
        };

        store
            .record_execution_trace(&trace)
            .expect("record execution trace");

        let traces = store
            .list_execution_traces_by_fingerprint(&fingerprint.key)
            .expect("list execution traces");
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0], trace);
    }

    #[test]
    fn runtime_store_round_trips_reward_events_for_request() {
        let (_dir, store) = temp_store();
        let reward = RewardEvent {
            event_id: "evt-1".into(),
            request_id: "req-1".into(),
            session_id: "sess-1".into(),
            kind: RewardKind::Accepted,
            value: 1,
            notes: Some("user accepted final answer".into()),
            recorded_at_us: 2000,
        };

        store.record_reward_event(&reward).expect("record reward");

        let rewards = store
            .list_reward_events_for_request("req-1")
            .expect("list reward events");
        assert_eq!(rewards.len(), 1);
        assert_eq!(rewards[0], reward);
    }

    #[test]
    fn runtime_store_lists_execution_traces_by_session_and_fingerprint() {
        let (_dir, store) = temp_store();
        let fingerprint =
            TaskFingerprint::from_parts("developer", "execution", "write the file", &Vec::new());
        let trace_a = ExecutionTrace {
            request_id: "req-1".into(),
            session_id: "sess-a".into(),
            user_id: "u1".into(),
            agent_id: "developer".into(),
            channel: GatewayChannel::Cli,
            prompt_mode: "execution".into(),
            task_fingerprint: fingerprint.clone(),
            user_input_summary: "write the file".into(),
            tool_names: Vec::new(),
            retrieved_corpora: Vec::new(),
            outcome: TraceOutcome::Succeeded,
            latency_ms: 10,
            response_summary: "ok".into(),
            tool_runtime_policy: None,
            recorded_at_us: 1,
        };
        let mut trace_b = trace_a.clone();
        trace_b.request_id = "req-2".into();
        trace_b.session_id = "sess-b".into();

        store
            .record_execution_trace(&trace_a)
            .expect("record trace a");
        store
            .record_execution_trace(&trace_b)
            .expect("record trace b");

        let traces = store
            .list_execution_traces_by_session_and_fingerprint("sess-a", &fingerprint.key)
            .expect("list by session and fingerprint");
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].request_id, "req-1");
    }

    #[test]
    fn runtime_store_prunes_learning_records_to_retention_limits() {
        let (_dir, store) = temp_store();
        for idx in 0..3u64 {
            store
                .record_execution_trace(&ExecutionTrace {
                    request_id: format!("req-{}", idx),
                    session_id: "sess-1".into(),
                    user_id: "u1".into(),
                    agent_id: "developer".into(),
                    channel: GatewayChannel::Cli,
                    prompt_mode: "execution".into(),
                    task_fingerprint: TaskFingerprint::from_parts(
                        "developer",
                        "execution",
                        "write the file",
                        &Vec::new(),
                    ),
                    user_input_summary: "write the file".into(),
                    tool_names: Vec::new(),
                    retrieved_corpora: Vec::new(),
                    outcome: TraceOutcome::Succeeded,
                    latency_ms: 1,
                    response_summary: "ok".into(),
                    tool_runtime_policy: None,
                    recorded_at_us: idx,
                })
                .expect("record trace");
            store
                .record_reward_event(&RewardEvent {
                    event_id: format!("evt-{}", idx),
                    request_id: format!("req-{}", idx),
                    session_id: "sess-1".into(),
                    kind: RewardKind::Accepted,
                    value: 1,
                    notes: None,
                    recorded_at_us: idx,
                })
                .expect("record reward");
            store
                .append_learning_derivative_event(&LearningDerivativeEvent {
                    event_id: format!("derivative-{}", idx),
                    task_fingerprint: TaskFingerprint::from_parts(
                        "developer",
                        "execution",
                        "write the file",
                        &Vec::new(),
                    )
                    .key,
                    kind: LearningDerivativeKind::PromptCompile,
                    artifact_id: format!("artifact-{}", idx),
                    notes: "derivative".into(),
                    created_at_us: idx,
                })
                .expect("append derivative event");
        }

        store
            .prune_learning_records(2, 2, 2)
            .expect("prune learning records");

        let traces = store
            .list_execution_traces_by_fingerprint(
                &TaskFingerprint::from_parts(
                    "developer",
                    "execution",
                    "write the file",
                    &Vec::new(),
                )
                .key,
            )
            .expect("list traces");
        assert_eq!(traces.len(), 2);
        let rewards = store
            .list_reward_events_for_request("req-2")
            .expect("list rewards req-2");
        assert_eq!(rewards.len(), 1);
        let old_rewards = store
            .list_reward_events_for_request("req-0")
            .expect("list rewards req-0");
        assert!(old_rewards.is_empty());
        let derivatives = store
            .list_learning_derivative_events(
                &TaskFingerprint::from_parts(
                    "developer",
                    "execution",
                    "write the file",
                    &Vec::new(),
                )
                .key,
            )
            .expect("list derivative events");
        assert_eq!(derivatives.len(), 2);
    }

    #[test]
    fn runtime_store_builds_replay_samples_for_fingerprint() {
        let (_dir, store) = temp_store();
        let fingerprint =
            TaskFingerprint::from_parts("developer", "execution", "write the file", &Vec::new());
        let trace = ExecutionTrace {
            request_id: "req-1".into(),
            session_id: "sess-1".into(),
            user_id: "u1".into(),
            agent_id: "developer".into(),
            channel: GatewayChannel::Cli,
            prompt_mode: "execution".into(),
            task_fingerprint: fingerprint.clone(),
            user_input_summary: "write the file".into(),
            tool_names: Vec::new(),
            retrieved_corpora: vec!["workspace".into()],
            outcome: TraceOutcome::Succeeded,
            latency_ms: 5,
            response_summary: "done".into(),
            tool_runtime_policy: None,
            recorded_at_us: 10,
        };
        store.record_execution_trace(&trace).expect("record trace");
        store
            .record_reward_event(&RewardEvent {
                event_id: "evt-1".into(),
                request_id: "req-1".into(),
                session_id: "sess-1".into(),
                kind: RewardKind::Accepted,
                value: 1,
                notes: None,
                recorded_at_us: 11,
            })
            .expect("record reward");

        let samples = store
            .build_replay_samples_for_fingerprint(&fingerprint.key)
            .expect("build replay samples");
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].trace.request_id, "req-1");
        assert_eq!(samples[0].reward_score, 1);
        assert_eq!(samples[0].rewards.len(), 1);
    }

    #[test]
    fn runtime_store_summarizes_fingerprint_for_evaluation() {
        let (_dir, store) = temp_store();
        let fingerprint =
            TaskFingerprint::from_parts("developer", "execution", "summarize docs", &Vec::new());
        for (request_id, outcome, reward, ts) in [
            ("req-1", TraceOutcome::Succeeded, 1, 10u64),
            ("req-2", TraceOutcome::ApprovalRequired, -1, 20u64),
        ] {
            store
                .record_execution_trace(&ExecutionTrace {
                    request_id: request_id.into(),
                    session_id: "sess-1".into(),
                    user_id: "u1".into(),
                    agent_id: "developer".into(),
                    channel: GatewayChannel::Cli,
                    prompt_mode: "execution".into(),
                    task_fingerprint: fingerprint.clone(),
                    user_input_summary: "summarize docs".into(),
                    tool_names: Vec::new(),
                    retrieved_corpora: Vec::new(),
                    outcome,
                    latency_ms: 5,
                    response_summary: "done".into(),
                    tool_runtime_policy: None,
                    recorded_at_us: ts,
                })
                .expect("record trace");
            store
                .record_reward_event(&RewardEvent {
                    event_id: format!("evt-{}", request_id),
                    request_id: request_id.into(),
                    session_id: "sess-1".into(),
                    kind: if reward > 0 {
                        RewardKind::Accepted
                    } else {
                        RewardKind::Rejected
                    },
                    value: reward,
                    notes: None,
                    recorded_at_us: ts + 1,
                })
                .expect("record reward");
        }

        let summary = store
            .summarize_fingerprint_for_evaluation(&fingerprint.key)
            .expect("summarize fingerprint")
            .expect("summary exists");
        assert_eq!(summary.trace_count, 2);
        assert_eq!(summary.success_count, 1);
        assert_eq!(summary.approval_required_count, 1);
        assert_eq!(summary.cumulative_reward, 0);
        assert_eq!(summary.latest_recorded_at_us, 20);
    }

    #[test]
    fn runtime_store_lists_fingerprint_clusters() {
        let (_dir, store) = temp_store();
        let fingerprint =
            TaskFingerprint::from_parts("developer", "execution", "summarize docs", &Vec::new());
        store
            .record_execution_trace(&ExecutionTrace {
                request_id: "req-1".into(),
                session_id: "sess-1".into(),
                user_id: "u1".into(),
                agent_id: "developer".into(),
                channel: GatewayChannel::Cli,
                prompt_mode: "execution".into(),
                task_fingerprint: fingerprint.clone(),
                user_input_summary: "summarize docs".into(),
                tool_names: Vec::new(),
                retrieved_corpora: vec!["workspace".into()],
                outcome: TraceOutcome::Succeeded,
                latency_ms: 5,
                response_summary: "done".into(),
                tool_runtime_policy: None,
                recorded_at_us: 10,
            })
            .expect("record trace");
        let clusters = store.list_fingerprint_clusters().expect("list clusters");
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].summary.task_fingerprint, fingerprint.key);
        assert_eq!(clusters[0].top_agents, vec!["developer".to_string()]);
        assert_eq!(clusters[0].top_prompt_modes, vec!["execution".to_string()]);
    }

    #[test]
    fn runtime_store_round_trips_candidate_artifacts() {
        let (_dir, store) = temp_store();
        let record = CandidateArtifactRecord {
            candidate_id: "cand-1".into(),
            task_fingerprint: "fp-1".into(),
            kind: CandidateArtifactKind::Macro,
            status: CandidateArtifactStatus::Proposed,
            title: "Summarize docs macro".into(),
            summary: "Compile repeated summarize-docs workflow into a macro.".into(),
            payload_json: r#"{"steps":["search","read","summarize"]}"#.into(),
            created_at_us: 10,
            updated_at_us: 10,
        };
        store
            .upsert_candidate_artifact(&record)
            .expect("upsert candidate");
        let records = store
            .list_candidate_artifacts_for_fingerprint("fp-1")
            .expect("list candidates");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0], record);
    }

    #[test]
    fn runtime_store_synthesizes_macro_candidate_from_stable_tool_workflow() {
        let (_dir, store) = temp_store();
        let fingerprint = TaskFingerprint::from_parts(
            "developer",
            "execution",
            "summarize docs",
            &["read_file".into()],
        );
        for idx in 0..3 {
            let req_id = format!("req-{}", idx);
            store
                .record_execution_trace(&ExecutionTrace {
                    request_id: req_id.clone(),
                    session_id: "sess-1".into(),
                    user_id: "u1".into(),
                    agent_id: "developer".into(),
                    channel: GatewayChannel::Cli,
                    prompt_mode: "execution".into(),
                    task_fingerprint: fingerprint.clone(),
                    user_input_summary: "summarize docs".into(),
                    tool_names: vec!["read_file".into()],
                    retrieved_corpora: vec!["workspace".into()],
                    outcome: TraceOutcome::Succeeded,
                    latency_ms: 5,
                    response_summary: "done".into(),
                    tool_runtime_policy: None,
                    recorded_at_us: 10 + idx,
                })
                .expect("record trace");
            store
                .record_reward_event(&RewardEvent {
                    event_id: format!("evt-{}", idx),
                    request_id: req_id,
                    session_id: "sess-1".into(),
                    kind: RewardKind::Accepted,
                    value: 1,
                    notes: None,
                    recorded_at_us: 20 + idx,
                })
                .expect("record reward");
        }

        let created = store
            .synthesize_candidate_artifacts(100)
            .expect("synthesize candidates");
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].kind, CandidateArtifactKind::Macro);

        let persisted = store
            .list_candidate_artifacts_for_fingerprint(&fingerprint.key)
            .expect("list candidates");
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].kind, CandidateArtifactKind::Macro);
    }

    #[test]
    fn runtime_store_skips_unstable_clusters_when_synthesizing_candidates() {
        let (_dir, store) = temp_store();
        let fingerprint =
            TaskFingerprint::from_parts("developer", "execution", "fragile workflow", &Vec::new());
        for (idx, outcome) in [
            (0u64, TraceOutcome::Succeeded),
            (1u64, TraceOutcome::Failed),
            (2u64, TraceOutcome::Succeeded),
        ] {
            store
                .record_execution_trace(&ExecutionTrace {
                    request_id: format!("req-{}", idx),
                    session_id: "sess-1".into(),
                    user_id: "u1".into(),
                    agent_id: "developer".into(),
                    channel: GatewayChannel::Cli,
                    prompt_mode: "execution".into(),
                    task_fingerprint: fingerprint.clone(),
                    user_input_summary: "fragile workflow".into(),
                    tool_names: Vec::new(),
                    retrieved_corpora: Vec::new(),
                    outcome,
                    latency_ms: 5,
                    response_summary: "done".into(),
                    tool_runtime_policy: None,
                    recorded_at_us: 10 + idx,
                })
                .expect("record trace");
        }

        let created = store
            .synthesize_candidate_artifacts(100)
            .expect("synthesize candidates");
        assert!(created.is_empty());
    }

    #[test]
    fn runtime_store_evaluates_candidate_and_persists_eval_run() {
        let (_dir, store) = temp_store();
        let fingerprint = TaskFingerprint::from_parts(
            "developer",
            "execution",
            "summarize docs",
            &["read_file".into()],
        );
        let candidate = CandidateArtifactRecord {
            candidate_id: "cand-1".into(),
            task_fingerprint: fingerprint.key.clone(),
            kind: CandidateArtifactKind::Macro,
            status: CandidateArtifactStatus::Proposed,
            title: "Macro".into(),
            summary: "Macro".into(),
            payload_json: "{}".into(),
            created_at_us: 1,
            updated_at_us: 1,
        };
        store
            .upsert_candidate_artifact(&candidate)
            .expect("upsert candidate");
        for idx in 0..3 {
            let req_id = format!("req-{}", idx);
            store
                .record_execution_trace(&ExecutionTrace {
                    request_id: req_id.clone(),
                    session_id: "sess-1".into(),
                    user_id: "u1".into(),
                    agent_id: "developer".into(),
                    channel: GatewayChannel::Cli,
                    prompt_mode: "execution".into(),
                    task_fingerprint: fingerprint.clone(),
                    user_input_summary: "summarize docs".into(),
                    tool_names: vec!["read_file".into()],
                    retrieved_corpora: vec!["workspace".into()],
                    outcome: TraceOutcome::Succeeded,
                    latency_ms: 5,
                    response_summary: "done".into(),
                    tool_runtime_policy: None,
                    recorded_at_us: 10 + idx,
                })
                .expect("record trace");
            store
                .record_reward_event(&RewardEvent {
                    event_id: format!("evt-{}", idx),
                    request_id: req_id,
                    session_id: "sess-1".into(),
                    kind: RewardKind::Accepted,
                    value: 1,
                    notes: None,
                    recorded_at_us: 20 + idx,
                })
                .expect("record reward");
        }

        let run = store
            .evaluate_candidate_artifact("cand-1", 100)
            .expect("evaluate candidate");
        assert!(run.passed);
        let runs = store
            .list_candidate_evaluation_runs("cand-1")
            .expect("list eval runs");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0], run);
    }

    #[test]
    fn runtime_store_blocks_privileged_candidate_promotion_without_approval() {
        let (_dir, store) = temp_store();
        let candidate = CandidateArtifactRecord {
            candidate_id: "cand-1".into(),
            task_fingerprint: "fp-1".into(),
            kind: CandidateArtifactKind::Macro,
            status: CandidateArtifactStatus::Proposed,
            title: "Macro".into(),
            summary: "Macro".into(),
            payload_json: "{}".into(),
            created_at_us: 1,
            updated_at_us: 1,
        };
        store
            .upsert_candidate_artifact(&candidate)
            .expect("upsert candidate");
        store
            .append_candidate_evaluation_run(&CandidateEvaluationRun {
                run_id: "eval-1".into(),
                candidate_id: "cand-1".into(),
                task_fingerprint: "fp-1".into(),
                sample_count: 3,
                score: 90,
                passed: true,
                notes: String::new(),
                created_at_us: 2,
            })
            .expect("append eval");

        let profiles = vec![AgentCapabilityProfile {
            agent_id: "omni".into(),
            class: aria_core::AgentClass::Generalist,
            tool_allowlist: Vec::new(),
            skill_allowlist: vec![],
            mcp_server_allowlist: vec![],
            mcp_tool_allowlist: vec![],
            mcp_prompt_allowlist: vec![],
            mcp_resource_allowlist: vec![],
            filesystem_scopes: vec![],
            retrieval_scopes: vec![],
            delegation_scope: None,
            web_domain_allowlist: vec![],
            web_domain_blocklist: vec![],
            browser_profile_allowlist: vec![],
            browser_action_scope: None,
        computer_profile_allowlist: vec![],
        computer_action_scope: None,
            browser_session_scope: None,
            crawl_scope: None,
            web_approval_policy: None,
            web_transport_allowlist: vec![],
            requires_elevation: true,
            side_effect_level: aria_core::SideEffectLevel::Privileged,
            trust_profile: None,
        }];
        let record = store
            .apply_candidate_promotion(
                "cand-1",
                &profiles,
                None,
                CandidatePromotionAction::Promote,
                100,
            )
            .expect("apply promotion");
        assert_eq!(record.status, CandidatePromotionStatus::Blocked);
        let candidate_after = store
            .read_candidate_artifact("cand-1")
            .expect("read candidate");
        assert_eq!(candidate_after.status, CandidateArtifactStatus::Proposed);
    }

    #[test]
    fn runtime_store_applies_candidate_promotion_and_updates_status() {
        let (_dir, store) = temp_store();
        let candidate = CandidateArtifactRecord {
            candidate_id: "cand-2".into(),
            task_fingerprint: "fp-2".into(),
            kind: CandidateArtifactKind::Prompt,
            status: CandidateArtifactStatus::Proposed,
            title: "Prompt".into(),
            summary: "Prompt".into(),
            payload_json: "{}".into(),
            created_at_us: 1,
            updated_at_us: 1,
        };
        store
            .upsert_candidate_artifact(&candidate)
            .expect("upsert candidate");
        store
            .append_candidate_evaluation_run(&CandidateEvaluationRun {
                run_id: "eval-2".into(),
                candidate_id: "cand-2".into(),
                task_fingerprint: "fp-2".into(),
                sample_count: 3,
                score: 90,
                passed: true,
                notes: String::new(),
                created_at_us: 2,
            })
            .expect("append eval");

        let record = store
            .apply_candidate_promotion(
                "cand-2",
                &[],
                Some("u1".into()),
                CandidatePromotionAction::Promote,
                100,
            )
            .expect("apply promotion");
        assert_eq!(record.status, CandidatePromotionStatus::Applied);
        let candidate_after = store
            .read_candidate_artifact("cand-2")
            .expect("read candidate");
        assert_eq!(candidate_after.status, CandidateArtifactStatus::Promoted);
        let promotions = store
            .list_candidate_promotion_records("cand-2")
            .expect("list promotions");
        assert_eq!(promotions.len(), 1);
        assert_eq!(promotions[0], record);
        let events = store
            .list_learning_derivative_events("fp-2")
            .expect("list derivative events");
        assert!(events
            .iter()
            .any(|event| event.kind == LearningDerivativeKind::Promotion));
    }

    #[test]
    fn runtime_store_lists_promoted_candidates_for_matching_request() {
        let (_dir, store) = temp_store();
        let record = CandidateArtifactRecord {
            candidate_id: "cand-1".into(),
            task_fingerprint: "v1|agent=developer|mode=execution|text=list workspace files|tools="
                .into(),
            kind: CandidateArtifactKind::Prompt,
            status: CandidateArtifactStatus::Promoted,
            title: "Prompt".into(),
            summary: "Prompt".into(),
            payload_json: "{}".into(),
            created_at_us: 1,
            updated_at_us: 1,
        };
        store
            .upsert_candidate_artifact(&record)
            .expect("upsert candidate");

        let matches = store
            .list_promoted_candidates_for_request("developer", "execution", "list workspace files")
            .expect("list promoted candidates");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].candidate_id, "cand-1");
    }

    #[test]
    fn runtime_store_synthesizes_and_lists_selector_models() {
        let (_dir, store) = temp_store();
        let fingerprint = TaskFingerprint::from_parts(
            "developer",
            "execution",
            "write the file",
            &["write_file".into()],
        );
        for idx in 0..3 {
            let req_id = format!("req-{}", idx);
            store
                .record_execution_trace(&ExecutionTrace {
                    request_id: req_id.clone(),
                    session_id: "sess-1".into(),
                    user_id: "u1".into(),
                    agent_id: "developer".into(),
                    channel: GatewayChannel::Cli,
                    prompt_mode: "execution".into(),
                    task_fingerprint: fingerprint.clone(),
                    user_input_summary: "write the file".into(),
                    tool_names: vec!["write_file".into()],
                    retrieved_corpora: Vec::new(),
                    outcome: TraceOutcome::Succeeded,
                    latency_ms: 1,
                    response_summary: "ok".into(),
                    tool_runtime_policy: None,
                    recorded_at_us: idx,
                })
                .expect("record trace");
            store
                .record_reward_event(&RewardEvent {
                    event_id: format!("evt-{}", idx),
                    request_id: req_id,
                    session_id: "sess-1".into(),
                    kind: RewardKind::Accepted,
                    value: 1,
                    notes: None,
                    recorded_at_us: idx,
                })
                .expect("record reward");
        }

        let models = store
            .synthesize_selector_models(100)
            .expect("synthesize selector models");
        assert!(models
            .iter()
            .any(|m| m.kind == SelectorModelKind::ToolRanker));
        let matches = store
            .list_selector_models_for_request("developer", "execution", "write the file")
            .expect("list selector models");
        assert!(matches
            .iter()
            .any(|m| m.kind == SelectorModelKind::ToolRanker));
    }

    #[test]
    fn runtime_store_builds_prompt_optimization_datasets() {
        let (_dir, store) = temp_store();
        let fingerprint =
            TaskFingerprint::from_parts("developer", "execution", "summarize release notes", &[]);
        for idx in 0..2 {
            let req_id = format!("req-prompt-{}", idx);
            store
                .record_execution_trace(&ExecutionTrace {
                    request_id: req_id.clone(),
                    session_id: "sess-1".into(),
                    user_id: "u1".into(),
                    agent_id: "developer".into(),
                    channel: GatewayChannel::Cli,
                    prompt_mode: "execution".into(),
                    task_fingerprint: fingerprint.clone(),
                    user_input_summary: "summarize release notes".into(),
                    tool_names: vec![],
                    retrieved_corpora: vec!["workspace".into()],
                    outcome: TraceOutcome::Succeeded,
                    latency_ms: 1,
                    response_summary: "short release digest".into(),
                    tool_runtime_policy: None,
                    recorded_at_us: idx,
                })
                .expect("record trace");
            store
                .record_reward_event(&RewardEvent {
                    event_id: format!("evt-prompt-{}", idx),
                    request_id: req_id,
                    session_id: "sess-1".into(),
                    kind: RewardKind::Accepted,
                    value: 1,
                    notes: None,
                    recorded_at_us: idx,
                })
                .expect("record reward");
        }

        let datasets = store
            .build_prompt_optimization_datasets(2)
            .expect("build prompt datasets");
        assert_eq!(datasets.len(), 1);
        assert_eq!(datasets[0].success_count, 2);
        assert_eq!(datasets[0].agent_id, "developer");
    }

    #[test]
    fn runtime_store_compiles_prompt_optimization_candidates() {
        let (_dir, store) = temp_store();
        let fingerprint =
            TaskFingerprint::from_parts("developer", "execution", "summarize release notes", &[]);
        for idx in 0..2 {
            let req_id = format!("req-cp-{}", idx);
            store
                .record_execution_trace(&ExecutionTrace {
                    request_id: req_id.clone(),
                    session_id: "sess-1".into(),
                    user_id: "u1".into(),
                    agent_id: "developer".into(),
                    channel: GatewayChannel::Cli,
                    prompt_mode: "execution".into(),
                    task_fingerprint: fingerprint.clone(),
                    user_input_summary: "summarize release notes".into(),
                    tool_names: vec![],
                    retrieved_corpora: vec!["workspace".into()],
                    outcome: TraceOutcome::Succeeded,
                    latency_ms: 1,
                    response_summary: "short release digest".into(),
                    tool_runtime_policy: None,
                    recorded_at_us: idx,
                })
                .expect("record trace");
            store
                .record_reward_event(&RewardEvent {
                    event_id: format!("evt-cp-{}", idx),
                    request_id: req_id,
                    session_id: "sess-1".into(),
                    kind: RewardKind::Accepted,
                    value: 1,
                    notes: None,
                    recorded_at_us: idx,
                })
                .expect("record reward");
        }

        let candidates = store
            .compile_prompt_optimization_candidates(100, 2)
            .expect("compile prompt candidates");
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].kind,
            aria_learning::CandidateArtifactKind::Prompt
        );
        assert!(candidates[0]
            .payload_json
            .contains("\"compiled_from\":\"prompt_optimization_dataset\""));
    }

    #[test]
    fn runtime_store_builds_macro_compilation_datasets() {
        let (_dir, store) = temp_store();
        let fingerprint = TaskFingerprint::from_parts(
            "developer",
            "execution",
            "prepare release file",
            &["read_file".into(), "write_file".into()],
        );
        for idx in 0..2 {
            let req_id = format!("req-macro-ds-{}", idx);
            store
                .record_execution_trace(&ExecutionTrace {
                    request_id: req_id.clone(),
                    session_id: "sess-1".into(),
                    user_id: "u1".into(),
                    agent_id: "developer".into(),
                    channel: GatewayChannel::Cli,
                    prompt_mode: "execution".into(),
                    task_fingerprint: fingerprint.clone(),
                    user_input_summary: "prepare release file".into(),
                    tool_names: vec!["read_file".into(), "write_file".into()],
                    retrieved_corpora: vec!["workspace".into()],
                    outcome: TraceOutcome::Succeeded,
                    latency_ms: 1,
                    response_summary: "release file created".into(),
                    tool_runtime_policy: None,
                    recorded_at_us: idx,
                })
                .expect("record trace");
            store
                .record_reward_event(&RewardEvent {
                    event_id: format!("evt-macro-ds-{}", idx),
                    request_id: req_id,
                    session_id: "sess-1".into(),
                    kind: RewardKind::Accepted,
                    value: 1,
                    notes: None,
                    recorded_at_us: idx,
                })
                .expect("record reward");
        }

        let datasets = store
            .build_macro_compilation_datasets(2)
            .expect("build macro datasets");
        assert_eq!(datasets.len(), 1);
        assert_eq!(
            datasets[0].dominant_tool_sequence,
            vec!["read_file".to_string(), "write_file".to_string()]
        );
    }

    #[test]
    fn runtime_store_compiles_macro_candidates() {
        let (_dir, store) = temp_store();
        let fingerprint = TaskFingerprint::from_parts(
            "developer",
            "execution",
            "prepare release file",
            &["read_file".into(), "write_file".into()],
        );
        for idx in 0..2 {
            let req_id = format!("req-macro-cp-{}", idx);
            store
                .record_execution_trace(&ExecutionTrace {
                    request_id: req_id.clone(),
                    session_id: "sess-1".into(),
                    user_id: "u1".into(),
                    agent_id: "developer".into(),
                    channel: GatewayChannel::Cli,
                    prompt_mode: "execution".into(),
                    task_fingerprint: fingerprint.clone(),
                    user_input_summary: "prepare release file".into(),
                    tool_names: vec!["read_file".into(), "write_file".into()],
                    retrieved_corpora: vec!["workspace".into()],
                    outcome: TraceOutcome::Succeeded,
                    latency_ms: 1,
                    response_summary: "release file created".into(),
                    tool_runtime_policy: None,
                    recorded_at_us: idx,
                })
                .expect("record trace");
            store
                .record_reward_event(&RewardEvent {
                    event_id: format!("evt-macro-cp-{}", idx),
                    request_id: req_id,
                    session_id: "sess-1".into(),
                    kind: RewardKind::Accepted,
                    value: 1,
                    notes: None,
                    recorded_at_us: idx,
                })
                .expect("record reward");
        }

        let candidates = store
            .compile_macro_candidates(100, 2)
            .expect("compile macro candidates");
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].kind,
            aria_learning::CandidateArtifactKind::Macro
        );
        assert!(candidates[0]
            .payload_json
            .contains("\"compiled_from\":\"macro_compilation_dataset\""));
        assert!(candidates[0]
            .payload_json
            .contains("\"tools\":[\"read_file\",\"write_file\"]"));
    }

    #[test]
    fn runtime_store_builds_wasm_compilation_datasets() {
        let (_dir, store) = temp_store();
        let fingerprint = TaskFingerprint::from_parts(
            "developer",
            "execution",
            "prepare release file",
            &["read_file".into(), "write_file".into()],
        );
        for idx in 0..5 {
            let req_id = format!("req-wasm-ds-{}", idx);
            store
                .record_execution_trace(&ExecutionTrace {
                    request_id: req_id.clone(),
                    session_id: "sess-1".into(),
                    user_id: "u1".into(),
                    agent_id: "developer".into(),
                    channel: GatewayChannel::Cli,
                    prompt_mode: "execution".into(),
                    task_fingerprint: fingerprint.clone(),
                    user_input_summary: "prepare release file".into(),
                    tool_names: vec!["read_file".into(), "write_file".into()],
                    retrieved_corpora: vec!["workspace".into()],
                    outcome: TraceOutcome::Succeeded,
                    latency_ms: 1,
                    response_summary: "release file created".into(),
                    tool_runtime_policy: None,
                    recorded_at_us: idx,
                })
                .expect("record trace");
            store
                .record_reward_event(&RewardEvent {
                    event_id: format!("evt-wasm-ds-{}", idx),
                    request_id: req_id,
                    session_id: "sess-1".into(),
                    kind: RewardKind::Accepted,
                    value: 1,
                    notes: None,
                    recorded_at_us: idx,
                })
                .expect("record reward");
        }

        let datasets = store
            .build_wasm_compilation_datasets(5)
            .expect("build wasm datasets");
        assert_eq!(datasets.len(), 1);
        assert_eq!(
            datasets[0].deterministic_tool_sequence,
            vec!["read_file".to_string(), "write_file".to_string()]
        );
    }

    #[test]
    fn runtime_store_compiles_wasm_candidates() {
        let (_dir, store) = temp_store();
        let fingerprint = TaskFingerprint::from_parts(
            "developer",
            "execution",
            "prepare release file",
            &["read_file".into(), "write_file".into()],
        );
        for idx in 0..5 {
            let req_id = format!("req-wasm-cp-{}", idx);
            store
                .record_execution_trace(&ExecutionTrace {
                    request_id: req_id.clone(),
                    session_id: "sess-1".into(),
                    user_id: "u1".into(),
                    agent_id: "developer".into(),
                    channel: GatewayChannel::Cli,
                    prompt_mode: "execution".into(),
                    task_fingerprint: fingerprint.clone(),
                    user_input_summary: "prepare release file".into(),
                    tool_names: vec!["read_file".into(), "write_file".into()],
                    retrieved_corpora: vec!["workspace".into()],
                    outcome: TraceOutcome::Succeeded,
                    latency_ms: 1,
                    response_summary: "release file created".into(),
                    tool_runtime_policy: None,
                    recorded_at_us: idx,
                })
                .expect("record trace");
            store
                .record_reward_event(&RewardEvent {
                    event_id: format!("evt-wasm-cp-{}", idx),
                    request_id: req_id,
                    session_id: "sess-1".into(),
                    kind: RewardKind::Accepted,
                    value: 1,
                    notes: None,
                    recorded_at_us: idx,
                })
                .expect("record reward");
        }

        let candidates = store
            .compile_wasm_candidates(100, 5)
            .expect("compile wasm candidates");
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].kind,
            aria_learning::CandidateArtifactKind::Wasm
        );
        assert!(candidates[0]
            .payload_json
            .contains("\"compiled_from\":\"wasm_compilation_dataset\""));
        assert!(candidates[0]
            .payload_json
            .contains("\"activation\":\"blocked_until_manual_runtime_support\""));
    }

    #[test]
    fn runtime_store_records_learning_derivative_events_and_metrics() {
        let (_dir, store) = temp_store();
        let event = LearningDerivativeEvent {
            event_id: "evt-1".into(),
            task_fingerprint: "fp-1".into(),
            kind: LearningDerivativeKind::PromptCompile,
            artifact_id: "prompt:fp-1".into(),
            notes: "compiled prompt candidate".into(),
            created_at_us: 10,
        };
        store
            .append_learning_derivative_event(&event)
            .expect("append derivative event");
        let listed = store
            .list_learning_derivative_events("fp-1")
            .expect("list derivative events");
        assert_eq!(listed, vec![event]);

        let metrics = store.learning_metrics_snapshot().expect("metrics snapshot");
        assert_eq!(metrics.derivative_event_count, 1);
    }

    #[test]
    fn runtime_store_records_promotion_in_derivative_audit() {
        let (_dir, store) = temp_store();
        let candidate = CandidateArtifactRecord {
            candidate_id: "cand-promote".into(),
            task_fingerprint: "fp-promote".into(),
            kind: CandidateArtifactKind::Prompt,
            status: CandidateArtifactStatus::Proposed,
            title: "Prompt".into(),
            summary: "Prompt".into(),
            payload_json: "{}".into(),
            created_at_us: 1,
            updated_at_us: 1,
        };
        store
            .upsert_candidate_artifact(&candidate)
            .expect("upsert candidate");
        store
            .append_candidate_evaluation_run(&CandidateEvaluationRun {
                run_id: "eval-promote".into(),
                candidate_id: "cand-promote".into(),
                task_fingerprint: "fp-promote".into(),
                sample_count: 3,
                score: 90,
                passed: true,
                notes: String::new(),
                created_at_us: 2,
            })
            .expect("append eval");

        let _ = store
            .apply_candidate_promotion(
                "cand-promote",
                &[],
                Some("u1".into()),
                CandidatePromotionAction::Promote,
                100,
            )
            .expect("apply promotion");

        let events = store
            .list_learning_derivative_events("fp-promote")
            .expect("list derivative events");
        assert!(events
            .iter()
            .any(|event| event.kind == LearningDerivativeKind::Promotion));
    }
}
