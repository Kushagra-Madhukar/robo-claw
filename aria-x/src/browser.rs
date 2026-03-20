fn browser_profiles_root(sessions_dir: &Path) -> PathBuf {
    sessions_dir.join("browser_profiles")
}

fn browser_profile_dir(sessions_dir: &Path, profile_id: &str) -> PathBuf {
    browser_profiles_root(sessions_dir).join(profile_id)
}

fn current_browser_binding_for_agent(
    sessions_dir: &Path,
    session_id: aria_core::Uuid,
    agent_id: &str,
) -> Result<Option<aria_core::BrowserProfileBindingRecord>, OrchestratorError> {
    Ok(RuntimeStore::for_sessions_dir(sessions_dir)
        .list_browser_profile_bindings(Some(session_id), Some(agent_id))
        .map_err(OrchestratorError::ToolError)?
        .into_iter()
        .next())
}

fn current_browser_profile_for_agent(
    sessions_dir: &Path,
    session_id: aria_core::Uuid,
    agent_id: &str,
) -> Result<Option<aria_core::BrowserProfile>, OrchestratorError> {
    let Some(binding) = current_browser_binding_for_agent(sessions_dir, session_id, agent_id)? else {
        return default_browser_profile(sessions_dir);
    };
    let profile = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_browser_profiles()
        .map_err(OrchestratorError::ToolError)?
        .into_iter()
        .find(|profile| profile.profile_id == binding.profile_id);
    if profile.is_some() {
        Ok(profile)
    } else {
        default_browser_profile(sessions_dir)
    }
}

fn default_browser_profile(
    sessions_dir: &Path,
) -> Result<Option<aria_core::BrowserProfile>, OrchestratorError> {
    let mut profiles = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_browser_profiles()
        .map_err(OrchestratorError::ToolError)?;
    profiles.sort_by_key(|profile| std::cmp::Reverse(profile.created_at_us));
    if let Some(default_profile) = profiles.iter().find(|profile| profile.is_default).cloned() {
        return Ok(Some(default_profile));
    }
    if profiles.len() == 1 {
        return Ok(profiles.into_iter().next());
    }
    Ok(None)
}

fn current_browser_session_for_agent(
    sessions_dir: &Path,
    session_id: aria_core::Uuid,
    agent_id: &str,
    browser_session_id: &str,
) -> Result<aria_core::BrowserSessionRecord, OrchestratorError> {
    RuntimeStore::for_sessions_dir(sessions_dir)
        .list_browser_sessions(Some(session_id), Some(agent_id))
        .map_err(OrchestratorError::ToolError)?
        .into_iter()
        .find(|record| record.browser_session_id == browser_session_id)
        .ok_or_else(|| {
            OrchestratorError::ToolError(format!(
                "browser session '{}' not found",
                browser_session_id
            ))
        })
}

fn latest_browser_session_state_for_profile(
    sessions_dir: &Path,
    agent_id: &str,
    profile_id: &str,
) -> Result<Option<aria_core::BrowserSessionStateRecord>, OrchestratorError> {
    let mut states = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_browser_session_states(None, None)
        .map_err(OrchestratorError::ToolError)?;
    states.retain(|state| state.agent_id == agent_id && state.profile_id == profile_id);
    states.sort_by_key(|state| std::cmp::Reverse(state.updated_at_us));
    Ok(states.into_iter().next())
}

fn browser_profile_by_id(
    sessions_dir: &Path,
    profile_id: &str,
) -> Result<aria_core::BrowserProfile, OrchestratorError> {
    RuntimeStore::for_sessions_dir(sessions_dir)
        .list_browser_profiles()
        .map_err(OrchestratorError::ToolError)?
        .into_iter()
        .find(|profile| profile.profile_id == profile_id)
        .ok_or_else(|| {
            OrchestratorError::ToolError(format!(
                "browser profile '{}' does not exist",
                profile_id
            ))
        })
}

fn browser_transport_kind_for_profile(
    profile: &aria_core::BrowserProfile,
) -> aria_core::BrowserTransportKind {
    match profile.mode {
        aria_core::BrowserProfileMode::Ephemeral
        | aria_core::BrowserProfileMode::ManagedPersistent => {
            aria_core::BrowserTransportKind::ManagedBrowser
        }
        aria_core::BrowserProfileMode::AttachedExternal => {
            aria_core::BrowserTransportKind::AttachedBrowser
        }
        aria_core::BrowserProfileMode::ExtensionBound => {
            aria_core::BrowserTransportKind::ExtensionBrowser
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct BrowserTransportLaunch {
    pid: Option<u32>,
    launch_command: Vec<String>,
    metadata: serde_json::Value,
}

#[async_trait::async_trait]
trait BrowserTransport: Send + Sync {
    fn kind(&self) -> aria_core::BrowserTransportKind;

    async fn start_session(
        &self,
        profile: &aria_core::BrowserProfile,
        profile_dir: &Path,
        start_url: Option<&str>,
    ) -> Result<BrowserTransportLaunch, OrchestratorError>;

    async fn screenshot(
        &self,
        browser_session: &aria_core::BrowserSessionRecord,
        url: &str,
        output_path: &Path,
    ) -> Result<serde_json::Value, OrchestratorError>;

    async fn run_action(
        &self,
        browser_session: &aria_core::BrowserSessionRecord,
        request: &aria_core::BrowserActionRequest,
    ) -> Result<serde_json::Value, OrchestratorError>;

    async fn persist_state(
        &self,
        browser_session: &aria_core::BrowserSessionRecord,
    ) -> Result<serde_json::Value, OrchestratorError>;

    async fn restore_state(
        &self,
        browser_session: &aria_core::BrowserSessionRecord,
        storage_state: serde_json::Value,
    ) -> Result<serde_json::Value, OrchestratorError>;

    async fn fill_credentials(
        &self,
        browser_session: &aria_core::BrowserSessionRecord,
        domain: &str,
        credentials: serde_json::Value,
        secrets_to_redact: &[String],
    ) -> Result<serde_json::Value, OrchestratorError>;
}

struct ManagedBrowserTransport;

struct BridgeOnlyBrowserTransport {
    kind: aria_core::BrowserTransportKind,
}

impl BridgeOnlyBrowserTransport {
    fn unsupported_start_message(&self) -> String {
        match self.kind {
            aria_core::BrowserTransportKind::AttachedBrowser => {
                "attached browser transport requires a dedicated attached-browser runtime; managed launch is intentionally not reused".into()
            }
            aria_core::BrowserTransportKind::ExtensionBrowser => {
                "extension browser transport requires an extension-bound runtime; managed launch is intentionally not reused".into()
            }
            aria_core::BrowserTransportKind::RemoteBrowser => {
                "remote browser transport requires a dedicated remote-browser runtime; managed launch is intentionally not reused".into()
            }
            aria_core::BrowserTransportKind::ManagedBrowser => {
                "managed browser transport should not use bridge-only transport".into()
            }
        }
    }
}

#[async_trait::async_trait]
impl BrowserTransport for ManagedBrowserTransport {
    fn kind(&self) -> aria_core::BrowserTransportKind {
        aria_core::BrowserTransportKind::ManagedBrowser
    }

    async fn start_session(
        &self,
        profile: &aria_core::BrowserProfile,
        profile_dir: &Path,
        start_url: Option<&str>,
    ) -> Result<BrowserTransportLaunch, OrchestratorError> {
        let launch_command = build_browser_launch_command(profile.engine, profile_dir, start_url);
        let pid = spawn_browser_launch_command(&launch_command)?;
        Ok(BrowserTransportLaunch {
            pid,
            launch_command: launch_command.clone(),
            metadata: serde_json::json!({
                "transport": self.kind(),
                "launch_command": launch_command,
                "pid": pid,
                "profile_dir": profile_dir,
                "url": start_url,
            }),
        })
    }

    async fn screenshot(
        &self,
        browser_session: &aria_core::BrowserSessionRecord,
        url: &str,
        output_path: &Path,
    ) -> Result<serde_json::Value, OrchestratorError> {
        let command = build_browser_screenshot_command(
            browser_session.engine,
            Path::new(&browser_session.profile_dir),
            url,
            output_path,
        )?;
        run_blocking_browser_task("browser screenshot command", {
            let command = command.clone();
            let output_path = output_path.to_path_buf();
            move || {
                run_browser_command_until_path_exists(
                    &command,
                    &output_path,
                    std::time::Duration::from_secs(30),
                )
            }
        })
        .await?;
        Ok(serde_json::json!({
            "transport": self.kind(),
            "command": command,
        }))
    }

    async fn run_action(
        &self,
        browser_session: &aria_core::BrowserSessionRecord,
        request: &aria_core::BrowserActionRequest,
    ) -> Result<serde_json::Value, OrchestratorError> {
        let (command, writable_dirs) = build_browser_automation_command(browser_session, request)?;
        let bridge_payload = run_browser_json_command_async(command, writable_dirs).await?;
        Ok(serde_json::json!({
            "transport": self.kind(),
            "bridge": bridge_payload,
        }))
    }

    async fn persist_state(
        &self,
        browser_session: &aria_core::BrowserSessionRecord,
    ) -> Result<serde_json::Value, OrchestratorError> {
        let bridge_payload = serde_json::json!({
            "browser_session_id": browser_session.browser_session_id,
            "profile_id": browser_session.profile_id,
            "profile_dir": browser_session.profile_dir,
            "engine": browser_session.engine,
            "start_url": browser_session.start_url,
            "action": "export_storage_state",
        });
        let bridge_writable_dirs =
            dedupe_normalized_paths([PathBuf::from(&browser_session.profile_dir)]);
        let (bridge_command, bridge_writable_dirs) =
            build_browser_automation_stdin_command(&bridge_writable_dirs, "persist_state")?;
        run_browser_json_command_with_input_async(
            bridge_command,
            bridge_writable_dirs,
            Some(bridge_payload),
        )
        .await
    }

    async fn restore_state(
        &self,
        browser_session: &aria_core::BrowserSessionRecord,
        storage_state: serde_json::Value,
    ) -> Result<serde_json::Value, OrchestratorError> {
        let bridge_payload = serde_json::json!({
            "browser_session_id": browser_session.browser_session_id,
            "profile_id": browser_session.profile_id,
            "profile_dir": browser_session.profile_dir,
            "engine": browser_session.engine,
            "start_url": browser_session.start_url,
            "action": "import_storage_state",
            "storage_state": storage_state,
        });
        let bridge_writable_dirs =
            dedupe_normalized_paths([PathBuf::from(&browser_session.profile_dir)]);
        let (bridge_command, bridge_writable_dirs) =
            build_browser_automation_stdin_command(&bridge_writable_dirs, "restore_state")?;
        run_browser_json_command_with_input_async(
            bridge_command,
            bridge_writable_dirs,
            Some(bridge_payload),
        )
        .await
    }

    async fn fill_credentials(
        &self,
        browser_session: &aria_core::BrowserSessionRecord,
        domain: &str,
        credentials: serde_json::Value,
        secrets_to_redact: &[String],
    ) -> Result<serde_json::Value, OrchestratorError> {
        let bridge_payload = serde_json::json!({
            "browser_session_id": browser_session.browser_session_id,
            "profile_id": browser_session.profile_id,
            "profile_dir": browser_session.profile_dir,
            "engine": browser_session.engine,
            "start_url": browser_session.start_url,
            "action": "fill_credentials",
            "domain": domain,
            "credentials": credentials,
        });
        let bridge_writable_dirs =
            dedupe_normalized_paths([PathBuf::from(&browser_session.profile_dir)]);
        let (bridge_command, bridge_writable_dirs) =
            build_browser_automation_stdin_command(&bridge_writable_dirs, "fill_credentials")?;
        Ok(redact_known_secrets_from_json(
            run_browser_json_command_with_input(
                &bridge_command,
                &bridge_writable_dirs,
                Some(&bridge_payload),
            )?,
            secrets_to_redact,
        ))
    }
}

#[async_trait::async_trait]
impl BrowserTransport for BridgeOnlyBrowserTransport {
    fn kind(&self) -> aria_core::BrowserTransportKind {
        self.kind
    }

    async fn start_session(
        &self,
        _profile: &aria_core::BrowserProfile,
        _profile_dir: &Path,
        _start_url: Option<&str>,
    ) -> Result<BrowserTransportLaunch, OrchestratorError> {
        Err(OrchestratorError::ToolError(self.unsupported_start_message()))
    }

    async fn screenshot(
        &self,
        _browser_session: &aria_core::BrowserSessionRecord,
        _url: &str,
        _output_path: &Path,
    ) -> Result<serde_json::Value, OrchestratorError> {
        Err(OrchestratorError::ToolError(format!(
            "{:?} transport does not support native screenshot capture yet",
            self.kind
        )))
    }

    async fn run_action(
        &self,
        browser_session: &aria_core::BrowserSessionRecord,
        request: &aria_core::BrowserActionRequest,
    ) -> Result<serde_json::Value, OrchestratorError> {
        ManagedBrowserTransport
            .run_action(browser_session, request)
            .await
            .map(|payload| {
                serde_json::json!({
                    "transport": self.kind(),
                    "bridge": payload,
                })
            })
    }

    async fn persist_state(
        &self,
        browser_session: &aria_core::BrowserSessionRecord,
    ) -> Result<serde_json::Value, OrchestratorError> {
        ManagedBrowserTransport.persist_state(browser_session).await
    }

    async fn restore_state(
        &self,
        browser_session: &aria_core::BrowserSessionRecord,
        storage_state: serde_json::Value,
    ) -> Result<serde_json::Value, OrchestratorError> {
        ManagedBrowserTransport
            .restore_state(browser_session, storage_state)
            .await
    }

    async fn fill_credentials(
        &self,
        browser_session: &aria_core::BrowserSessionRecord,
        domain: &str,
        credentials: serde_json::Value,
        secrets_to_redact: &[String],
    ) -> Result<serde_json::Value, OrchestratorError> {
        ManagedBrowserTransport
            .fill_credentials(browser_session, domain, credentials, secrets_to_redact)
            .await
    }
}

fn browser_transport_for_profile(
    profile: &aria_core::BrowserProfile,
) -> Arc<dyn BrowserTransport> {
    match browser_transport_kind_for_profile(profile) {
        aria_core::BrowserTransportKind::ManagedBrowser => Arc::new(ManagedBrowserTransport),
        aria_core::BrowserTransportKind::AttachedBrowser => {
            Arc::new(BridgeOnlyBrowserTransport {
                kind: aria_core::BrowserTransportKind::AttachedBrowser,
            })
        }
        aria_core::BrowserTransportKind::ExtensionBrowser => {
            Arc::new(BridgeOnlyBrowserTransport {
                kind: aria_core::BrowserTransportKind::ExtensionBrowser,
            })
        }
        aria_core::BrowserTransportKind::RemoteBrowser => {
            Arc::new(BridgeOnlyBrowserTransport {
                kind: aria_core::BrowserTransportKind::RemoteBrowser,
            })
        }
    }
}

fn browser_transport_for_session(
    browser_session: &aria_core::BrowserSessionRecord,
) -> Arc<dyn BrowserTransport> {
    match browser_session.transport {
        aria_core::BrowserTransportKind::ManagedBrowser => Arc::new(ManagedBrowserTransport),
        aria_core::BrowserTransportKind::AttachedBrowser => {
            Arc::new(BridgeOnlyBrowserTransport {
                kind: aria_core::BrowserTransportKind::AttachedBrowser,
            })
        }
        aria_core::BrowserTransportKind::ExtensionBrowser => {
            Arc::new(BridgeOnlyBrowserTransport {
                kind: aria_core::BrowserTransportKind::ExtensionBrowser,
            })
        }
        aria_core::BrowserTransportKind::RemoteBrowser => {
            Arc::new(BridgeOnlyBrowserTransport {
                kind: aria_core::BrowserTransportKind::RemoteBrowser,
            })
        }
    }
}

fn resolve_browser_login_domain(input: &str) -> Result<String, OrchestratorError> {
    let trimmed = input.trim();
    if trimmed.contains("://") {
        let parsed = reqwest::Url::parse(trimmed).map_err(|e| {
            OrchestratorError::ToolError(format!("Invalid domain or URL '{}': {}", input, e))
        })?;
        return url_host_key(&parsed);
    }
    normalize_domain_value(trimmed)
}

fn private_network_override_enabled() -> bool {
    runtime_env().allow_private_web_targets
}

fn profile_allows_private_network_targets(profile: Option<&AgentCapabilityProfile>) -> bool {
    private_network_override_enabled()
        || profile
            .map(|profile| {
                profile.side_effect_level == aria_core::SideEffectLevel::Privileged
                    || matches!(
                        profile.trust_profile,
                        Some(
                            aria_core::TrustProfile::TrustedLocal
                                | aria_core::TrustProfile::TrustedWorkspace
                        )
                    )
            })
            .unwrap_or(false)
}

fn ipv4_is_non_public(ip: &std::net::Ipv4Addr) -> bool {
    let octets = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        || octets[0] == 0
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 198 && octets[1] == 18)
        || (octets[0] == 198 && octets[1] == 19)
}

fn ipv6_is_non_public(ip: &std::net::Ipv6Addr) -> bool {
    let segments = ip.segments();
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || ip.is_multicast()
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
}

fn ip_is_non_public(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ip) => ipv4_is_non_public(&ip),
        std::net::IpAddr::V6(ip) => ipv6_is_non_public(&ip),
    }
}

fn validate_web_url_target_syntactic(
    url: &str,
    allow_private: bool,
) -> Result<reqwest::Url, OrchestratorError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| OrchestratorError::ToolError(format!("Invalid URL '{}': {}", url, e)))?;
    match parsed.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(OrchestratorError::ToolError(format!(
                "URL scheme '{}' is not allowed for web access",
                scheme
            )));
        }
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| OrchestratorError::ToolError(format!("URL '{}' is missing a host", url)))?;
    let host_lower = host.to_ascii_lowercase();
    if !allow_private {
        if matches!(host_lower.as_str(), "localhost" | "localhost.localdomain")
            || host_lower.ends_with(".local")
            || host_lower.ends_with(".internal")
        {
            return Err(OrchestratorError::ToolError(format!(
                "URL '{}' targets a local or internal hostname, which is blocked by default",
                url
            )));
        }
        if let Ok(ip) = host_lower.parse::<std::net::IpAddr>() {
            if ip_is_non_public(ip) {
                return Err(OrchestratorError::ToolError(format!(
                    "URL '{}' targets a private or non-public IP address, which is blocked by default",
                    url
                )));
            }
        }
    }
    Ok(parsed)
}

async fn validate_web_url_target_runtime(
    url: &str,
    allow_private: bool,
) -> Result<reqwest::Url, OrchestratorError> {
    let parsed = validate_web_url_target_syntactic(url, allow_private)?;
    if allow_private {
        return Ok(parsed);
    }
    let host = parsed.host_str().ok_or_else(|| {
        OrchestratorError::ToolError(format!("URL '{}' is missing a runtime host", url))
    })?;
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Ok(parsed);
    }
    let port = parsed.port_or_known_default().unwrap_or(80);
    let lookup = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| OrchestratorError::ToolError(format!("Failed to resolve host '{}': {}", host, e)))?;
    for addr in lookup {
        if ip_is_non_public(addr.ip()) {
            return Err(OrchestratorError::ToolError(format!(
                "URL '{}' resolved to a private or non-public IP address, which is blocked by default",
                url
            )));
        }
    }
    Ok(parsed)
}

fn latest_browser_login_state_for_profile(
    sessions_dir: &Path,
    agent_id: &str,
    profile_id: &str,
    domain: &str,
) -> Result<Option<aria_core::BrowserLoginStateRecord>, OrchestratorError> {
    let mut states = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_browser_login_states(None, Some(agent_id), Some(domain))
        .map_err(OrchestratorError::ToolError)?;
    states.retain(|state| state.profile_id == profile_id);
    states.sort_by_key(|state| std::cmp::Reverse(state.updated_at_us));
    Ok(states.into_iter().next())
}

fn browser_binary_env_var(engine: aria_core::BrowserEngine) -> &'static str {
    match engine {
        aria_core::BrowserEngine::Chromium => "ARIA_BROWSER_CHROMIUM_BIN",
        aria_core::BrowserEngine::Chrome => "ARIA_BROWSER_CHROME_BIN",
        aria_core::BrowserEngine::Edge => "ARIA_BROWSER_EDGE_BIN",
        aria_core::BrowserEngine::SafariBridge => "ARIA_BROWSER_SAFARI_BIN",
    }
}

fn browser_default_app_name(engine: aria_core::BrowserEngine) -> &'static str {
    match engine {
        aria_core::BrowserEngine::Chromium => "Chromium",
        aria_core::BrowserEngine::Chrome => "Google Chrome",
        aria_core::BrowserEngine::Edge => "Microsoft Edge",
        aria_core::BrowserEngine::SafariBridge => "Safari",
    }
}

fn browser_default_binary_candidates(engine: aria_core::BrowserEngine) -> &'static [&'static str] {
    match engine {
        #[cfg(target_os = "macos")]
        aria_core::BrowserEngine::Chromium => &[
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Chromium Dev.app/Contents/MacOS/Chromium Dev",
        ],
        #[cfg(target_os = "macos")]
        aria_core::BrowserEngine::Chrome => &[
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Google Chrome Beta.app/Contents/MacOS/Google Chrome Beta",
        ],
        #[cfg(target_os = "macos")]
        aria_core::BrowserEngine::Edge => &[
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        ],
        #[cfg(target_os = "macos")]
        aria_core::BrowserEngine::SafariBridge => &[],
        #[cfg(target_os = "linux")]
        aria_core::BrowserEngine::Chromium => &["chromium", "chromium-browser"],
        #[cfg(target_os = "linux")]
        aria_core::BrowserEngine::Chrome => &["google-chrome", "google-chrome-stable"],
        #[cfg(target_os = "linux")]
        aria_core::BrowserEngine::Edge => &["microsoft-edge", "microsoft-edge-stable"],
        #[cfg(target_os = "linux")]
        aria_core::BrowserEngine::SafariBridge => &[],
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        _ => &[],
    }
}

fn configured_browser_binary(engine: aria_core::BrowserEngine) -> Option<String> {
    match engine {
        aria_core::BrowserEngine::Chromium => runtime_env().browser_chromium_bin.clone(),
        aria_core::BrowserEngine::Chrome => runtime_env().browser_chrome_bin.clone(),
        aria_core::BrowserEngine::Edge => runtime_env().browser_edge_bin.clone(),
        aria_core::BrowserEngine::SafariBridge => runtime_env().browser_safari_bin.clone(),
    }
    .filter(|value| !value.trim().is_empty())
}

fn browser_binary_available_exact(engine: aria_core::BrowserEngine) -> bool {
    if configured_browser_binary(engine).is_some() {
        return true;
    }
    let candidates = browser_default_binary_candidates(engine);
    #[cfg(target_os = "macos")]
    {
        return candidates
            .iter()
            .any(|candidate| std::path::Path::new(candidate).exists());
    }
    #[cfg(target_os = "linux")]
    {
        return candidates.iter().any(|candidate| {
            std::process::Command::new("sh")
                .arg("-lc")
                .arg(format!("command -v {} >/dev/null 2>&1", candidate))
                .status()
                .map(|status| status.success())
                .unwrap_or(false)
        });
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

fn fallback_browser_engine(engine: aria_core::BrowserEngine) -> Option<aria_core::BrowserEngine> {
    match engine {
        aria_core::BrowserEngine::Chromium => Some(aria_core::BrowserEngine::Chrome),
        _ => None,
    }
}

fn resolve_browser_binary(engine: aria_core::BrowserEngine) -> Result<String, OrchestratorError> {
    if let Some(explicit_bin) = configured_browser_binary(engine) {
        return Ok(explicit_bin);
    }
    let candidates = browser_default_binary_candidates(engine);
    #[cfg(target_os = "macos")]
    for candidate in candidates {
        if std::path::Path::new(candidate).exists() {
            return Ok((*candidate).into());
        }
    }
    #[cfg(target_os = "linux")]
    for candidate in candidates {
        if std::process::Command::new("sh")
            .arg("-lc")
            .arg(format!("command -v {} >/dev/null 2>&1", candidate))
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
        {
            return Ok((*candidate).into());
        }
    }
    if let Some(fallback) = fallback_browser_engine(engine) {
        if let Ok(binary) = resolve_browser_binary(fallback) {
            return Ok(binary);
        }
    }
    Err(OrchestratorError::ToolError(format!(
        "No browser binary available for {:?}; set {}",
        engine,
        browser_binary_env_var(engine)
    )))
}

fn build_browser_launch_command(
    engine: aria_core::BrowserEngine,
    profile_dir: &Path,
    start_url: Option<&str>,
) -> Vec<String> {
    let profile_arg = format!("--user-data-dir={}", profile_dir.display());
    let start_url = start_url.unwrap_or("about:blank").to_string();
    if let Some(explicit_bin) = configured_browser_binary(engine) {
        return vec![explicit_bin, profile_arg, start_url];
    }
    let effective_engine = fallback_browser_engine(engine)
        .filter(|fallback| {
            !browser_binary_available_exact(engine) && browser_binary_available_exact(*fallback)
        })
        .unwrap_or(engine);

    #[cfg(target_os = "macos")]
    {
        return vec![
            "open".into(),
            "-na".into(),
            browser_default_app_name(effective_engine).into(),
            "--args".into(),
            profile_arg,
            start_url,
        ];
    }

    #[cfg(target_os = "linux")]
    {
        let binary = match effective_engine {
            aria_core::BrowserEngine::Chromium => "chromium",
            aria_core::BrowserEngine::Chrome => "google-chrome",
            aria_core::BrowserEngine::Edge => "microsoft-edge",
            aria_core::BrowserEngine::SafariBridge => "safari",
        };
        return vec![binary.into(), profile_arg, start_url];
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        vec![
            browser_default_app_name(engine).into(),
            profile_arg,
            start_url,
        ]
    }
}

fn build_browser_screenshot_command(
    engine: aria_core::BrowserEngine,
    profile_dir: &Path,
    target_url: &str,
    output_path: &Path,
) -> Result<Vec<String>, OrchestratorError> {
    if matches!(engine, aria_core::BrowserEngine::SafariBridge) {
        return Err(OrchestratorError::ToolError(
            "browser_screenshot is not supported for safari_bridge yet".into(),
        ));
    }
    let binary = resolve_browser_binary(engine)?;
    Ok(vec![
        binary,
        format!("--user-data-dir={}", profile_dir.display()),
        "--headless=new".into(),
        "--disable-gpu".into(),
        "--hide-scrollbars".into(),
        "--run-all-compositor-stages-before-draw".into(),
        format!("--screenshot={}", output_path.display()),
        "--window-size=1440,1024".into(),
        target_url.to_string(),
    ])
}

fn spawn_browser_launch_command(command: &[String]) -> Result<Option<u32>, OrchestratorError> {
    let (program, args) = command.split_first().ok_or_else(|| {
        OrchestratorError::ToolError("browser launch command is empty".into())
    })?;
    let child = std::process::Command::new(program)
        .args(args)
        .spawn()
        .map_err(|e| OrchestratorError::ToolError(format!("Failed to launch browser: {}", e)))?;
    Ok(Some(child.id()))
}

fn run_browser_command(command: &[String]) -> Result<(), OrchestratorError> {
    let (program, args) = command.split_first().ok_or_else(|| {
        OrchestratorError::ToolError("browser command is empty".into())
    })?;
    let status = std::process::Command::new(program)
        .args(args)
        .status()
        .map_err(|e| OrchestratorError::ToolError(format!("Failed to run browser command: {}", e)))?;
    if !status.success() {
        return Err(OrchestratorError::ToolError(format!(
            "browser command failed with status {}",
            status
        )));
    }
    Ok(())
}

fn run_browser_command_until_path_exists(
    command: &[String],
    expected_path: &Path,
    timeout: std::time::Duration,
) -> Result<(), OrchestratorError> {
    let (program, args) = command.split_first().ok_or_else(|| {
        OrchestratorError::ToolError("browser command is empty".into())
    })?;
    let mut child = std::process::Command::new(program)
        .args(args)
        .spawn()
        .map_err(|e| OrchestratorError::ToolError(format!("Failed to run browser command: {}", e)))?;
    let started = std::time::Instant::now();
    loop {
        if expected_path.exists() {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(());
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|e| OrchestratorError::ToolError(format!("Failed to poll browser command: {}", e)))?
        {
            if status.success() {
                if expected_path.exists() {
                    return Ok(());
                }
                return Err(OrchestratorError::ToolError(format!(
                    "browser command exited successfully but no artifact was written to '{}'",
                    expected_path.display()
                )));
            }
            return Err(OrchestratorError::ToolError(format!(
                "browser command failed with status {}",
                status
            )));
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(OrchestratorError::ToolError(format!(
                "browser command timed out after {}s",
                timeout.as_secs()
            )));
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

async fn run_blocking_browser_task<T, F>(
    label: &'static str,
    task: F,
) -> Result<T, OrchestratorError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, OrchestratorError> + Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .map_err(|e| OrchestratorError::ToolError(format!("{label} join failure: {e}")))?
}

async fn create_dir_all_async(path: PathBuf, label: &'static str) -> Result<(), OrchestratorError> {
    run_blocking_browser_task(label, move || {
        std::fs::create_dir_all(&path).map_err(|e| {
            OrchestratorError::ToolError(format!(
                "Failed to prepare directory '{}': {}",
                path.display(),
                e
            ))
        })
    })
    .await
}

async fn write_bytes_async(
    path: PathBuf,
    bytes: Vec<u8>,
    label: &'static str,
) -> Result<(), OrchestratorError> {
    run_blocking_browser_task(label, move || {
        std::fs::write(&path, bytes).map_err(|e| {
            OrchestratorError::ToolError(format!("Failed to persist '{}': {}", path.display(), e))
        })
    })
    .await
}

async fn read_bytes_async(path: PathBuf, label: &'static str) -> Result<Vec<u8>, OrchestratorError> {
    run_blocking_browser_task(label, move || {
        std::fs::read(&path).map_err(|e| {
            OrchestratorError::ToolError(format!("Failed to read '{}': {}", path.display(), e))
        })
    })
    .await
}

async fn file_size_async(path: PathBuf, label: &'static str) -> Result<u64, OrchestratorError> {
    run_blocking_browser_task(label, move || {
        std::fs::metadata(&path)
            .map(|meta| meta.len())
            .map_err(|e| {
                OrchestratorError::ToolError(format!(
                    "Failed to inspect file '{}': {}",
                    path.display(),
                    e
                ))
            })
    })
    .await
}

async fn run_browser_json_command_async(
    command: Vec<String>,
    writable_dirs: Vec<PathBuf>,
) -> Result<serde_json::Value, OrchestratorError> {
    run_blocking_browser_task("browser json command", move || {
        run_browser_json_command(&command, &writable_dirs)
    })
    .await
}

async fn run_browser_json_command_with_input_async(
    command: Vec<String>,
    writable_dirs: Vec<PathBuf>,
    stdin_payload: Option<serde_json::Value>,
) -> Result<serde_json::Value, OrchestratorError> {
    run_blocking_browser_task("browser json command", move || {
        run_browser_json_command_with_input(&command, &writable_dirs, stdin_payload.as_ref())
    })
    .await
}

async fn run_artifact_scan_async(
    path: PathBuf,
    kind: aria_core::BrowserArtifactKind,
    mime_type: String,
) -> Result<(), OrchestratorError> {
    run_blocking_browser_task("artifact scan", move || run_artifact_scan(&path, kind, &mime_type))
        .await
}

fn browser_action_kind_for_interaction(
    action: aria_core::BrowserInteractionKind,
) -> aria_core::BrowserActionKind {
    match action {
        aria_core::BrowserInteractionKind::Navigate => aria_core::BrowserActionKind::Navigate,
        aria_core::BrowserInteractionKind::Wait => aria_core::BrowserActionKind::Wait,
        aria_core::BrowserInteractionKind::Click => aria_core::BrowserActionKind::Click,
        aria_core::BrowserInteractionKind::Type => aria_core::BrowserActionKind::Type,
        aria_core::BrowserInteractionKind::Select => aria_core::BrowserActionKind::Select,
        aria_core::BrowserInteractionKind::Scroll => aria_core::BrowserActionKind::Scroll,
    }
}

fn sha256_file_hex(path: &Path) -> Result<String, OrchestratorError> {
    let bytes = std::fs::read(path).map_err(|e| {
        OrchestratorError::ToolError(format!(
            "Failed to read browser automation bridge '{}': {}",
            path.display(),
            e
        ))
    })?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

fn read_browser_automation_bridge_manifest(
    binary: &str,
) -> Result<BrowserAutomationBridgeManifest, OrchestratorError> {
    let output = run_browser_bridge_process(binary, &[String::from("--bridge-meta")], &[])
        .map_err(|e| {
            OrchestratorError::ToolError(format!(
                "Failed to query browser automation bridge metadata: {}",
                e
            ))
        })?;
    if !output.status.success() {
        return Err(OrchestratorError::ToolError(format!(
            "browser automation bridge metadata command failed with status {}",
            output.status
        )));
    }
    serde_json::from_slice::<BrowserAutomationBridgeManifest>(&output.stdout).map_err(|e| {
        OrchestratorError::ToolError(format!(
            "browser automation bridge metadata was not valid JSON: {}",
            e
        ))
    })
}

fn validate_browser_automation_bridge_mode(
    manifest: &BrowserAutomationBridgeManifest,
    mode: &str,
) -> Result<(), OrchestratorError> {
    if manifest.protocol_version != REQUIRED_BROWSER_AUTOMATION_PROTOCOL_VERSION {
        return Err(OrchestratorError::ToolError(format!(
            "browser automation bridge protocol_version={} is incompatible; required={}",
            manifest.protocol_version, REQUIRED_BROWSER_AUTOMATION_PROTOCOL_VERSION
        )));
    }
    if !manifest.supported_modes.iter().any(|value| value == mode) {
        return Err(OrchestratorError::ToolError(format!(
            "browser automation bridge does not support required mode '{}'",
            mode
        )));
    }
    Ok(())
}

fn validate_browser_automation_bridge_command(
    manifest: &BrowserAutomationBridgeManifest,
    command: &str,
) -> Result<(), OrchestratorError> {
    if !manifest
        .supported_commands
        .iter()
        .any(|value| value == command)
    {
        return Err(OrchestratorError::ToolError(format!(
            "browser automation bridge does not support required command '{}'",
            command
        )));
    }
    Ok(())
}

fn resolve_trusted_browser_automation_bridge_for_mode(
    mode: &str,
) -> Result<BrowserAutomationBridgeInfo, OrchestratorError> {
    let binary = runtime_env()
        .browser_automation_bin
        .clone()
        .ok_or_else(|| {
        OrchestratorError::ToolError(
            "browser automation bridge is not configured; set HIVECLAW_BROWSER_AUTOMATION_BIN (or legacy ARIA_BROWSER_AUTOMATION_BIN)".into(),
        )
    })?;
    let path = PathBuf::from(&binary);
    if !path.is_absolute() {
        return Err(OrchestratorError::ToolError(
            "HIVECLAW_BROWSER_AUTOMATION_BIN (or legacy ARIA_BROWSER_AUTOMATION_BIN) must be an absolute path".into(),
        ));
    }
    let metadata = std::fs::metadata(&path).map_err(|e| {
        OrchestratorError::ToolError(format!(
            "browser automation bridge '{}' is unavailable: {}",
            path.display(),
            e
        ))
    })?;
    if !metadata.is_file() {
        return Err(OrchestratorError::ToolError(format!(
            "browser automation bridge '{}' must point to a regular file",
            path.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(OrchestratorError::ToolError(format!(
                "browser automation bridge '{}' is not executable",
                path.display()
            )));
        }
    }
    let expected = runtime_env().browser_automation_sha256_allowlist.clone();
    if expected.is_empty() {
        return Err(OrchestratorError::ToolError(
            "HIVECLAW_BROWSER_AUTOMATION_SHA256_ALLOWLIST (or legacy ARIA_BROWSER_AUTOMATION_SHA256_ALLOWLIST) must not be empty".into(),
        ));
    }
    let actual = sha256_file_hex(&path)?;
    if !expected.iter().any(|checksum| checksum == &actual) {
        return Err(OrchestratorError::ToolError(format!(
            "browser automation bridge '{}' is not in the trusted checksum allowlist",
            path.display()
        )));
    }
    let manifest = read_browser_automation_bridge_manifest(&binary)?;
    validate_browser_automation_bridge_mode(&manifest, mode)?;
    Ok(BrowserAutomationBridgeInfo {
        binary,
        sha256_hex: actual,
        manifest,
        os_containment_requested: browser_bridge_containment_requested(),
        os_containment_available: browser_bridge_containment_available(),
        containment_backend: if browser_bridge_containment_available() {
            Some(browser_bridge_containment_backend_name().to_string())
        } else {
            None
        },
    })
}

fn browser_bridge_minimal_env() -> Vec<(String, String)> {
    let mut env = vec![
        ("LANG".to_string(), "C.UTF-8".to_string()),
        ("LC_ALL".to_string(), "C.UTF-8".to_string()),
    ];
    for key in ["PATH", "HOME", "TMPDIR", "TMP", "TEMP"] {
        if let Some(value) = non_empty_env(key) {
            env.push((key.to_string(), value));
        }
    }
    env
}

fn build_browser_bridge_command(
    program: &str,
    args: &[String],
    writable_dirs: &[PathBuf],
) -> Result<std::process::Command, OrchestratorError> {
    let mut cmd = if browser_bridge_containment_requested() {
        build_os_contained_process_command(program, args, writable_dirs)?
    } else {
        let mut cmd = std::process::Command::new(program);
        cmd.args(args);
        cmd
    };
    cmd.env_clear();
    for (key, value) in browser_bridge_minimal_env() {
        cmd.env(key, value);
    }
    let cwd = writable_dirs
        .iter()
        .find(|path| path.exists() && path.is_dir())
        .cloned()
        .unwrap_or_else(std::env::temp_dir);
    cmd.current_dir(cwd);
    Ok(cmd)
}

fn run_browser_bridge_process(
    program: &str,
    args: &[String],
    writable_dirs: &[PathBuf],
) -> Result<std::process::Output, std::io::Error> {
    build_browser_bridge_command(program, args, writable_dirs)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
}

fn build_browser_automation_command(
    browser_session: &aria_core::BrowserSessionRecord,
    request: &aria_core::BrowserActionRequest,
) -> Result<(Vec<String>, Vec<PathBuf>), OrchestratorError> {
    let bridge = resolve_trusted_browser_automation_bridge_for_mode("argv_json")?;
    validate_browser_automation_bridge_command(&bridge.manifest, "browser_action")?;
    let payload = serde_json::json!({
        "browser_session_id": browser_session.browser_session_id,
        "profile_id": browser_session.profile_id,
        "profile_dir": browser_session.profile_dir,
        "engine": browser_session.engine,
        "start_url": browser_session.start_url,
        "action": request.action,
        "url": request.url,
        "selector": request.selector,
        "text": request.text,
        "value": request.value,
        "millis": request.millis,
    });
    Ok((
        vec![bridge.binary, payload.to_string()],
        dedupe_normalized_paths([
            PathBuf::from(&browser_session.profile_dir),
            std::env::temp_dir(),
        ]),
    ))
}

fn build_browser_automation_stdin_command(
    writable_dirs: &[PathBuf],
    required_command: &str,
) -> Result<(Vec<String>, Vec<PathBuf>), OrchestratorError> {
    let bridge = resolve_trusted_browser_automation_bridge_for_mode("stdin_json")?;
    validate_browser_automation_bridge_command(&bridge.manifest, required_command)?;
    Ok((
        vec![bridge.binary],
        dedupe_normalized_paths(
            writable_dirs
                .iter()
                .cloned()
                .chain(std::iter::once(std::env::temp_dir())),
        ),
    ))
}

fn run_browser_json_command(
    command: &[String],
    writable_dirs: &[PathBuf],
) -> Result<serde_json::Value, OrchestratorError> {
    run_browser_json_command_with_input(command, writable_dirs, None)
}

fn run_browser_json_command_with_input(
    command: &[String],
    writable_dirs: &[PathBuf],
    stdin_payload: Option<&serde_json::Value>,
) -> Result<serde_json::Value, OrchestratorError> {
    let (program, args) = command.split_first().ok_or_else(|| {
        OrchestratorError::ToolError("browser automation command is empty".into())
    })?;
    let mut child = build_browser_bridge_command(program, args, writable_dirs)?
        .stdin(if stdin_payload.is_some() {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        })
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            OrchestratorError::ToolError(format!("Failed to run browser automation command: {}", e))
        })?;
    if let Some(payload) = stdin_payload {
        use std::io::Write;
        let mut stdin = child.stdin.take().ok_or_else(|| {
            OrchestratorError::ToolError("browser automation stdin was unavailable".into())
        })?;
        let bytes = serde_json::to_vec(payload).map_err(|e| {
            OrchestratorError::ToolError(format!(
                "Failed to serialize browser automation payload: {}",
                e
            ))
        })?;
        stdin.write_all(&bytes).map_err(|e| {
            OrchestratorError::ToolError(format!(
                "Failed to write browser automation stdin payload: {}",
                e
            ))
        })?;
    }
    let output = child.wait_with_output().map_err(|e| {
        OrchestratorError::ToolError(format!("Failed to collect browser automation output: {}", e))
    })?;
    if !output.status.success() {
        return Err(OrchestratorError::ToolError(format!(
            "browser automation command failed with status {}",
            output.status
        )));
    }
    if output.stdout.is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_slice::<serde_json::Value>(&output.stdout).map_err(|e| {
        OrchestratorError::ToolError(format!("browser automation output was not valid JSON: {}", e))
    })
}

fn redact_known_secrets_from_json(
    value: serde_json::Value,
    secrets: &[String],
) -> serde_json::Value {
    match value {
        serde_json::Value::String(text) => {
            let mut redacted = text;
            for secret in secrets {
                if !secret.is_empty() && redacted.contains(secret) {
                    redacted = redacted.replace(secret, "[REDACTED]");
                }
            }
            serde_json::Value::String(redacted)
        }
        serde_json::Value::Array(values) => serde_json::Value::Array(
            values
                .into_iter()
                .map(|value| redact_known_secrets_from_json(value, secrets))
                .collect(),
        ),
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, redact_known_secrets_from_json(value, secrets)))
                .collect(),
        ),
        other => other,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncryptedBrowserSessionState {
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
struct WebStoragePolicy {
    browser_artifact_max_count: usize,
    browser_artifact_max_bytes: u64,
    browser_session_state_max_count: usize,
    browser_session_state_max_bytes: u64,
    crawl_job_max_count: usize,
    watch_job_max_count: usize,
    website_memory_max_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct WebStorageUsage {
    browser_artifact_count: usize,
    browser_artifact_total_bytes: u64,
    browser_session_state_count: usize,
    browser_session_state_total_bytes: u64,
    crawl_job_count: usize,
    watch_job_count: usize,
    website_memory_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct WebRatePolicy {
    domain_min_interval_ms: u64,
    fetch_retry_attempts: u32,
    fetch_retry_base_delay_ms: u64,
    fetch_retry_max_delay_ms: u64,
    watch_max_jobs_per_agent: usize,
    watch_max_jobs_per_domain: usize,
}

#[derive(Debug, Clone, Serialize)]
struct ArtifactSafetyPolicy {
    download_max_bytes: u64,
    snapshot_max_bytes: u64,
    extract_max_bytes: u64,
    screenshot_max_bytes: u64,
    allowed_download_mime_prefixes: Vec<String>,
    blocked_download_extensions: Vec<String>,
    scan_bin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BrowserAutomationBridgeManifest {
    protocol_version: u32,
    #[serde(default)]
    bridge_version: Option<String>,
    #[serde(default)]
    supported_modes: Vec<String>,
    #[serde(default)]
    supported_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct HtmlExtractionResult {
    text: String,
    title: Option<String>,
    headings: Vec<String>,
    excerpt: Option<String>,
    profile: &'static str,
    site_adapter: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
struct BrowserAutomationBridgeInfo {
    binary: String,
    sha256_hex: String,
    manifest: BrowserAutomationBridgeManifest,
    os_containment_requested: bool,
    os_containment_available: bool,
    containment_backend: Option<String>,
}

const REQUIRED_BROWSER_AUTOMATION_PROTOCOL_VERSION: u32 = 1;

fn web_storage_policy() -> WebStoragePolicy {
    WebStoragePolicy {
        browser_artifact_max_count: runtime_env().browser_artifact_max_count,
        browser_artifact_max_bytes: runtime_env().browser_artifact_max_bytes,
        browser_session_state_max_count: runtime_env().browser_session_state_max_count,
        browser_session_state_max_bytes: runtime_env().browser_session_state_max_bytes,
        crawl_job_max_count: runtime_env().crawl_job_max_count,
        watch_job_max_count: runtime_env().watch_job_max_count,
        website_memory_max_count: runtime_env().website_memory_max_count,
    }
}

fn web_rate_policy() -> WebRatePolicy {
    WebRatePolicy {
        domain_min_interval_ms: runtime_env().web_domain_min_interval_ms,
        fetch_retry_attempts: runtime_env().web_fetch_retry_attempts,
        fetch_retry_base_delay_ms: runtime_env().web_fetch_retry_base_delay_ms,
        fetch_retry_max_delay_ms: runtime_env().web_fetch_retry_max_delay_ms,
        watch_max_jobs_per_agent: runtime_env().watch_max_jobs_per_agent,
        watch_max_jobs_per_domain: runtime_env().watch_max_jobs_per_domain,
    }
}

fn artifact_safety_policy() -> ArtifactSafetyPolicy {
    ArtifactSafetyPolicy {
        download_max_bytes: runtime_env().download_max_bytes,
        snapshot_max_bytes: runtime_env().snapshot_max_bytes,
        extract_max_bytes: runtime_env().extract_max_bytes,
        screenshot_max_bytes: runtime_env().screenshot_max_bytes,
        allowed_download_mime_prefixes: runtime_env().allowed_download_mime_prefixes.clone(),
        blocked_download_extensions: runtime_env().blocked_download_extensions.clone(),
        scan_bin: runtime_env().artifact_scan_bin.clone(),
    }
}

fn safe_file_size(path: &str) -> u64 {
    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

fn parse_retry_after_delay_ms(value: Option<&reqwest::header::HeaderValue>) -> Option<u64> {
    let header = value?.to_str().ok()?.trim();
    if let Ok(seconds) = header.parse::<u64>() {
        return Some(seconds.saturating_mul(1000));
    }
    let parsed = chrono::DateTime::parse_from_rfc2822(header).ok()?;
    let now = chrono::Utc::now();
    let target = parsed.with_timezone(&chrono::Utc);
    let delay = (target - now).num_milliseconds();
    if delay <= 0 {
        None
    } else {
        Some(delay as u64)
    }
}

async fn throttle_web_domain_request(domain: &str) {
    let policy = web_rate_policy();
    if policy.domain_min_interval_ms == 0 {
        return;
    }
    let quota = Quota::with_period(Duration::from_millis(policy.domain_min_interval_ms))
        .unwrap_or_else(|| Quota::per_second(NonZeroU32::new(1).expect("nonzero")));
    let limiter = app_runtime()
        .web_domain_rate_limiters
        .get_with_by_ref(domain, || Arc::new(governor::RateLimiter::direct(quota)));
    limiter.until_ready().await;
}

fn retryable_web_status(status: reqwest::StatusCode) -> bool {
    matches!(
        status,
        reqwest::StatusCode::TOO_MANY_REQUESTS
            | reqwest::StatusCode::BAD_GATEWAY
            | reqwest::StatusCode::SERVICE_UNAVAILABLE
            | reqwest::StatusCode::GATEWAY_TIMEOUT
    )
}

fn infer_download_mime_type(filename: &str, declared_mime_type: &str, bytes: &[u8]) -> String {
    infer::get(bytes)
        .map(|kind| kind.mime_type().to_string())
        .or_else(|| {
            infer::get_from_path(filename)
                .ok()
                .flatten()
                .map(|kind| kind.mime_type().to_string())
        })
        .unwrap_or_else(|| declared_mime_type.to_string())
}

fn validate_download_artifact_policy(
    filename: &str,
    declared_mime_type: &str,
    bytes: &[u8],
    byte_len: u64,
) -> Result<String, OrchestratorError> {
    let policy = artifact_safety_policy();
    if byte_len > policy.download_max_bytes {
        return Err(OrchestratorError::ToolError(format!(
            "download exceeds max allowed size: {} bytes > {} bytes",
            byte_len, policy.download_max_bytes
        )));
    }
    let mime_type = infer_download_mime_type(filename, declared_mime_type, bytes);
    let mime_lower = mime_type.to_ascii_lowercase();
    if !policy
        .allowed_download_mime_prefixes
        .iter()
        .any(|prefix| mime_lower.starts_with(prefix))
    {
        return Err(OrchestratorError::ToolError(format!(
            "download MIME type '{}' is not allowed by policy",
            mime_type
        )));
    }
    if let Some(extension) = Path::new(filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
    {
        if policy
            .blocked_download_extensions
            .iter()
            .any(|blocked| blocked == &extension)
        {
            return Err(OrchestratorError::ToolError(format!(
                "download file extension '.{}' is blocked by policy",
                extension
            )));
        }
    }
    Ok(mime_type)
}

fn validate_artifact_size_limit(
    kind: aria_core::BrowserArtifactKind,
    byte_len: u64,
) -> Result<(), OrchestratorError> {
    let policy = artifact_safety_policy();
    let (label, max_bytes) = match kind {
        aria_core::BrowserArtifactKind::Download => ("download", policy.download_max_bytes),
        aria_core::BrowserArtifactKind::DomSnapshot => ("snapshot", policy.snapshot_max_bytes),
        aria_core::BrowserArtifactKind::ExtractedText => ("extract", policy.extract_max_bytes),
        aria_core::BrowserArtifactKind::Screenshot => ("screenshot", policy.screenshot_max_bytes),
        aria_core::BrowserArtifactKind::LaunchMetadata => return Ok(()),
    };
    if byte_len > max_bytes {
        return Err(OrchestratorError::ToolError(format!(
            "{} artifact exceeds max allowed size: {} bytes > {} bytes",
            label, byte_len, max_bytes
        )));
    }
    Ok(())
}

fn run_artifact_scan(
    path: &Path,
    kind: aria_core::BrowserArtifactKind,
    mime_type: &str,
) -> Result<(), OrchestratorError> {
    let Some(scan_bin) = artifact_safety_policy().scan_bin else {
        return Ok(());
    };
    let scan_path = PathBuf::from(&scan_bin);
    if !scan_path.is_absolute() {
        return Err(OrchestratorError::ToolError(
            "HIVECLAW_ARTIFACT_SCAN_BIN (or legacy ARIA_ARTIFACT_SCAN_BIN) must be an absolute path".into(),
        ));
    }
    let output = std::process::Command::new(&scan_path)
        .arg(path)
        .arg(format!("{:?}", kind).to_ascii_lowercase())
        .arg(mime_type)
        .output()
        .map_err(|e| {
            OrchestratorError::ToolError(format!(
                "Failed to run artifact scan command '{}': {}",
                scan_path.display(),
                e
            ))
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(OrchestratorError::ToolError(format!(
            "artifact scan rejected '{}': {}",
            path.display(),
            stderr.trim()
        )));
    }
    Ok(())
}

fn compute_web_storage_usage(sessions_dir: &Path) -> Result<WebStorageUsage, OrchestratorError> {
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    let artifacts = store
        .list_browser_artifacts(None, None)
        .map_err(OrchestratorError::ToolError)?;
    let states = store
        .list_browser_session_states(None, None)
        .map_err(OrchestratorError::ToolError)?;
    let crawl_jobs = store.list_crawl_jobs().map_err(OrchestratorError::ToolError)?;
    let watch_jobs = store.list_watch_jobs().map_err(OrchestratorError::ToolError)?;
    let website_memory = store
        .list_website_memory(None)
        .map_err(OrchestratorError::ToolError)?;
    Ok(WebStorageUsage {
        browser_artifact_count: artifacts.len(),
        browser_artifact_total_bytes: artifacts
            .iter()
            .map(|artifact| safe_file_size(&artifact.storage_path))
            .sum(),
        browser_session_state_count: states.len(),
        browser_session_state_total_bytes: states
            .iter()
            .map(|state| safe_file_size(&state.storage_path))
            .sum(),
        crawl_job_count: crawl_jobs.len(),
        watch_job_count: watch_jobs.len(),
        website_memory_count: website_memory.len(),
    })
}

fn enforce_web_storage_policy(sessions_dir: &Path) -> Result<(), OrchestratorError> {
    let policy = web_storage_policy();
    let store = RuntimeStore::for_sessions_dir(sessions_dir);

    let mut artifacts = store
        .list_browser_artifacts(None, None)
        .map_err(OrchestratorError::ToolError)?;
    artifacts.sort_by_key(|artifact| artifact.created_at_us);
    let mut artifact_bytes: u64 = artifacts
        .iter()
        .map(|artifact| safe_file_size(&artifact.storage_path))
        .sum();
    while artifacts.len() > policy.browser_artifact_max_count
        || artifact_bytes > policy.browser_artifact_max_bytes
    {
        let artifact = artifacts.remove(0);
        let size = safe_file_size(&artifact.storage_path);
        let _ = std::fs::remove_file(&artifact.storage_path);
        store
            .delete_browser_artifact(&artifact.artifact_id)
            .map_err(OrchestratorError::ToolError)?;
        artifact_bytes = artifact_bytes.saturating_sub(size);
    }

    let mut states = store
        .list_browser_session_states(None, None)
        .map_err(OrchestratorError::ToolError)?;
    states.sort_by_key(|state| state.updated_at_us);
    let mut state_bytes: u64 = states
        .iter()
        .map(|state| safe_file_size(&state.storage_path))
        .sum();
    while states.len() > policy.browser_session_state_max_count
        || state_bytes > policy.browser_session_state_max_bytes
    {
        let state = states.remove(0);
        let size = safe_file_size(&state.storage_path);
        let _ = std::fs::remove_file(&state.storage_path);
        store
            .delete_browser_session_state(&state.state_id)
            .map_err(OrchestratorError::ToolError)?;
        state_bytes = state_bytes.saturating_sub(size);
    }

    let mut crawl_jobs = store.list_crawl_jobs().map_err(OrchestratorError::ToolError)?;
    crawl_jobs.sort_by_key(|job| job.updated_at_us);
    while crawl_jobs.len() > policy.crawl_job_max_count {
        let job = crawl_jobs.remove(0);
        store
            .delete_crawl_job(&job.crawl_id)
            .map_err(OrchestratorError::ToolError)?;
    }

    let mut watch_jobs = store.list_watch_jobs().map_err(OrchestratorError::ToolError)?;
    watch_jobs.sort_by_key(|job| job.updated_at_us);
    while watch_jobs.len() > policy.watch_job_max_count {
        let job = watch_jobs.remove(0);
        store
            .delete_watch_job(&job.watch_id)
            .map_err(OrchestratorError::ToolError)?;
    }

    let mut website_memory = store
        .list_website_memory(None)
        .map_err(OrchestratorError::ToolError)?;
    website_memory.sort_by_key(|record| record.updated_at_us);
    while website_memory.len() > policy.website_memory_max_count {
        let record = website_memory.remove(0);
        store
            .delete_website_memory(&record.record_id)
            .map_err(OrchestratorError::ToolError)?;
    }

    Ok(())
}

fn browser_state_encryption_key() -> Result<Key<Aes256Gcm>, OrchestratorError> {
    let master_key_raw = runtime_env().master_key.clone().ok_or_else(|| {
        OrchestratorError::ToolError(
            "HIVECLAW_MASTER_KEY or ARIA_MASTER_KEY must be set before persisting or restoring browser session state"
                .into(),
        )
    })?;
    if master_key_raw.trim().is_empty() {
        return Err(OrchestratorError::ToolError(
            "HIVECLAW_MASTER_KEY or ARIA_MASTER_KEY must not be empty when browser session state encryption is enabled"
                .into(),
        ));
    }
    let digest = Sha256::digest(master_key_raw.as_bytes());
    Ok(*Key::<Aes256Gcm>::from_slice(&digest))
}

fn encrypt_browser_session_state_payload(
    plaintext: &[u8],
) -> Result<EncryptedBrowserSessionState, OrchestratorError> {
    let cipher = Aes256Gcm::new(&browser_state_encryption_key()?);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher.encrypt(&nonce, plaintext).map_err(|e| {
        OrchestratorError::ToolError(format!(
            "Failed to encrypt browser session state payload: {}",
            e
        ))
    })?;
    Ok(EncryptedBrowserSessionState {
        nonce: nonce.to_vec(),
        ciphertext,
    })
}

fn decrypt_browser_session_state_payload(
    encrypted: &EncryptedBrowserSessionState,
) -> Result<Vec<u8>, OrchestratorError> {
    let cipher = Aes256Gcm::new(&browser_state_encryption_key()?);
    let nonce = Nonce::from_slice(&encrypted.nonce);
    cipher.decrypt(nonce, encrypted.ciphertext.as_ref()).map_err(|e| {
        OrchestratorError::ToolError(format!(
            "Failed to decrypt browser session state payload: {}",
            e
        ))
    })
}

fn browser_session_artifacts_root(sessions_dir: &Path, browser_session_id: &str) -> PathBuf {
    sessions_dir.join("browser_artifacts").join(browser_session_id)
}

fn browser_session_state_root(sessions_dir: &Path, profile_id: &str) -> PathBuf {
    sessions_dir.join("browser_session_state").join(profile_id)
}


fn extract_browser_profile_target(call: &ToolCall) -> Result<Option<String>, OrchestratorError> {
    match call.name.as_str() {
        "browser_profile_use" | "browser_open" | "browser_session_start" => {
            if call.name == "browser_profile_use" {
                let request: BrowserProfileUseRequest = decode_tool_args(call)?;
                Ok(Some(required_trimmed(&request.profile_id, "profile_id")?))
            } else if call.name == "browser_open" {
                let request: BrowserOpenRequest = decode_tool_args(call)?;
                Ok(request
                    .profile_id
                    .as_deref()
                    .map(|value| required_trimmed(value, "profile_id"))
                    .transpose()?)
            } else {
                let request: BrowserSessionStartRequest = decode_tool_args(call)?;
                Ok(request
                    .profile_id
                    .as_deref()
                    .map(|value| required_trimmed(value, "profile_id"))
                    .transpose()?)
            }
        }
        _ => Ok(None),
    }
}

fn extract_browser_action_request(
    call: &ToolCall,
) -> Result<Option<aria_core::BrowserActionRequest>, OrchestratorError> {
    if call.name != "browser_act" {
        return Ok(None);
    }
    let mut payload: serde_json::Value = decode_tool_args(call)?;
    if let Some(action_obj) = payload.get("action").and_then(|value| value.as_object()).cloned() {
        if let Some(kind) = action_obj
            .get("kind")
            .and_then(|value| value.as_str())
            .or_else(|| action_obj.get("action").and_then(|value| value.as_str()))
        {
            payload["action"] = serde_json::Value::String(kind.to_string());
        }
        for key in ["selector", "text", "value", "url", "millis"] {
            if payload.get(key).is_none() && action_obj.get(key).is_some() {
                payload[key] = action_obj
                    .get(key)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
            }
        }
    }
    let request = serde_json::from_value::<aria_core::BrowserActionRequest>(payload).map_err(|err| {
        OrchestratorError::ToolError(format!("Invalid args for '{}': {}", call.name, err))
    })?;
    Ok(Some(request))
}

fn validate_browser_profile_request(
    capability_profile: Option<&AgentCapabilityProfile>,
    call: &ToolCall,
    sessions_dir: Option<&Path>,
    session_id: Option<aria_core::Uuid>,
) -> Result<(), OrchestratorError> {
    let Some(profile_id) = extract_browser_profile_target(call)? else {
        return Ok(());
    };
    let Some(profile) = capability_profile else {
        return Ok(());
    };
    if !profile.browser_profile_allowlist.is_empty()
        && !profile
            .browser_profile_allowlist
            .iter()
            .any(|allowed| allowed == &profile_id)
    {
        append_scope_denial_record(
            sessions_dir,
            &profile.agent_id,
            session_id,
            ScopeDenialKind::BrowserProfileScope,
            profile_id.clone(),
            format!(
                "browser profile '{}' not permitted for agent '{}'",
                profile_id, profile.agent_id
            ),
        );
        return Err(OrchestratorError::ToolError(format!(
            "browser profile '{}' not permitted for agent '{}'",
            profile_id, profile.agent_id
        )));
    }
    if let Some(sessions_dir) = sessions_dir {
        let browser_profile = browser_profile_by_id(sessions_dir, &profile_id)?;
        let transport = browser_transport_kind_for_profile(&browser_profile);
        if !profile.web_transport_allowlist.is_empty()
            && !profile.web_transport_allowlist.contains(&transport)
        {
            append_scope_denial_record(
                Some(sessions_dir),
                &profile.agent_id,
                session_id,
                ScopeDenialKind::BrowserProfileScope,
                profile_id.clone(),
                format!(
                    "browser transport '{:?}' for profile '{}' not permitted for agent '{}'",
                    transport, profile_id, profile.agent_id
                ),
            );
            return Err(OrchestratorError::ToolError(format!(
                "browser transport '{:?}' for profile '{}' not permitted for agent '{}'",
                transport, profile_id, profile.agent_id
            )));
        }
    }
    Ok(())
}

fn validate_browser_action_request(
    capability_profile: Option<&AgentCapabilityProfile>,
    call: &ToolCall,
    sessions_dir: Option<&Path>,
    session_id: Option<aria_core::Uuid>,
) -> Result<(), OrchestratorError> {
    let Some(request) = extract_browser_action_request(call)? else {
        return Ok(());
    };
    let Some(profile) = capability_profile else {
        return Ok(());
    };
    let allowed = match (profile.browser_action_scope, request.action) {
        (Some(aria_core::BrowserActionScope::DiscoverOnly), _) => false,
        (Some(aria_core::BrowserActionScope::ReadOnly), aria_core::BrowserInteractionKind::Wait)
        | (Some(aria_core::BrowserActionScope::ReadOnly), aria_core::BrowserInteractionKind::Navigate) => true,
        (
            Some(aria_core::BrowserActionScope::InteractiveNonAuth),
            aria_core::BrowserInteractionKind::Wait
                | aria_core::BrowserInteractionKind::Navigate
                | aria_core::BrowserInteractionKind::Scroll
                | aria_core::BrowserInteractionKind::Click,
        ) => true,
        (Some(aria_core::BrowserActionScope::InteractiveAuth), _) => true,
        (Some(aria_core::BrowserActionScope::Download), aria_core::BrowserInteractionKind::Wait)
        | (Some(aria_core::BrowserActionScope::Download), aria_core::BrowserInteractionKind::Navigate) => true,
        (Some(aria_core::BrowserActionScope::SubmitWrite), _) => true,
        (None, _) => true,
        _ => false,
    };
    if !allowed {
        append_scope_denial_record(
            sessions_dir,
            &profile.agent_id,
            session_id,
            ScopeDenialKind::BrowserActionScope,
            format!("{:?}", request.action),
            format!(
                "browser action '{:?}' not permitted for agent '{}'",
                request.action, profile.agent_id
            ),
        );
        return Err(OrchestratorError::ToolError(format!(
            "browser action '{:?}' not permitted for agent '{}'",
            request.action, profile.agent_id
        )));
    }
    if matches!(
        request.action,
        aria_core::BrowserInteractionKind::Click
            | aria_core::BrowserInteractionKind::Type
            | aria_core::BrowserInteractionKind::Select
    ) {
        return Err(aria_intelligence::approval_required_error(&call.name));
    }
    Ok(())
}
