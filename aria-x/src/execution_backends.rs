use aria_core::{
    ExecutionBackendConfig, ExecutionBackendKind, ExecutionBackendKnownHostsPolicy,
    ExecutionBackendProfile, ExecutionBackendSshConfig, ExecutionBackendTrustLevel,
    ExecutionBackendVmConfig,
};

const DEFAULT_DOCKER_IMAGE: &str = "alpine:3.20";

pub(crate) fn default_execution_backend_profiles() -> Vec<ExecutionBackendProfile> {
    let mut profiles = vec![
        ExecutionBackendProfile {
            backend_id: "local-default".into(),
            display_name: "Local Default".into(),
            kind: ExecutionBackendKind::Local,
            config: None,
            is_default: true,
            requires_approval: false,
            supports_workspace_mount: true,
            supports_browser: true,
            supports_desktop: true,
            supports_artifact_return: true,
            supports_network_egress: true,
            trust_level: ExecutionBackendTrustLevel::LocalTrusted,
        },
        ExecutionBackendProfile {
            backend_id: "docker-sandbox".into(),
            display_name: format!("Docker Sandbox ({})", DEFAULT_DOCKER_IMAGE),
            kind: ExecutionBackendKind::Docker,
            config: Some(ExecutionBackendConfig::Docker {
                default_image: Some(DEFAULT_DOCKER_IMAGE.into()),
            }),
            is_default: false,
            requires_approval: true,
            supports_workspace_mount: true,
            supports_browser: false,
            supports_desktop: false,
            supports_artifact_return: true,
            supports_network_egress: false,
            trust_level: ExecutionBackendTrustLevel::IsolatedSandbox,
        },
        ExecutionBackendProfile {
            backend_id: "vm-guarded".into(),
            display_name: "Managed VM Guarded".into(),
            kind: ExecutionBackendKind::ManagedVm,
            config: Some(ExecutionBackendConfig::ManagedVm(ExecutionBackendVmConfig {
                provider: "unconfigured".into(),
                profile_name: Some("guarded-default".into()),
                supports_desktop_stream: true,
            })),
            is_default: false,
            requires_approval: true,
            supports_workspace_mount: true,
            supports_browser: true,
            supports_desktop: true,
            supports_artifact_return: true,
            supports_network_egress: false,
            trust_level: ExecutionBackendTrustLevel::IsolatedSandbox,
        },
    ];
    if let Some(ssh_profile) = env_ssh_backend_profile() {
        profiles.push(ssh_profile);
    }
    profiles
}

pub(crate) fn ensure_default_execution_backend_profiles(
    sessions_dir: &std::path::Path,
) -> Result<Vec<ExecutionBackendProfile>, String> {
    let store = crate::runtime_store::RuntimeStore::for_sessions_dir(sessions_dir);
    let existing = store.list_execution_backend_profiles()?;
    let now_us = chrono::Utc::now().timestamp_micros() as u64;
    let defaults = default_execution_backend_profiles();
    let existing_ids = existing
        .iter()
        .map(|profile| profile.backend_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    for profile in &defaults {
        if !existing_ids.contains(profile.backend_id.as_str()) {
            store.upsert_execution_backend_profile(profile, now_us)?;
        }
    }
    let mut merged = store.list_execution_backend_profiles()?;
    merged.sort_by(|lhs, rhs| lhs.backend_id.cmp(&rhs.backend_id));
    Ok(merged)
}

pub(crate) fn default_docker_image() -> &'static str {
    DEFAULT_DOCKER_IMAGE
}

fn env_ssh_backend_profile() -> Option<ExecutionBackendProfile> {
    let host = env_alias("HIVECLAW_SSH_HOST", "ARIA_SSH_HOST")?;
    let user = env_alias("HIVECLAW_SSH_USER", "ARIA_SSH_USER");
    let identity_file = env_alias("HIVECLAW_SSH_IDENTITY_FILE", "ARIA_SSH_IDENTITY_FILE");
    let remote_workspace_root =
        env_alias("HIVECLAW_SSH_WORKSPACE_ROOT", "ARIA_SSH_WORKSPACE_ROOT");
    let port = env_alias("HIVECLAW_SSH_PORT", "ARIA_SSH_PORT")
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(22);
    let known_hosts_policy = match env_alias(
        "HIVECLAW_SSH_KNOWN_HOSTS_POLICY",
        "ARIA_SSH_KNOWN_HOSTS_POLICY",
    )
    .as_deref()
    .map(|value| value.trim().to_ascii_lowercase())
    .as_deref()
    {
        Some("accept_new") | Some("accept-new") => {
            ExecutionBackendKnownHostsPolicy::AcceptNew
        }
        Some("insecure_ignore") | Some("insecure-ignore") | Some("no") => {
            ExecutionBackendKnownHostsPolicy::InsecureIgnore
        }
        _ => ExecutionBackendKnownHostsPolicy::Strict,
    };
    Some(ExecutionBackendProfile {
        backend_id: "ssh-remote".into(),
        display_name: format!("SSH Remote ({})", host),
        kind: ExecutionBackendKind::Ssh,
        config: Some(ExecutionBackendConfig::Ssh(ExecutionBackendSshConfig {
            host,
            port,
            user,
            identity_file,
            remote_workspace_root,
            known_hosts_policy,
        })),
        is_default: false,
        requires_approval: true,
        supports_workspace_mount: true,
        supports_browser: false,
        supports_desktop: false,
        supports_artifact_return: true,
        supports_network_egress: true,
        trust_level: ExecutionBackendTrustLevel::RemoteBounded,
    })
}

fn env_alias(primary: &str, legacy: &str) -> Option<String> {
    std::env::var(primary)
        .ok()
        .or_else(|| std::env::var(legacy).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod execution_backend_tests {
    use super::*;

    #[test]
    fn default_execution_backend_profiles_include_vm_boundary() {
        let profiles = default_execution_backend_profiles();
        assert!(profiles.iter().any(|profile| {
            profile.backend_id == "vm-guarded"
                && profile.kind == ExecutionBackendKind::ManagedVm
                && matches!(
                    profile.config,
                    Some(ExecutionBackendConfig::ManagedVm(_))
                )
        }));
    }

    #[test]
    fn env_ssh_backend_profile_is_seeded_from_hiveclaw_env() {
        unsafe {
            std::env::set_var("HIVECLAW_SSH_HOST", "edge-box");
            std::env::set_var("HIVECLAW_SSH_USER", "robot");
            std::env::set_var("HIVECLAW_SSH_PORT", "2202");
            std::env::set_var("HIVECLAW_SSH_WORKSPACE_ROOT", "/srv/hive");
            std::env::set_var("HIVECLAW_SSH_KNOWN_HOSTS_POLICY", "accept_new");
        }
        let profile = env_ssh_backend_profile().expect("ssh profile");
        assert_eq!(profile.backend_id, "ssh-remote");
        assert_eq!(profile.kind, ExecutionBackendKind::Ssh);
        match profile.config {
            Some(ExecutionBackendConfig::Ssh(config)) => {
                assert_eq!(config.host, "edge-box");
                assert_eq!(config.user.as_deref(), Some("robot"));
                assert_eq!(config.port, 2202);
                assert_eq!(config.remote_workspace_root.as_deref(), Some("/srv/hive"));
                assert_eq!(
                    config.known_hosts_policy,
                    ExecutionBackendKnownHostsPolicy::AcceptNew
                );
            }
            other => panic!("unexpected ssh config: {:?}", other),
        }
        unsafe {
            std::env::remove_var("HIVECLAW_SSH_HOST");
            std::env::remove_var("HIVECLAW_SSH_USER");
            std::env::remove_var("HIVECLAW_SSH_PORT");
            std::env::remove_var("HIVECLAW_SSH_WORKSPACE_ROOT");
            std::env::remove_var("HIVECLAW_SSH_KNOWN_HOSTS_POLICY");
        }
    }
}
