use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionBackendKind {
    Local,
    Docker,
    Ssh,
    ManagedVm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionBackendTrustLevel {
    LocalTrusted,
    IsolatedSandbox,
    RemoteBounded,
    RemotePrivileged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionBackendKnownHostsPolicy {
    Strict,
    AcceptNew,
    InsecureIgnore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionBackendSshConfig {
    pub host: String,
    #[serde(default = "default_execution_backend_ssh_port")]
    pub port: u16,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub identity_file: Option<String>,
    #[serde(default)]
    pub remote_workspace_root: Option<String>,
    #[serde(default = "default_execution_backend_known_hosts_policy")]
    pub known_hosts_policy: ExecutionBackendKnownHostsPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionBackendVmConfig {
    pub provider: String,
    #[serde(default)]
    pub profile_name: Option<String>,
    #[serde(default)]
    pub supports_desktop_stream: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionBackendConfig {
    Docker {
        #[serde(default)]
        default_image: Option<String>,
    },
    Ssh(ExecutionBackendSshConfig),
    ManagedVm(ExecutionBackendVmConfig),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionBackendProfile {
    pub backend_id: String,
    pub display_name: String,
    pub kind: ExecutionBackendKind,
    #[serde(default)]
    pub config: Option<ExecutionBackendConfig>,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub requires_approval: bool,
    #[serde(default)]
    pub supports_workspace_mount: bool,
    #[serde(default)]
    pub supports_browser: bool,
    #[serde(default)]
    pub supports_desktop: bool,
    #[serde(default)]
    pub supports_artifact_return: bool,
    #[serde(default)]
    pub supports_network_egress: bool,
    pub trust_level: ExecutionBackendTrustLevel,
}

fn default_execution_backend_ssh_port() -> u16 {
    22
}

fn default_execution_backend_known_hosts_policy() -> ExecutionBackendKnownHostsPolicy {
    ExecutionBackendKnownHostsPolicy::Strict
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionWorkerStatus {
    Online,
    Degraded,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RobotWorkerHealth {
    Healthy,
    Degraded,
    Faulted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RobotWorkerBinding {
    pub robot_id: String,
    #[serde(default)]
    pub ros2_profile_id: Option<String>,
    #[serde(default)]
    pub allowed_intents: Vec<RoboticsIntentKind>,
    pub policy_group: String,
    pub max_abs_velocity: f32,
    pub health: RobotWorkerHealth,
    #[serde(default)]
    pub health_notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionWorkerRecord {
    pub worker_id: String,
    pub display_name: String,
    pub node_id: String,
    pub backend_id: String,
    pub backend_kind: ExecutionBackendKind,
    #[serde(default)]
    pub supports_browser: bool,
    #[serde(default)]
    pub supports_desktop: bool,
    #[serde(default)]
    pub supports_gpu: bool,
    #[serde(default)]
    pub supports_robotics: bool,
    #[serde(default)]
    pub max_concurrency: u32,
    pub trust_level: ExecutionBackendTrustLevel,
    pub status: ExecutionWorkerStatus,
    pub last_heartbeat_us: u64,
    #[serde(default)]
    pub robot_binding: Option<RobotWorkerBinding>,
}
