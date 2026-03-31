use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerRuntimeKind {
    LocalDesktop,
    ManagedVm,
    RemoteDesktop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerSurfaceKind {
    BrowserRuntime,
    ChromeDevtoolsMcp,
    ComputerRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerActionKind {
    CaptureScreenshot,
    PointerMove,
    PointerClick,
    KeyboardType,
    KeyPress,
    ClipboardRead,
    ClipboardWrite,
    WindowFocus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerPointerButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerActionScope {
    ObserveOnly,
    PointerOnly,
    PointerAndKeyboard,
    ClipboardRead,
    ClipboardReadWrite,
    FullDesktopControl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerArtifactKind {
    Screenshot,
    WindowSnapshot,
    ClipboardSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputerExecutionProfile {
    pub profile_id: String,
    pub display_name: String,
    pub runtime_kind: ComputerRuntimeKind,
    #[serde(default)]
    pub isolated: bool,
    #[serde(default)]
    pub headless: bool,
    #[serde(default)]
    pub allow_clipboard: bool,
    #[serde(default)]
    pub allow_keyboard: bool,
    #[serde(default)]
    pub allow_pointer: bool,
    #[serde(default)]
    pub allowed_windows: Vec<String>,
    pub created_at_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputerSessionRecord {
    pub computer_session_id: String,
    pub session_id: Uuid,
    pub agent_id: String,
    pub profile_id: String,
    pub runtime_kind: ComputerRuntimeKind,
    #[serde(default)]
    pub selected_window_id: Option<String>,
    pub created_at_us: u64,
    pub updated_at_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputerActionRequest {
    #[serde(default)]
    pub computer_session_id: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub target_window_id: Option<String>,
    pub action: ComputerActionKind,
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
    #[serde(default)]
    pub button: Option<ComputerPointerButton>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputerArtifactRecord {
    pub artifact_id: String,
    pub session_id: Uuid,
    pub agent_id: String,
    #[serde(default)]
    pub computer_session_id: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
    pub kind: ComputerArtifactKind,
    pub mime_type: String,
    pub storage_path: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
    pub created_at_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputerActionAuditRecord {
    pub audit_id: String,
    pub session_id: Uuid,
    pub agent_id: String,
    #[serde(default)]
    pub computer_session_id: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
    pub action: ComputerActionKind,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
    pub created_at_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceSelectionDecision {
    pub surface: ComputerSurfaceKind,
    pub reason: String,
}
