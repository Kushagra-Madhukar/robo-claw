use async_trait::async_trait;
use std::io::Write;
use std::process::{Command, Stdio};

use aria_core::{
    ExecutionArtifactKind, ExecutionBackendConfig, ExecutionBackendKind,
    ExecutionBackendKnownHostsPolicy, ExecutionBackendProfile, ExecutionBackendTrustLevel,
    ExecutionContractKind, ExecutionWorkerRecord, ExecutionWorkerStatus, RobotWorkerHealth,
    RoboticsIntentKind,
};

use crate::{OrchestratorError, ToolCall, ToolExecutionResult, ToolExecutor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionBackendRequest {
    pub requested_backend_id: Option<String>,
    pub contract_kind: Option<ExecutionContractKind>,
    pub required_artifact_kinds: Vec<ExecutionArtifactKind>,
    pub needs_workspace_mount: bool,
    pub needs_browser: bool,
    pub needs_desktop: bool,
    pub requires_network_egress: bool,
}

impl Default for ExecutionBackendRequest {
    fn default() -> Self {
        Self {
            requested_backend_id: None,
            contract_kind: None,
            required_artifact_kinds: Vec::new(),
            needs_workspace_mount: false,
            needs_browser: false,
            needs_desktop: false,
            requires_network_egress: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRoutingRequest {
    pub backend_request: ExecutionBackendRequest,
    pub requires_gpu: bool,
    pub requires_robotics: bool,
    pub target_robot_id: Option<String>,
    pub required_robot_intent: Option<RoboticsIntentKind>,
    pub required_ros2_profile_id: Option<String>,
    pub minimum_trust_level: ExecutionBackendTrustLevel,
}

impl Default for WorkerRoutingRequest {
    fn default() -> Self {
        Self {
            backend_request: ExecutionBackendRequest::default(),
            requires_gpu: false,
            requires_robotics: false,
            target_robot_id: None,
            required_robot_intent: None,
            required_ros2_profile_id: None,
            minimum_trust_level: ExecutionBackendTrustLevel::RemotePrivileged,
        }
    }
}

#[async_trait]
pub trait ExecutionBackend: Send + Sync {
    fn profile(&self) -> &ExecutionBackendProfile;

    async fn execute_tool(&self, call: &ToolCall) -> Result<ToolExecutionResult, OrchestratorError>;
}

pub struct LocalExecutionBackend<T: ToolExecutor> {
    profile: ExecutionBackendProfile,
    executor: T,
}

impl<T: ToolExecutor> LocalExecutionBackend<T> {
    pub fn new(profile: ExecutionBackendProfile, executor: T) -> Self {
        Self { profile, executor }
    }
}

#[async_trait]
impl<T: ToolExecutor> ExecutionBackend for LocalExecutionBackend<T> {
    fn profile(&self) -> &ExecutionBackendProfile {
        &self.profile
    }

    async fn execute_tool(&self, call: &ToolCall) -> Result<ToolExecutionResult, OrchestratorError> {
        self.executor.execute(call).await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerExecutionBackendConfig {
    pub image: String,
    pub workspace_host_path: Option<String>,
    pub workspace_container_path: String,
    pub network_mode: Option<String>,
    pub env_allowlist: Vec<String>,
    pub entrypoint_args: Vec<String>,
}

impl DockerExecutionBackendConfig {
    pub fn build_command_args(&self) -> Vec<String> {
        let mut args = vec!["run".into(), "--rm".into(), "-i".into()];
        if let Some(network_mode) = self.network_mode.as_deref() {
            args.push("--network".into());
            args.push(network_mode.into());
        }
        if let Some(host_path) = self.workspace_host_path.as_deref() {
            args.push("-v".into());
            args.push(format!(
                "{}:{}:rw",
                host_path, self.workspace_container_path
            ));
            args.push("-w".into());
            args.push(self.workspace_container_path.clone());
        }
        for env_key in &self.env_allowlist {
            if let Ok(value) = std::env::var(env_key) {
                args.push("-e".into());
                args.push(format!("{}={}", env_key, value));
            }
        }
        args.push(self.image.clone());
        args.extend(self.entrypoint_args.clone());
        args
    }
}

pub struct DockerExecutionBackend {
    profile: ExecutionBackendProfile,
    config: DockerExecutionBackendConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshExecutionBackendConfig {
    pub host: String,
    pub port: u16,
    pub user: Option<String>,
    pub identity_file: Option<String>,
    pub remote_workspace_root: Option<String>,
    pub known_hosts_policy: ExecutionBackendKnownHostsPolicy,
}

impl SshExecutionBackendConfig {
    pub fn from_profile(profile: &ExecutionBackendProfile) -> Result<Self, OrchestratorError> {
        let Some(ExecutionBackendConfig::Ssh(config)) = profile.config.as_ref() else {
            return Err(OrchestratorError::ToolError(format!(
                "execution backend '{}' is missing ssh configuration",
                profile.backend_id
            )));
        };
        Ok(Self {
            host: config.host.clone(),
            port: config.port,
            user: config.user.clone(),
            identity_file: config.identity_file.clone(),
            remote_workspace_root: config.remote_workspace_root.clone(),
            known_hosts_policy: config.known_hosts_policy.clone(),
        })
    }

    pub fn build_command_args(&self, remote_command: &str) -> Vec<String> {
        let mut args = vec!["-p".into(), self.port.to_string()];
        match self.known_hosts_policy {
            ExecutionBackendKnownHostsPolicy::Strict => {}
            ExecutionBackendKnownHostsPolicy::AcceptNew => {
                args.push("-o".into());
                args.push("StrictHostKeyChecking=accept-new".into());
            }
            ExecutionBackendKnownHostsPolicy::InsecureIgnore => {
                args.push("-o".into());
                args.push("StrictHostKeyChecking=no".into());
                args.push("-o".into());
                args.push("UserKnownHostsFile=/dev/null".into());
            }
        }
        if let Some(identity_file) = self.identity_file.as_deref() {
            args.push("-i".into());
            args.push(identity_file.into());
        }
        let destination = match self.user.as_deref() {
            Some(user) => format!("{}@{}", user, self.host),
            None => self.host.clone(),
        };
        args.push(destination);
        args.push(remote_command.into());
        args
    }
}

pub struct SshExecutionBackend {
    profile: ExecutionBackendProfile,
    config: SshExecutionBackendConfig,
}

impl SshExecutionBackend {
    pub fn new(profile: ExecutionBackendProfile, config: SshExecutionBackendConfig) -> Self {
        Self { profile, config }
    }

    async fn run_remote_command(
        &self,
        call: &ToolCall,
    ) -> Result<ToolExecutionResult, OrchestratorError> {
        let payload = serde_json::to_string(call).map_err(|e| {
            OrchestratorError::ToolError(format!("serialize ssh tool call failed: {}", e))
        })?;
        let remote_command = format!("hiveclaw-exec '{}'", payload.replace('\'', "'\\''"));
        let output = Command::new("ssh")
            .args(self.config.build_command_args(&remote_command))
            .output()
            .map_err(|e| OrchestratorError::ToolError(format!("ssh backend failed: {}", e)))?;
        if !output.status.success() {
            return Err(OrchestratorError::ToolError(format!(
                "ssh backend exited with status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        serde_json::from_slice::<ToolExecutionResult>(&output.stdout).map_err(|e| {
            OrchestratorError::ToolError(format!("parse ssh backend result failed: {}", e))
        })
    }
}

#[async_trait]
impl ExecutionBackend for SshExecutionBackend {
    fn profile(&self) -> &ExecutionBackendProfile {
        &self.profile
    }

    async fn execute_tool(&self, call: &ToolCall) -> Result<ToolExecutionResult, OrchestratorError> {
        self.run_remote_command(call).await
    }
}

impl DockerExecutionBackend {
    pub fn new(profile: ExecutionBackendProfile, config: DockerExecutionBackendConfig) -> Self {
        Self { profile, config }
    }

    async fn run_container(&self, call: &ToolCall) -> Result<ToolExecutionResult, OrchestratorError> {
        let mut child = Command::new("docker")
            .args(self.config.build_command_args())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| OrchestratorError::ToolError(format!("spawn docker failed: {}", e)))?;

        if let Some(mut stdin) = child.stdin.take() {
            let payload = serde_json::to_vec(call).map_err(|e| {
                OrchestratorError::ToolError(format!("serialize docker tool call failed: {}", e))
            })?;
            stdin.write_all(&payload).map_err(|e| {
                OrchestratorError::ToolError(format!("write docker stdin failed: {}", e))
            })?;
        }

        let output = child.wait_with_output().map_err(|e| {
            OrchestratorError::ToolError(format!("wait for docker failed: {}", e))
        })?;
        if !output.status.success() {
            return Err(OrchestratorError::ToolError(format!(
                "docker backend exited with status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }

        serde_json::from_slice::<ToolExecutionResult>(&output.stdout).map_err(|e| {
            OrchestratorError::ToolError(format!(
                "parse docker backend result failed: {}",
                e
            ))
        })
    }
}

#[async_trait]
impl ExecutionBackend for DockerExecutionBackend {
    fn profile(&self) -> &ExecutionBackendProfile {
        &self.profile
    }

    async fn execute_tool(&self, call: &ToolCall) -> Result<ToolExecutionResult, OrchestratorError> {
        self.run_container(call).await
    }
}

pub fn select_execution_backend(
    request: &ExecutionBackendRequest,
    profiles: &[ExecutionBackendProfile],
) -> Result<ExecutionBackendProfile, String> {
    let mut eligible = profiles
        .iter()
        .filter(|profile| backend_profile_matches_request(profile, request))
        .collect::<Vec<_>>();

    if let Some(requested_id) = request.requested_backend_id.as_deref() {
        let profile = profiles
            .iter()
            .find(|profile| profile.backend_id == requested_id)
            .ok_or_else(|| format!("requested backend '{}' is not registered", requested_id))?;
        if !backend_profile_matches_request(profile, request) {
            return Err(format!(
                "requested backend '{}' does not satisfy execution requirements",
                requested_id
            ));
        }
        return Ok(profile.clone());
    }

    if eligible.is_empty() {
        return Err("no execution backend satisfies the requested capabilities".into());
    }

    eligible.sort_by_key(|profile| {
        (
            !profile.is_default,
            profile.kind != ExecutionBackendKind::Local,
            profile.backend_id.clone(),
        )
    });

    Ok((*eligible[0]).clone())
}

pub fn select_execution_worker(
    request: &WorkerRoutingRequest,
    workers: &[ExecutionWorkerRecord],
    profiles: &[ExecutionBackendProfile],
) -> Result<ExecutionWorkerRecord, String> {
    let selected_backend = select_execution_backend(&request.backend_request, profiles)?;
    let mut eligible = workers
        .iter()
        .filter(|worker| {
            worker.backend_id == selected_backend.backend_id
                && worker_status_is_routable(worker.status)
                && worker_matches_request(worker, request)
        })
        .collect::<Vec<_>>();

    if eligible.is_empty() {
        return Err(format!(
            "no execution worker satisfies backend '{}' and capability requirements",
            selected_backend.backend_id
        ));
    }

    eligible.sort_by_key(|worker| {
        (
            worker.status != ExecutionWorkerStatus::Online,
            usize::MAX - worker.max_concurrency as usize,
            worker.worker_id.clone(),
        )
    });

    Ok((*eligible[0]).clone())
}

fn backend_profile_matches_request(
    profile: &ExecutionBackendProfile,
    request: &ExecutionBackendRequest,
) -> bool {
    if request.needs_workspace_mount && !profile.supports_workspace_mount {
        return false;
    }
    if request.needs_browser && !profile.supports_browser {
        return false;
    }
    if request.needs_desktop && !profile.supports_desktop {
        return false;
    }
    if request.requires_network_egress && !profile.supports_network_egress {
        return false;
    }
    if request.required_artifact_kinds.is_empty() {
        return true;
    }
    profile.supports_artifact_return
}

fn worker_status_is_routable(status: ExecutionWorkerStatus) -> bool {
    matches!(status, ExecutionWorkerStatus::Online | ExecutionWorkerStatus::Degraded)
}

fn worker_matches_request(
    worker: &ExecutionWorkerRecord,
    request: &WorkerRoutingRequest,
) -> bool {
    if request.backend_request.needs_browser && !worker.supports_browser {
        return false;
    }
    if request.backend_request.needs_desktop && !worker.supports_desktop {
        return false;
    }
    if request.requires_gpu && !worker.supports_gpu {
        return false;
    }
    if request.requires_robotics {
        if !worker.supports_robotics {
            return false;
        }
        let Some(binding) = worker.robot_binding.as_ref() else {
            return false;
        };
        if let Some(robot_id) = request.target_robot_id.as_deref() {
            if binding.robot_id != robot_id {
                return false;
            }
        }
        if let Some(profile_id) = request.required_ros2_profile_id.as_deref() {
            if binding.ros2_profile_id.as_deref() != Some(profile_id) {
                return false;
            }
        }
        if let Some(intent) = request.required_robot_intent {
            if !binding.allowed_intents.is_empty() && !binding.allowed_intents.contains(&intent) {
                return false;
            }
            match (binding.health, intent) {
                (RobotWorkerHealth::Faulted, _) => return false,
                (RobotWorkerHealth::Degraded, RoboticsIntentKind::MoveActuator | RoboticsIntentKind::Halt) => {
                    return false;
                }
                _ => {}
            }
        } else if binding.health == RobotWorkerHealth::Faulted {
            return false;
        }
    }
    trust_level_rank(worker.trust_level) <= trust_level_rank(request.minimum_trust_level)
}

fn trust_level_rank(level: ExecutionBackendTrustLevel) -> u8 {
    match level {
        ExecutionBackendTrustLevel::LocalTrusted => 0,
        ExecutionBackendTrustLevel::IsolatedSandbox => 1,
        ExecutionBackendTrustLevel::RemoteBounded => 2,
        ExecutionBackendTrustLevel::RemotePrivileged => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct MockExecutor;

    #[async_trait]
    impl ToolExecutor for MockExecutor {
        async fn execute(&self, call: &ToolCall) -> Result<ToolExecutionResult, OrchestratorError> {
            Ok(ToolExecutionResult::text(format!("ran {}", call.name)))
        }
    }

    fn backend(
        backend_id: &str,
        kind: ExecutionBackendKind,
        is_default: bool,
    ) -> ExecutionBackendProfile {
        ExecutionBackendProfile {
            backend_id: backend_id.into(),
            display_name: backend_id.into(),
            kind,
            config: None,
            is_default,
            requires_approval: kind != ExecutionBackendKind::Local,
            supports_workspace_mount: true,
            supports_browser: matches!(kind, ExecutionBackendKind::Local | ExecutionBackendKind::ManagedVm),
            supports_desktop: matches!(kind, ExecutionBackendKind::Local | ExecutionBackendKind::ManagedVm),
            supports_artifact_return: true,
            supports_network_egress: kind != ExecutionBackendKind::ManagedVm,
            trust_level: match kind {
                ExecutionBackendKind::Local => ExecutionBackendTrustLevel::LocalTrusted,
                ExecutionBackendKind::Docker | ExecutionBackendKind::ManagedVm => {
                    ExecutionBackendTrustLevel::IsolatedSandbox
                }
                ExecutionBackendKind::Ssh => ExecutionBackendTrustLevel::RemoteBounded,
            },
        }
    }

    fn worker(
        worker_id: &str,
        backend_id: &str,
        backend_kind: ExecutionBackendKind,
        status: ExecutionWorkerStatus,
    ) -> ExecutionWorkerRecord {
        ExecutionWorkerRecord {
            worker_id: worker_id.into(),
            display_name: worker_id.into(),
            node_id: "node-a".into(),
            backend_id: backend_id.into(),
            backend_kind,
            supports_browser: matches!(backend_kind, ExecutionBackendKind::Local),
            supports_desktop: matches!(
                backend_kind,
                ExecutionBackendKind::Local | ExecutionBackendKind::ManagedVm
            ),
            supports_gpu: false,
            supports_robotics: false,
            max_concurrency: 2,
            trust_level: match backend_kind {
                ExecutionBackendKind::Local => ExecutionBackendTrustLevel::LocalTrusted,
                ExecutionBackendKind::Docker | ExecutionBackendKind::ManagedVm => {
                    ExecutionBackendTrustLevel::IsolatedSandbox
                }
                ExecutionBackendKind::Ssh => ExecutionBackendTrustLevel::RemoteBounded,
            },
            status,
            last_heartbeat_us: 1,
            robot_binding: None,
        }
    }

    #[test]
    fn backend_selection_defaults_to_local_when_available() {
        let selected = select_execution_backend(
            &ExecutionBackendRequest::default(),
            &[
                backend("docker-main", ExecutionBackendKind::Docker, false),
                backend("local-main", ExecutionBackendKind::Local, true),
            ],
        )
        .expect("select backend");
        assert_eq!(selected.backend_id, "local-main");
        assert_eq!(selected.kind, ExecutionBackendKind::Local);
    }

    #[test]
    fn backend_selection_can_require_desktop_capability() {
        let selected = select_execution_backend(
            &ExecutionBackendRequest {
                needs_desktop: true,
                ..ExecutionBackendRequest::default()
            },
            &[
                backend("docker-main", ExecutionBackendKind::Docker, false),
                backend("vm-desktop", ExecutionBackendKind::ManagedVm, false),
            ],
        )
        .expect("select backend");
        assert_eq!(selected.backend_id, "vm-desktop");
        assert_eq!(selected.kind, ExecutionBackendKind::ManagedVm);
    }

    #[test]
    fn backend_selection_rejects_ineligible_explicit_backend() {
        let err = select_execution_backend(
            &ExecutionBackendRequest {
                requested_backend_id: Some("docker-main".into()),
                needs_desktop: true,
                ..ExecutionBackendRequest::default()
            },
            &[backend("docker-main", ExecutionBackendKind::Docker, false)],
        )
        .expect_err("desktop request should reject docker backend");
        assert!(err.contains("does not satisfy"));
    }

    #[test]
    fn docker_backend_builds_bounded_command_args() {
        let cfg = DockerExecutionBackendConfig {
            image: "ghcr.io/hiveclaw/tool-runner:latest".into(),
            workspace_host_path: Some("/tmp/workspace".into()),
            workspace_container_path: "/workspace".into(),
            network_mode: Some("none".into()),
            env_allowlist: vec![],
            entrypoint_args: vec!["tool-runner".into()],
        };
        let args = cfg.build_command_args();
        assert_eq!(args[0], "run");
        assert!(args.contains(&"--rm".to_string()));
        assert!(args.contains(&"--network".to_string()));
        assert!(args.contains(&"none".to_string()));
        assert!(args.contains(&"-v".to_string()));
        assert!(args.contains(&"/tmp/workspace:/workspace:rw".to_string()));
        assert_eq!(
            args.last().expect("entrypoint arg"),
            "tool-runner"
        );
    }

    #[test]
    fn ssh_backend_builds_command_args_with_known_hosts_policy() {
        let cfg = SshExecutionBackendConfig {
            host: "edge-box".into(),
            port: 2202,
            user: Some("robot".into()),
            identity_file: Some("/tmp/id_ed25519".into()),
            remote_workspace_root: Some("/srv/hive".into()),
            known_hosts_policy: ExecutionBackendKnownHostsPolicy::AcceptNew,
        };
        let args = cfg.build_command_args("printf hive");
        assert_eq!(args[0], "-p");
        assert_eq!(args[1], "2202");
        assert!(args.contains(&"-i".to_string()));
        assert!(args.contains(&"/tmp/id_ed25519".to_string()));
        assert!(args.contains(&"StrictHostKeyChecking=accept-new".to_string()));
        assert!(args.contains(&"robot@edge-box".to_string()));
        assert_eq!(args.last().expect("remote command"), "printf hive");
    }

    #[test]
    fn ssh_backend_config_builds_destination_and_policy_flags() {
        let cfg = SshExecutionBackendConfig {
            host: "example.internal".into(),
            port: 2222,
            user: Some("builder".into()),
            identity_file: Some("/tmp/test_ed25519".into()),
            remote_workspace_root: Some("/srv/workspaces".into()),
            known_hosts_policy: ExecutionBackendKnownHostsPolicy::InsecureIgnore,
        };
        let args = cfg.build_command_args("echo hello");
        assert!(args.windows(2).any(|pair| pair == ["-p", "2222"]));
        assert!(args.windows(2).any(|pair| pair == ["-i", "/tmp/test_ed25519"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-o", "StrictHostKeyChecking=no"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-o", "UserKnownHostsFile=/dev/null"]));
        assert!(args.iter().any(|arg| arg == "builder@example.internal"));
        assert_eq!(args.last().expect("remote command"), "echo hello");
    }

    #[test]
    fn docker_backend_selection_prefers_local_by_default() {
        let selected = select_execution_backend(
            &ExecutionBackendRequest {
                needs_workspace_mount: true,
                ..ExecutionBackendRequest::default()
            },
            &[
                backend("docker-main", ExecutionBackendKind::Docker, false),
                backend("local-main", ExecutionBackendKind::Local, true),
            ],
        )
        .expect("select backend");
        assert_eq!(selected.backend_id, "local-main");
    }

    #[test]
    fn worker_routing_prefers_online_worker_with_matching_backend() {
        let selected = select_execution_worker(
            &WorkerRoutingRequest {
                backend_request: ExecutionBackendRequest {
                    requested_backend_id: Some("docker-main".into()),
                    ..ExecutionBackendRequest::default()
                },
                minimum_trust_level: ExecutionBackendTrustLevel::IsolatedSandbox,
                ..WorkerRoutingRequest::default()
            },
            &[
                worker(
                    "worker-offline",
                    "docker-main",
                    ExecutionBackendKind::Docker,
                    ExecutionWorkerStatus::Offline,
                ),
                worker(
                    "worker-online",
                    "docker-main",
                    ExecutionBackendKind::Docker,
                    ExecutionWorkerStatus::Online,
                ),
            ],
            &[backend("docker-main", ExecutionBackendKind::Docker, false)],
        )
        .expect("select worker");
        assert_eq!(selected.worker_id, "worker-online");
    }

    #[test]
    fn worker_routing_rejects_workers_without_required_capability() {
        let err = select_execution_worker(
            &WorkerRoutingRequest {
                backend_request: ExecutionBackendRequest {
                    requested_backend_id: Some("docker-main".into()),
                    needs_desktop: true,
                    ..ExecutionBackendRequest::default()
                },
                minimum_trust_level: ExecutionBackendTrustLevel::IsolatedSandbox,
                ..WorkerRoutingRequest::default()
            },
            &[worker(
                "worker-online",
                "docker-main",
                ExecutionBackendKind::Docker,
                ExecutionWorkerStatus::Online,
            )],
            &[backend("docker-main", ExecutionBackendKind::Docker, false)],
        )
        .expect_err("desktop task should reject docker worker");
        assert!(
            err.contains("no execution worker") || err.contains("does not satisfy"),
            "unexpected routing error: {}",
            err
        );
    }

    #[test]
    fn worker_routing_matches_bound_robot_worker_profile_and_intent() {
        let mut robot_worker = worker(
            "robot-worker",
            "ssh-robotics",
            ExecutionBackendKind::Ssh,
            ExecutionWorkerStatus::Online,
        );
        robot_worker.supports_robotics = true;
        robot_worker.robot_binding = Some(aria_core::RobotWorkerBinding {
            robot_id: "rover-7".into(),
            ros2_profile_id: Some("ros2-lab".into()),
            allowed_intents: vec![RoboticsIntentKind::InspectActuator, RoboticsIntentKind::ReportState],
            policy_group: "lab-safe".into(),
            max_abs_velocity: 0.1,
            health: RobotWorkerHealth::Healthy,
            health_notes: vec![],
        });

        let selected = select_execution_worker(
            &WorkerRoutingRequest {
                backend_request: ExecutionBackendRequest {
                    requested_backend_id: Some("ssh-robotics".into()),
                    ..ExecutionBackendRequest::default()
                },
                requires_robotics: true,
                target_robot_id: Some("rover-7".into()),
                required_robot_intent: Some(RoboticsIntentKind::ReportState),
                required_ros2_profile_id: Some("ros2-lab".into()),
                minimum_trust_level: ExecutionBackendTrustLevel::RemoteBounded,
                ..WorkerRoutingRequest::default()
            },
            &[robot_worker],
            &[backend("ssh-robotics", ExecutionBackendKind::Ssh, false)],
        )
        .expect("robot worker should route");
        assert_eq!(selected.worker_id, "robot-worker");
    }

    #[test]
    fn worker_routing_rejects_degraded_motion_robot_workers() {
        let mut robot_worker = worker(
            "robot-worker",
            "ssh-robotics",
            ExecutionBackendKind::Ssh,
            ExecutionWorkerStatus::Online,
        );
        robot_worker.supports_robotics = true;
        robot_worker.robot_binding = Some(aria_core::RobotWorkerBinding {
            robot_id: "rover-7".into(),
            ros2_profile_id: Some("ros2-lab".into()),
            allowed_intents: vec![RoboticsIntentKind::MoveActuator],
            policy_group: "lab-safe".into(),
            max_abs_velocity: 0.1,
            health: RobotWorkerHealth::Degraded,
            health_notes: vec!["motor temp elevated".into()],
        });

        let err = select_execution_worker(
            &WorkerRoutingRequest {
                backend_request: ExecutionBackendRequest {
                    requested_backend_id: Some("ssh-robotics".into()),
                    ..ExecutionBackendRequest::default()
                },
                requires_robotics: true,
                target_robot_id: Some("rover-7".into()),
                required_robot_intent: Some(RoboticsIntentKind::MoveActuator),
                required_ros2_profile_id: Some("ros2-lab".into()),
                minimum_trust_level: ExecutionBackendTrustLevel::RemoteBounded,
                ..WorkerRoutingRequest::default()
            },
            &[robot_worker],
            &[backend("ssh-robotics", ExecutionBackendKind::Ssh, false)],
        )
        .expect_err("degraded motion worker should be rejected");
        assert!(err.contains("no execution worker"));
    }

    #[tokio::test]
    async fn local_execution_backend_delegates_to_tool_executor() {
        let backend = LocalExecutionBackend::new(
            backend("local-main", ExecutionBackendKind::Local, true),
            MockExecutor,
        );
        let result = backend
            .execute_tool(&ToolCall {
                invocation_id: None,
                name: "write_file".into(),
                arguments: "{\"path\":\"notes.txt\"}".into(),
            })
            .await
            .expect("execute tool");
        assert!(result.contains("write_file"));
    }
}
