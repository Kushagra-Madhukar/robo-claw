use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

const PRIMARY_CLI_NAME: &str = "hiveclaw";
const LEGACY_CLI_NAME: &str = "aria-x";
const DEFAULT_BOOTSTRAP_POLICY: &str = include_str!("../../aria-policy/policies/default.cedar");

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

struct ShutdownCoordinator {
    stopping: AtomicBool,
    notify: tokio::sync::Notify,
}

impl ShutdownCoordinator {
    fn new() -> Self {
        Self {
            stopping: AtomicBool::new(false),
            notify: tokio::sync::Notify::new(),
        }
    }

    fn signal_shutdown(&self) {
        if !self.stopping.swap(true, AtomicOrdering::SeqCst) {
            self.notify.notify_waiters();
        }
    }

    fn is_stopping(&self) -> bool {
        self.stopping.load(AtomicOrdering::SeqCst)
    }

    async fn wait(&self) {
        if self.is_stopping() {
            return;
        }
        self.notify.notified().await;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimePidRecord {
    pid: u32,
    config_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InstallMode {
    Copy,
}

#[derive(Debug, Clone, Deserialize)]
struct GoldenReplaySuite {
    scenarios: Vec<GoldenReplayScenario>,
}

#[derive(Debug, Clone, Deserialize)]
struct GoldenReplayScenario {
    id: String,
    task_fingerprint: String,
    expected_outcome: aria_learning::TraceOutcome,
    #[serde(default = "default_golden_replay_min_samples")]
    min_samples: usize,
    #[serde(default)]
    required_tools: Vec<String>,
    #[serde(default)]
    response_must_contain: Vec<String>,
    #[serde(default)]
    min_reward_score: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GoldenReplayScenarioResult {
    id: String,
    task_fingerprint: String,
    sample_count: usize,
    latest_request_id: Option<String>,
    latest_outcome: Option<aria_learning::TraceOutcome>,
    latest_reward_score: Option<i32>,
    passed: bool,
    reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GoldenReplayReport {
    scenario_count: usize,
    passed_count: usize,
    failed_count: usize,
    results: Vec<GoldenReplayScenarioResult>,
}

#[derive(Debug, Clone)]
struct ContractRegressionScenario {
    id: &'static str,
    request_text: &'static str,
    expected_kind: aria_core::ExecutionContractKind,
    expected_required_artifacts: Vec<aria_core::ExecutionArtifactKind>,
    expected_required_tools: Vec<&'static str>,
    expected_approval_required: bool,
    expected_tool_choice: Option<&'static str>,
    satisfied_tool_names: Vec<&'static str>,
    expected_plain_text_failure: Option<aria_core::ContractFailureReason>,
    approval_probe: Option<ContractApprovalProbe>,
}

#[derive(Debug, Clone)]
struct ContractApprovalProbe {
    tool_name: &'static str,
    arguments_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContractRegressionScenarioResult {
    id: String,
    contract_kind: aria_core::ExecutionContractKind,
    passed: bool,
    reasons: Vec<String>,
    required_tools: Vec<String>,
    tool_choice: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContractRegressionReport {
    scenario_count: usize,
    passed_count: usize,
    failed_count: usize,
    results: Vec<ContractRegressionScenarioResult>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProviderBenchmarkSuite {
    scenarios: Vec<ProviderBenchmarkScenario>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProviderBenchmarkScenario {
    id: String,
    task_fingerprint: String,
    #[serde(default = "default_provider_benchmark_min_samples")]
    min_samples_per_provider: usize,
    #[serde(default)]
    required_providers: Vec<String>,
    #[serde(default)]
    require_fallback_visibility: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderBenchmarkProviderResult {
    provider_id: String,
    model_ref: String,
    sample_count: usize,
    success_count: usize,
    failure_count: usize,
    approval_required_count: usize,
    clarification_required_count: usize,
    average_latency_ms: f64,
    average_prompt_tokens: f64,
    fallback_outcomes: usize,
    repair_fallback_calls: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderBenchmarkScenarioResult {
    id: String,
    task_fingerprint: String,
    passed: bool,
    reasons: Vec<String>,
    providers: Vec<ProviderBenchmarkProviderResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderBenchmarkReport {
    scenario_count: usize,
    passed_count: usize,
    failed_count: usize,
    results: Vec<ProviderBenchmarkScenarioResult>,
}

fn default_golden_replay_min_samples() -> usize {
    1
}

fn default_provider_benchmark_min_samples() -> usize {
    1
}

fn render_cli_help(topic: Option<&str>) -> String {
    match topic.map(|value| value.trim().to_ascii_lowercase()) {
        Some(topic) if topic == "init" => [
            "hiveclaw init",
            "",
            "Usage:",
            "  hiveclaw init",
            "  hiveclaw init <path>",
            "  hiveclaw init <path> --preset <recommended|edge>",
            "  hiveclaw init <path> --default-agent <agent_id>",
            "  hiveclaw init <path> --non-interactive --overwrite",
            "",
            "Behavior:",
            "  Bootstraps a local HiveClaw project in the target directory.",
            "  Creates .hiveclaw/config.toml, local policies, workspace directories,",
            "  and a HIVECLAW.md project guidance file.",
        ]
        .join("\n"),
        Some(topic) if topic == "doctor" => [
            "hiveclaw doctor",
            "",
            "Usage:",
            "  hiveclaw doctor",
            "  hiveclaw doctor stt",
            "  hiveclaw doctor env",
            "  hiveclaw doctor gateway",
            "  hiveclaw doctor browser",
            "  hiveclaw doctor mcp [--live] [--mode <launch_managed|auto_connect>]",
            "",
            "Commands:",
            "  doctor         Show a runtime/operator health summary.",
            "  doctor stt     Show detailed speech-to-text availability and configuration.",
            "  doctor env     Show resolved local environment/runtime input status.",
            "  doctor gateway Show configured gateway/channel status.",
            "  doctor browser Show browser automation/runtime configuration status.",
            "  doctor mcp     Show MCP runtime readiness and Chrome DevTools MCP status.",
            "                   With --live, perform a real Chrome DevTools MCP handshake probe.",
            "                   Use --mode auto_connect to probe the active Chrome session.",
        ]
        .join("\n"),
        Some(topic) if topic == "install" => [
            "hiveclaw install",
            "",
            "Usage:",
            "  hiveclaw install",
            "  hiveclaw install --bin-dir <path>",
            "  hiveclaw install --with-default-config",
            "  hiveclaw install --with-default-config --overwrite-config",
            "",
            "Behavior:",
            "  Copies the current HiveClaw executable into a user-level bin directory.",
            "  The default target is ~/.local/bin/hiveclaw and also installs ~/.local/bin/aria-x for compatibility.",
            "  The command does not edit shell startup files automatically.",
            "  With --with-default-config it seeds the standard HiveClaw config path.",
        ]
        .join("\n"),
        Some(topic) if topic == "completion" => [
            "hiveclaw completion",
            "",
            "Usage:",
            "  hiveclaw completion bash",
            "  hiveclaw completion zsh",
            "  hiveclaw completion fish",
            "",
            "Prints shell completion scripts to stdout.",
        ]
        .join("\n"),
        Some(topic) if topic == "skills" => [
            "hiveclaw skills",
            "",
            "Usage:",
            "  hiveclaw skills list",
            "  hiveclaw skills install --dir <skill_dir>",
            "  hiveclaw skills install --codex-dir <skill_dir>",
            "  hiveclaw skills install --signed-dir <skill_dir> [--public-key <hex>]",
            "  hiveclaw skills install --manifest <skill.toml>",
            "  hiveclaw skills update --dir <skill_dir>",
            "  hiveclaw skills enable <skill_id>",
            "  hiveclaw skills disable <skill_id>",
            "  hiveclaw skills bind <skill_id> [--agent <agent_id>] [--policy <manual|auto_suggest|auto_load_low_risk|approval_required>] [--version <requirement>]",
            "  hiveclaw skills unbind <skill_id> [--agent <agent_id>]",
            "  hiveclaw skills export <skill_id> [--output-dir <path>] [--signing-key-hex <hex>] [--format <native|codex>]",
            "  hiveclaw skills doctor [skill_id]",
            "",
            "Manages installed skills, bindings, trust/signature state, and export flows.",
        ]
        .join("\n"),
        Some(topic) if topic == "channels" => [
            "hiveclaw channels",
            "",
            "Usage:",
            "  hiveclaw channels list",
            "  hiveclaw channels status",
            "  hiveclaw channels add <channel>",
            "  hiveclaw channels remove <channel>",
        ]
        .join("\n"),
        Some(topic) if topic == "inspect" => [
            "hiveclaw inspect",
            "",
            "Usage:",
            "  hiveclaw inspect context [session_id] [agent_id]",
            "  hiveclaw inspect benchmark-summary",
            "  hiveclaw inspect runtime-profile",
            "  hiveclaw inspect robot-state [robot_id]",
            "  hiveclaw inspect ros2-profiles [profile_id]",
            "  hiveclaw inspect robotics-runs [robot_id]",
            "  hiveclaw inspect execution-backends",
            "  hiveclaw inspect execution-workers [backend_id]",
            "  hiveclaw inspect rules <workspace_root> [request_text] [target_path]",
            "  hiveclaw inspect provider-payloads [session_id] [agent_id]",
            "  hiveclaw inspect provider-payload [session_id] [agent_id]",
            "  hiveclaw inspect runs <session_id>",
            "  hiveclaw inspect run-tree <session_id> [root_run_id]",
            "  hiveclaw inspect run-events <run_id>",
            "  hiveclaw inspect mailbox <run_id>",
            "  hiveclaw inspect workspace-locks",
            "  hiveclaw inspect mcp-servers",
            "  hiveclaw inspect mcp-imports <server_id>",
            "  hiveclaw inspect mcp-bindings <agent_id>",
            "",
            "Renders JSON inspection output for persisted context and provider payload records.",
        ]
        .join("\n"),
        Some(topic) if topic == "explain" => [
            "hiveclaw explain",
            "",
            "Usage:",
            "  hiveclaw explain context [session_id] [agent_id]",
            "  hiveclaw explain provider-payloads [session_id] [agent_id]",
            "  hiveclaw explain provider-payload [session_id] [agent_id]",
            "",
            "Renders human-readable inspection output for persisted context and provider payload records.",
        ]
        .join("\n"),
        Some(topic) if topic == "run" => [
            "hiveclaw run",
            "",
            "Usage:",
            "  hiveclaw run [config]",
            "",
            "Starts the main runtime/gateway process. If config is omitted, the default",
            "project config path is used.",
        ]
        .join("\n"),
        Some(topic) if topic == "replay" => [
            "hiveclaw replay",
            "",
            "Usage:",
            "  hiveclaw replay golden <suite.toml>",
            "  hiveclaw replay contracts",
            "  hiveclaw replay providers <suite.toml>",
            "  hiveclaw replay gate --golden <suite.toml> [--providers <suite.toml>]",
            "",
            "Runs deterministic golden replay checks, contract regression checks,",
            "and provider comparison benchmark reports.",
        ]
        .join("\n"),
        Some(topic) if topic == "telemetry" => [
            "hiveclaw telemetry",
            "",
            "Usage:",
            "  hiveclaw telemetry export [--scope <local|shared>] [--output-dir <path>]",
            "",
            "Exports local-first telemetry bundles and JSONL event streams.",
        ]
        .join("\n"),
        Some(topic) if topic == "robotics" => [
            "hiveclaw robotics",
            "",
            "Usage:",
            "  hiveclaw robotics simulate <fixture.json>",
            "  hiveclaw robotics ros2-simulate <fixture.json>",
            "",
            "Runs a deterministic robotics simulation from a fixture and persists",
            "the resulting robot state and simulation record into the runtime store.",
            "The ros2-simulate variant compiles through an explicit ROS2 bridge",
            "profile instead of a generic tool invocation path.",
        ]
        .join("\n"),
        Some(topic) if topic == "tui" => [
            "hiveclaw tui",
            "",
            "Usage:",
            "  hiveclaw tui [config]",
            "  hiveclaw tui [config] --attach ws://127.0.0.1:8090/ws",
            "",
            "Starts the terminal UI. Without --attach it spawns a local runtime.",
        ]
        .join("\n"),
        Some(topic) if topic == "setup" => [
            "hiveclaw setup",
            "",
            "Usage:",
            "  hiveclaw setup stt --local",
            "  hiveclaw setup chrome-devtools-mcp [--agent <agent_id>] [--mode <launch_managed|auto_connect>] [--channel <stable|beta|dev|canary>]",
            "  hiveclaw setup ssh-backend --backend-id <id> --host <host> [--user <user>] [--port <port>] [--identity-file <path>]",
            "",
            "Setup commands bootstrap optional runtime integrations.",
        ]
        .join("\n"),
        Some(topic) if topic == "skills" => [
            "hiveclaw skills",
            "",
            "Usage:",
            "  hiveclaw skills list",
            "  hiveclaw skills doctor [skill_id]",
            "  hiveclaw skills install --dir <skill_dir>",
            "  hiveclaw skills install --codex-dir <skill_dir>",
            "  hiveclaw skills install --manifest <skill.toml>",
            "  hiveclaw skills install --signed-dir <skill_dir> [--public-key <hex>]",
            "  hiveclaw skills enable <skill_id>",
            "  hiveclaw skills disable <skill_id>",
            "  hiveclaw skills bind <skill_id> [--agent <agent_id>] [--policy <manual|auto_suggest|auto_load_low_risk|approval_required>] [--version <requirement>]",
            "  hiveclaw skills unbind <skill_id> [--agent <agent_id>]",
            "  hiveclaw skills export <skill_id> [--output-dir <path>] [--signing-key-hex <hex>] [--format <native|codex>]",
            "",
            "Manage installed skills, bindings, export packages, and trust state.",
        ]
        .join("\n"),
        _ => [
            "hiveclaw",
            "",
            "Usage:",
            "  hiveclaw init [path]",
            "  hiveclaw run [config]",
            "  hiveclaw tui [config] [--attach <ws-url>]",
            "  hiveclaw status",
            "  hiveclaw stop",
            "  hiveclaw install [--bin-dir <path>]",
            "  hiveclaw completion <bash|zsh|fish>",
            "  hiveclaw skills <list|install|update|enable|disable|bind|unbind|export|doctor> ...",
            "  hiveclaw doctor",
            "  hiveclaw doctor stt",
            "  hiveclaw doctor env",
            "  hiveclaw doctor gateway",
            "  hiveclaw doctor browser",
            "  hiveclaw doctor mcp",
            "  hiveclaw inspect context [session_id] [agent_id]",
            "  hiveclaw inspect benchmark-summary",
            "  hiveclaw inspect rules <workspace_root> [request_text] [target_path]",
            "  hiveclaw inspect provider-payloads [session_id] [agent_id]",
            "  hiveclaw inspect provider-payload [session_id] [agent_id]",
            "  hiveclaw inspect runs <session_id>",
            "  hiveclaw inspect run-tree <session_id> [root_run_id]",
            "  hiveclaw inspect run-events <run_id>",
            "  hiveclaw inspect mailbox <run_id>",
            "  hiveclaw inspect workspace-locks",
            "  hiveclaw inspect mcp-servers",
            "  hiveclaw inspect mcp-imports <server_id>",
            "  hiveclaw inspect mcp-bindings <agent_id>",
            "  hiveclaw explain context [session_id] [agent_id]",
            "  hiveclaw explain provider-payloads [session_id] [agent_id]",
            "  hiveclaw explain provider-payload [session_id] [agent_id]",
            "  hiveclaw --explain-context <session_id> [agent_id]",
            "  hiveclaw --explain-provider-payloads <session_id> [agent_id]",
            "  hiveclaw setup stt --local",
            "  hiveclaw setup chrome-devtools-mcp [--agent <agent_id>] [--mode <launch_managed|auto_connect>]",
            "  hiveclaw channels <list|status|add|remove> [channel]",
            "  hiveclaw help [topic]",
            "  hiveclaw telemetry export [--scope <local|shared>] [--output-dir <path>]",
            "",
            "Common topics:",
            "  init, run, tui, install, completion, skills, doctor, setup, channels, inspect, explain, replay, telemetry",
            "",
            "Compatibility:",
            "  The legacy `aria-x` command remains available as an alias.",
        ]
        .join("\n"),
    }
}

fn standard_project_config_path() -> Result<PathBuf, String> {
    let dirs = project_dirs()
        .ok_or_else(|| "unable to resolve application config directory".to_string())?;
    Ok(dirs.config_dir().join("config.toml"))
}

fn default_install_bin_dir() -> Result<PathBuf, String> {
    let user_dirs = directories::UserDirs::new()
        .ok_or_else(|| "unable to resolve user home directory for install".to_string())?;
    Ok(user_dirs.home_dir().join(".local").join("bin"))
}

fn path_contains_dir(path_var: &str, dir: &Path) -> bool {
    std::env::split_paths(path_var).any(|entry| entry == dir)
}

fn install_target_path(bin_dir: &Path) -> PathBuf {
    bin_dir.join(PRIMARY_CLI_NAME)
}

fn compatibility_install_target_path(bin_dir: &Path) -> PathBuf {
    bin_dir.join(LEGACY_CLI_NAME)
}

fn install_binary(current_exe: &Path, target: &Path, mode: InstallMode) -> Result<(), String> {
    let parent = target
        .parent()
        .ok_or_else(|| format!("install target '{}' has no parent directory", target.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("create install directory '{}' failed: {}", parent.display(), e))?;
    if target.exists() {
        std::fs::remove_file(target)
            .map_err(|e| format!("remove existing install target '{}' failed: {}", target.display(), e))?;
    }
    match mode {
        InstallMode::Copy => {
            std::fs::copy(current_exe, target).map_err(|e| {
                format!(
                    "copy '{}' to '{}' failed: {}",
                    current_exe.display(),
                    target.display(),
                    e
                )
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(target)
                    .map_err(|e| format!("read install target metadata '{}' failed: {}", target.display(), e))?
                    .permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(target, perms).map_err(|e| {
                    format!("set executable permissions on '{}' failed: {}", target.display(), e)
                })?;
            }
        }
    }
    Ok(())
}

fn run_install_command(args: &[String]) -> Result<String, String> {
    let mut bin_dir = None;
    let mut seed_default_config = false;
    let mut overwrite_config = false;
    let mut idx = 2usize;
    while let Some(arg) = args.get(idx) {
        match arg.as_str() {
            "--bin-dir" => {
                let value = args
                    .get(idx + 1)
                    .ok_or_else(|| "Usage: hiveclaw install [--bin-dir <path>]".to_string())?;
                bin_dir = Some(PathBuf::from(value));
                idx += 2;
            }
            "--with-default-config" => {
                seed_default_config = true;
                idx += 1;
            }
            "--overwrite-config" => {
                overwrite_config = true;
                idx += 1;
            }
            _ => {
                return Err(
                    "Usage: hiveclaw install [--bin-dir <path>] [--with-default-config] [--overwrite-config]"
                        .into(),
                )
            }
        }
    }
    let bin_dir = bin_dir.unwrap_or(default_install_bin_dir()?);
    let target = install_target_path(&bin_dir);
    let legacy_target = compatibility_install_target_path(&bin_dir);
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("resolve current executable failed: {}", e))?;
    install_binary(&current_exe, &target, InstallMode::Copy)?;
    if legacy_target != target {
        install_binary(&current_exe, &legacy_target, InstallMode::Copy)?;
    }
    let config_seed_status = if seed_default_config {
        Some(seed_default_runtime_config(overwrite_config)?)
    } else {
        None
    };

    let path_var = std::env::var("PATH").unwrap_or_default();
    let path_hint = if path_contains_dir(&path_var, &bin_dir) {
        "PATH status: target bin directory is already on PATH.".to_string()
    } else {
        format!(
            "PATH status: '{}' is not on PATH.\nAdd this to your shell profile:\n  export PATH=\"{}:$PATH\"",
            bin_dir.display(),
            bin_dir.display()
        )
    };

    Ok(format!(
        "Installed HiveClaw command(s).\nsource: {}\nprimary_target: {}\ncompatibility_alias: {}\ninstall_mode: copy\n{}\n{}",
        current_exe.display(),
        target.display(),
        legacy_target.display(),
        config_seed_status.unwrap_or_else(|| "config_seed: skipped".to_string()),
        path_hint
    ))
}

fn seed_default_runtime_config(overwrite: bool) -> Result<String, String> {
    let target = standard_project_config_path()?;
    seed_default_runtime_config_at(&target, overwrite)
}

fn seed_default_runtime_config_at(target: &Path, overwrite: bool) -> Result<String, String> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let example_source = manifest_dir.join("config.example.toml");
    let canonical_source = manifest_dir.join("config.toml");
    let source = if example_source.is_file() {
        example_source
    } else {
        canonical_source
    };
    let target_dir = target
        .parent()
        .ok_or_else(|| format!("config target '{}' has no parent directory", target.display()))?;
    std::fs::create_dir_all(target_dir).map_err(|e| {
        format!(
            "create config directory '{}' failed: {}",
            target_dir.display(),
            e
        )
    })?;
    if target.exists() && !overwrite {
        return Ok(format!(
            "config_seed: existing config preserved at {}",
            target.display()
        ));
    }
    std::fs::copy(&source, &target).map_err(|e| {
        format!(
            "copy default config '{}' to '{}' failed: {}",
            source.display(),
            target.display(),
            e
        )
    })?;
    Ok(format!(
        "config_seed: installed default config at {}",
        target.display()
    ))
}

fn render_shell_completion(shell: &str) -> Result<String, String> {
    match shell.trim().to_ascii_lowercase().as_str() {
        "bash" => Ok(
            r#"_hiveclaw_completions() {
    local cur prev words cword
    _init_completion || return
    local commands="init run tui status stop install completion skills doctor setup channels inspect explain help"
    local doctor_topics="stt env gateway browser mcp"
    local completion_shells="bash zsh fish"
    local channel_subcommands="list status add remove"
    local skill_subcommands="list install update enable disable bind unbind export doctor"
    case "${words[1]}" in
        skills)
            COMPREPLY=( $(compgen -W "${skill_subcommands}" -- "$cur") )
            ;;
        doctor)
            COMPREPLY=( $(compgen -W "${doctor_topics}" -- "$cur") )
            ;;
        inspect|explain)
            COMPREPLY=( $(compgen -W "context provider-payloads provider-payload" -- "$cur") )
            ;;
        completion)
            COMPREPLY=( $(compgen -W "${completion_shells}" -- "$cur") )
            ;;
        channels)
            COMPREPLY=( $(compgen -W "${channel_subcommands}" -- "$cur") )
            ;;
        *)
            COMPREPLY=( $(compgen -W "${commands}" -- "$cur") )
            ;;
    esac
}
complete -F _hiveclaw_completions hiveclaw
"#
            .to_string(),
        ),
        "zsh" => Ok(
            r#"#compdef hiveclaw
_hiveclaw() {
  local -a commands
  commands=(
    'init:Bootstrap a local HiveClaw project'
    'run:Run the main runtime'
    'tui:Run the terminal UI'
    'status:Show runtime status'
    'stop:Stop the runtime'
    'install:Install HiveClaw into a user bin directory'
    'completion:Print shell completion script'
    'skills:Manage installed skills and bindings'
    'doctor:Run operator health checks'
    'setup:Run setup flows'
    'channels:Manage configured channels'
    'inspect:Render JSON inspection output'
    'explain:Render human-readable inspection output'
    'help:Show help'
  )
  if (( CURRENT == 2 )); then
    _describe 'command' commands
    return
  fi
  case "$words[2]" in
    skills)
      _values 'skills command' list install update enable disable bind unbind export doctor
      ;;
    doctor)
      _values 'doctor topic' stt env gateway browser mcp
      ;;
    inspect|explain)
      _values 'inspection topic' context provider-payloads provider-payload
      ;;
    completion)
      _values 'shell' bash zsh fish
      ;;
    channels)
      _values 'channel command' list status add remove
      ;;
  esac
}
_hiveclaw
"#
            .to_string(),
        ),
        "fish" => Ok(
            r#"complete -c hiveclaw -f
complete -c hiveclaw -n '__fish_use_subcommand' -a 'init run tui status stop install completion skills doctor setup channels inspect explain help'
complete -c hiveclaw -n '__fish_seen_subcommand_from skills' -a 'list install update enable disable bind unbind export doctor'
complete -c hiveclaw -n '__fish_seen_subcommand_from doctor' -a 'stt env gateway browser mcp'
complete -c hiveclaw -n '__fish_seen_subcommand_from completion' -a 'bash zsh fish'
complete -c hiveclaw -n '__fish_seen_subcommand_from channels' -a 'list status add remove'
complete -c hiveclaw -n '__fish_seen_subcommand_from inspect explain' -a 'context provider-payloads provider-payload'
"#
            .to_string(),
        ),
        _ => Err("Usage: hiveclaw completion <bash|zsh|fish>".into()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitPreset {
    Recommended,
    Edge,
}

impl InitPreset {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "recommended" | "default" => Some(Self::Recommended),
            "edge" => Some(Self::Edge),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Recommended => "recommended",
            Self::Edge => "edge",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuidanceImportMode {
    Merge,
    Reference,
    Ignore,
}

impl GuidanceImportMode {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "merge" => Some(Self::Merge),
            "reference" | "link" => Some(Self::Reference),
            "ignore" | "none" => Some(Self::Ignore),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Reference => "reference",
            Self::Ignore => "ignore",
        }
    }
}

#[derive(Debug, Clone)]
struct InitPlan {
    root_dir: PathBuf,
    preset: InitPreset,
    default_agent: String,
    with_browser: bool,
    suggest_chrome_mcp: bool,
    import_mode: GuidanceImportMode,
}

fn prompt_line(label: &str, default: &str) -> Result<String, String> {
    print!("{} [{}]: ", label, default);
    std::io::stdout()
        .flush()
        .map_err(|e| format!("flush prompt failed: {}", e))?;
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(|e| format!("read prompt input failed: {}", e))?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn prompt_bool(label: &str, default: bool) -> Result<bool, String> {
    let default_str = if default { "Y/n" } else { "y/N" };
    let value = prompt_line(label, default_str)?;
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() || normalized == default_str.to_ascii_lowercase() {
        return Ok(default);
    }
    match normalized.as_str() {
        "y" | "yes" => Ok(true),
        "n" | "no" => Ok(false),
        _ => Ok(default),
    }
}

fn detect_bootstrap_provider() -> (&'static str, &'static str) {
    let runtime = load_runtime_env_config().ok();
    if runtime
        .as_ref()
        .and_then(|cfg| cfg.gemini_api_key.as_ref())
        .is_some()
    {
        return ("gemini", "gemini-3-flash-preview");
    }
    if runtime
        .as_ref()
        .and_then(|cfg| cfg.openrouter_api_key.as_ref())
        .is_some()
    {
        return ("openrouter", "openai/gpt-4o-mini");
    }
    if runtime
        .as_ref()
        .and_then(|cfg| cfg.openai_api_key.as_ref())
        .is_some()
    {
        return ("openai", "gpt-4o-mini");
    }
    if runtime
        .as_ref()
        .and_then(|cfg| cfg.anthropic_api_key.as_ref())
        .is_some()
    {
        return ("anthropic", "claude-3-7-sonnet");
    }
    ("gemini", "gemini-3-flash-preview")
}

fn build_bootstrap_config(plan: &InitPlan) -> String {
    let (backend, model) = detect_bootstrap_provider();
    let timezone = iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string());
    let edge = matches!(plan.preset, InitPreset::Edge);
    let max_tool_rounds = if edge { 4 } else { 6 };
    let max_parallel_requests = if edge { 2 } else { 8 };
    let wasm_max_memory_pages = if edge { 96 } else { 256 };
    let retrieval_char_budget = if edge { 6_000 } else { 16_000 };
    let browser_automation_enabled = if edge {
        false
    } else {
        plan.with_browser
    };
    let learning_enabled = !edge;
    let cluster_profile = if edge { "edge" } else { "node" };
    format!(
        "# HiveClaw project bootstrap\n\n\
         [llm]\n\
         backend = \"{backend}\"\n\
         model = \"{model}\"\n\
         max_tool_rounds = {max_tool_rounds}\n\n\
         [policy]\n\
         policy_path = \"./policies/default.cedar\"\n\n\
         [gateway]\n\
         adapter = \"cli\"\n\
         adapters = [\"cli\"]\n\
         session_scope_policy = \"main\"\n\n\
         [mesh]\n\
         mode = \"peer\"\n\
         endpoints = []\n\n\
         [agents_dir]\n\
         path = \"./agents\"\n\n\
         [router]\n\
         confidence_threshold = 0.70\n\
         tie_break_gap = 0.05\n\n\
         [ssmu]\n\
         sessions_dir = \"./workspace/sessions\"\n\
         operator_skill_signature_max_rows = 5000\n\
         operator_shell_exec_audit_max_rows = 20000\n\n\
         [scheduler]\n\
         enabled = false\n\
         tick_seconds = 1\n\n\
         [localization]\n\
         default_timezone = \"{timezone}\"\n\
         user_timezones = {{}}\n\n\
         [node]\n\
         id = \"orchestrator-1\"\n\
         role = \"orchestrator\"\n\
         tier = \"orchestrator\"\n\n\
         [cluster]\n\
         profile = \"{cluster_profile}\"\n\
         runtime_store_backend = \"sqlite\"\n\
         tenant_id = \"default\"\n\
         workspace_scope = \"default\"\n\
         scheduler_shards = 1\n\n\
         [resource_budget]\n\
         max_parallel_requests = {max_parallel_requests}\n\
         wasm_max_memory_pages = {wasm_max_memory_pages}\n\
         max_tool_rounds = {max_tool_rounds}\n\
         retrieval_context_char_budget = {retrieval_char_budget}\n\
         browser_automation_enabled = {browser_automation_enabled}\n\
         learning_enabled = {learning_enabled}\n\n\
         [ui]\n\
         enabled = false\n\
         bind_addr = \"127.0.0.1:8080\"\n\
         default_agent = \"{default_agent}\"\n",
        backend = backend,
        model = model,
        max_tool_rounds = max_tool_rounds,
        timezone = timezone,
        cluster_profile = cluster_profile,
        max_parallel_requests = max_parallel_requests,
        wasm_max_memory_pages = wasm_max_memory_pages,
        retrieval_char_budget = retrieval_char_budget,
        browser_automation_enabled = browser_automation_enabled,
        learning_enabled = learning_enabled,
        default_agent = plan.default_agent,
    )
}

fn build_hiveclaw_md(
    plan: &InitPlan,
    agents_md: Option<&Path>,
    claude_md: Option<&Path>,
) -> Result<String, String> {
    let mut sections = vec![
        "# HiveClaw Project Guidance".to_string(),
        "".to_string(),
        format!("- preset: `{}`", plan.preset.as_str()),
        format!("- default_agent: `{}`", plan.default_agent),
        format!("- browser_runtime_suggested: `{}`", plan.with_browser),
        format!(
            "- chrome_devtools_mcp_suggested: `{}`",
            plan.suggest_chrome_mcp
        ),
        "".to_string(),
        "## Notes".to_string(),
        "".to_string(),
        "This file is the HiveClaw-local project bootstrap guidance file.".to_string(),
        "Update it with project-specific workflow, tool, and review instructions as the project matures.".to_string(),
        "".to_string(),
    ];

    match plan.import_mode {
        GuidanceImportMode::Ignore => {
            sections.push("## Imported Guidance".to_string());
            sections.push("".to_string());
            sections.push("No external project guidance was imported during bootstrap.".to_string());
        }
        GuidanceImportMode::Reference => {
            sections.push("## Imported Guidance References".to_string());
            sections.push("".to_string());
            if let Some(path) = agents_md {
                sections.push(format!("- AGENTS.md: `{}`", path.display()));
            }
            if let Some(path) = claude_md {
                sections.push(format!("- CLAUDE.md: `{}`", path.display()));
            }
        }
        GuidanceImportMode::Merge => {
            sections.push("## Imported Guidance".to_string());
            sections.push("".to_string());
            if let Some(path) = agents_md {
                sections.push("### Imported from AGENTS.md".to_string());
                sections.push("".to_string());
                sections.push(
                    std::fs::read_to_string(path)
                        .map_err(|e| format!("read '{}' failed: {}", path.display(), e))?,
                );
                sections.push("".to_string());
            }
            if let Some(path) = claude_md {
                sections.push("### Imported from CLAUDE.md".to_string());
                sections.push("".to_string());
                sections.push(
                    std::fs::read_to_string(path)
                        .map_err(|e| format!("read '{}' failed: {}", path.display(), e))?,
                );
                sections.push("".to_string());
            }
            if agents_md.is_none() && claude_md.is_none() {
                sections.push("No existing AGENTS.md or CLAUDE.md file was present.".to_string());
            }
        }
    }

    Ok(sections.join("\n"))
}

fn run_init_command(args: &[String]) -> Result<String, String> {
    let mut root_dir = std::env::current_dir().map_err(|e| format!("resolve current dir failed: {}", e))?;
    let mut preset = InitPreset::Recommended;
    let mut default_agent = "developer".to_string();
    let mut with_browser = true;
    let mut suggest_chrome_mcp = true;
    let mut import_mode = None;
    let mut overwrite = false;
    let mut non_interactive = false;
    let mut idx = 2usize;
    while let Some(arg) = args.get(idx) {
        match arg.as_str() {
            "--preset" => {
                let value = args
                    .get(idx + 1)
                    .ok_or_else(|| "Usage: hiveclaw init [path] [--preset <recommended|edge>]".to_string())?;
                preset = InitPreset::parse(value)
                    .ok_or_else(|| format!("unknown preset '{}'", value))?;
                idx += 2;
            }
            "--default-agent" => {
                let value = args
                    .get(idx + 1)
                    .ok_or_else(|| "Usage: hiveclaw init [path] [--default-agent <agent_id>]".to_string())?;
                default_agent = value.trim().to_string();
                idx += 2;
            }
            "--with-browser" => {
                with_browser = true;
                idx += 1;
            }
            "--without-browser" => {
                with_browser = false;
                idx += 1;
            }
            "--with-chrome-mcp" => {
                suggest_chrome_mcp = true;
                idx += 1;
            }
            "--without-chrome-mcp" => {
                suggest_chrome_mcp = false;
                idx += 1;
            }
            "--import-guidance" => {
                let value = args
                    .get(idx + 1)
                    .ok_or_else(|| "Usage: hiveclaw init [path] [--import-guidance <merge|reference|ignore>]".to_string())?;
                import_mode = GuidanceImportMode::parse(value);
                if import_mode.is_none() {
                    return Err(format!("unknown import guidance mode '{}'", value));
                }
                idx += 2;
            }
            "--overwrite" => {
                overwrite = true;
                idx += 1;
            }
            "--non-interactive" => {
                non_interactive = true;
                idx += 1;
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown argument '{}'", other));
            }
            other => {
                root_dir = PathBuf::from(other);
                idx += 1;
            }
        }
    }

    let agents_md = root_dir.join("AGENTS.md");
    let claude_md = root_dir.join("CLAUDE.md");
    let guidance_present = agents_md.is_file() || claude_md.is_file();
    let interactive = !non_interactive
        && std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal();

    if interactive {
        let preset_value = prompt_line("Preset", preset.as_str())?;
        if let Some(parsed) = InitPreset::parse(&preset_value) {
            preset = parsed;
        }
        default_agent = prompt_line("Default agent", &default_agent)?;
        with_browser = prompt_bool("Enable browser-friendly setup hints?", with_browser)?;
        suggest_chrome_mcp =
            prompt_bool("Suggest Chrome DevTools MCP setup in next steps?", suggest_chrome_mcp)?;
        if guidance_present {
            let import_default = import_mode.unwrap_or(GuidanceImportMode::Merge).as_str();
            let import_value = prompt_line(
                "Import existing AGENTS.md / CLAUDE.md into HIVECLAW.md (merge/reference/ignore)",
                import_default,
            )?;
            if let Some(parsed) = GuidanceImportMode::parse(&import_value) {
                import_mode = Some(parsed);
            }
        }
    }

    if matches!(preset, InitPreset::Edge) {
        with_browser = false;
        suggest_chrome_mcp = false;
    }

    let plan = InitPlan {
        root_dir: root_dir.clone(),
        preset,
        default_agent,
        with_browser,
        suggest_chrome_mcp,
        import_mode: import_mode.unwrap_or(if guidance_present {
            GuidanceImportMode::Merge
        } else {
            GuidanceImportMode::Ignore
        }),
    };

    let config_dir = root_dir.join(".hiveclaw");
    let config_path = config_dir.join("config.toml");
    let policy_dir = config_dir.join("policies");
    let agents_dir = config_dir.join("agents");
    let sessions_dir = config_dir.join("workspace").join("sessions");
    let guidance_path = root_dir.join("HIVECLAW.md");

    if !overwrite && (config_path.exists() || guidance_path.exists()) {
        return Err(format!(
            "bootstrap output already exists under '{}'. Re-run with --overwrite to replace it.",
            config_dir.display()
        ));
    }

    std::fs::create_dir_all(&policy_dir)
        .map_err(|e| format!("create policy dir '{}' failed: {}", policy_dir.display(), e))?;
    std::fs::create_dir_all(&agents_dir)
        .map_err(|e| format!("create agents dir '{}' failed: {}", agents_dir.display(), e))?;
    std::fs::create_dir_all(&sessions_dir)
        .map_err(|e| format!("create sessions dir '{}' failed: {}", sessions_dir.display(), e))?;

    std::fs::write(&policy_dir.join("default.cedar"), DEFAULT_BOOTSTRAP_POLICY)
        .map_err(|e| format!("write bootstrap policy failed: {}", e))?;
    std::fs::write(&config_path, build_bootstrap_config(&plan))
        .map_err(|e| format!("write bootstrap config failed: {}", e))?;
    std::fs::write(
        agents_dir.join("README.md"),
        "# HiveClaw agents\n\nPlace agent configuration files here as the project grows.\n",
    )
    .map_err(|e| format!("write agents readme failed: {}", e))?;
    std::fs::write(
        &guidance_path,
        build_hiveclaw_md(
            &plan,
            agents_md.is_file().then_some(agents_md.as_path()),
            claude_md.is_file().then_some(claude_md.as_path()),
        )?,
    )
    .map_err(|e| format!("write HIVECLAW.md failed: {}", e))?;

    let browser_note = if plan.suggest_chrome_mcp {
        format!(
            "next_step_browser: run `{}` to enable Chrome-backed browser tooling",
            format!("{PRIMARY_CLI_NAME} setup chrome-devtools-mcp --agent {}", plan.default_agent)
        )
    } else {
        "next_step_browser: browser tooling not suggested for this preset".to_string()
    };

    Ok(format!(
        "HiveClaw project bootstrapped.\nroot: {}\nconfig: {}\npreset: {}\ndefault_agent: {}\nimport_guidance: {}\n{}\nnext_step_run: {} run {}",
        plan.root_dir.display(),
        config_path.display(),
        plan.preset.as_str(),
        plan.default_agent,
        plan.import_mode.as_str(),
        browser_note,
        PRIMARY_CLI_NAME,
        config_path.display(),
    ))
}

fn run_preflight_cli_command(args: &[String]) -> Option<Result<String, String>> {
    match args.get(1).map(String::as_str) {
        Some("help") => Some(Ok(render_cli_help(args.get(2).map(String::as_str)))),
        Some("-h") | Some("--help") => Some(Ok(render_cli_help(None))),
        Some("init") => Some(run_init_command(args)),
        Some("install") => Some(run_install_command(args)),
        Some("completion") => Some(render_shell_completion(
            args.get(2).map(String::as_str).unwrap_or_default(),
        )),
        _ => None,
    }
}

pub(crate) fn run_main() {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime build failed")
        .block_on(actual_main());
}

fn node_supports_ingress(role: &str) -> bool {
    matches!(
        role.trim().to_ascii_lowercase().as_str(),
        "orchestrator" | "combined" | "all" | "ingress"
    )
}

fn node_supports_outbound(role: &str) -> bool {
    matches!(
        role.trim().to_ascii_lowercase().as_str(),
        "orchestrator" | "combined" | "all" | "outbound"
    )
}

fn node_supports_scheduler(role: &str) -> bool {
    matches!(
        role.trim().to_ascii_lowercase().as_str(),
        "orchestrator" | "combined" | "all" | "scheduler"
    )
}

fn spawn_supervised_adapter<F, Fut>(
    adapter_name: &'static str,
    channel: GatewayChannel,
    shutdown: Arc<ShutdownCoordinator>,
    make_future: F,
) -> tokio::task::JoinHandle<()>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: core::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        let mut attempt = 0u32;
        loop {
            if shutdown.is_stopping() {
                crate::channel_health::mark_channel_adapter_state(channel, "stopped");
                break;
            }
            attempt = attempt.saturating_add(1);
            crate::channel_health::mark_channel_adapter_state(channel, "starting");
            crate::channel_health::record_channel_health_event(
                channel,
                crate::channel_health::ChannelHealthEventKind::AdapterStarted,
            );
            let child = tokio::spawn(make_future());
            tokio::pin!(child);
            let restart = tokio::select! {
                join = &mut child => {
                    match join {
                        Ok(()) => {
                            crate::channel_health::record_channel_health_event(
                                channel,
                                crate::channel_health::ChannelHealthEventKind::AdapterExited,
                            );
                            warn!(
                                adapter = adapter_name,
                                attempt = attempt,
                                "Adapter exited; scheduling restart"
                            );
                        }
                        Err(err) => {
                            crate::channel_health::record_channel_health_event(
                                channel,
                                crate::channel_health::ChannelHealthEventKind::AdapterPanicked,
                            );
                            warn!(
                                adapter = adapter_name,
                                attempt = attempt,
                                error = %err,
                                "Adapter task panicked; scheduling restart"
                            );
                        }
                    }
                    true
                }
                _ = shutdown.wait() => {
                    crate::channel_health::mark_channel_adapter_state(channel, "stopping");
                    child.as_mut().abort();
                    let _ = child.await;
                    false
                }
            };
            if !restart || shutdown.is_stopping() {
                crate::channel_health::mark_channel_adapter_state(channel, "stopped");
                break;
            }
            crate::channel_health::record_channel_health_event(
                channel,
                crate::channel_health::ChannelHealthEventKind::AdapterRestarted,
            );
            let backoff_secs = u64::from(attempt.min(5));
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(backoff_secs.max(1))) => {}
                _ = shutdown.wait() => {
                    crate::channel_health::mark_channel_adapter_state(channel, "stopped");
                    break;
                }
            }
        }
    })
}

async fn actual_main() {
    // Load .env from CWD and ~/.hiveclaw/.env (plus legacy ~/.aria/.env) before config.
    load_env();

    let args: Vec<String> = std::env::args().collect();
    let runtime_env = load_runtime_env_config().unwrap_or_else(|err| {
        eprintln!("[HiveClaw] Failed to resolve runtime environment config: {}", err);
        std::process::exit(1);
    });

    if let Some(output) = run_preflight_cli_command(&args) {
        match output {
            Ok(text) => {
                println!("{}", text);
                return;
            }
            Err(err) => {
                eprintln!("[HiveClaw] Command failed: {}", err);
                std::process::exit(1);
            }
        }
    }

    let startup_mode = crate::tui::parse_startup_mode(&args, runtime_env.config_path.clone());
    let tui_mode = matches!(startup_mode, crate::tui::StartupMode::Tui { .. });
    let config_path = match &startup_mode {
        crate::tui::StartupMode::Runtime { config_path }
        | crate::tui::StartupMode::Tui { config_path, .. } => config_path.clone(),
    };
    let runtime_config_path = if config_path.trim().is_empty() {
        default_runtime_config_path()
    } else {
        resolve_config_path(&config_path).with_extension("runtime.json")
    };

    if let Some(output) = run_process_control_command(&args) {
        match output {
            Ok(text) => {
                println!("{}", text);
                return;
            }
            Err(err) => {
                eprintln!("[HiveClaw] Process command failed: {}", err);
                std::process::exit(1);
            }
        }
    }

    if tui_mode {
        let attach_url = match &startup_mode {
            crate::tui::StartupMode::Tui { attach_url, .. } => attach_url.as_deref(),
            _ => None,
        };
        if let Err(err) = crate::tui::run_tui_mode(&config_path, attach_url).await {
            eprintln!("[HiveClaw] TUI failed: {}", err);
            std::process::exit(1);
        }
        return;
    }

    println!("[HiveClaw] Loading config from: {}", config_path);

    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "[HiveClaw] Failed to load config '{}' (cwd: {}): {}",
                config_path,
                std::env::current_dir().unwrap_or_default().display(),
                e
            );
            let _ = std::io::stderr().flush();
            std::process::exit(1);
        }
    };

    if let Some(output) = run_channel_onboarding_command(&config.path, &args) {
        match output {
            Ok(text) => {
                println!("{}", text);
                return;
            }
            Err(err) => {
                eprintln!("{}", err);
                std::process::exit(1);
            }
        }
    }

    if let Some(output) = run_stt_management_command(&config, &args) {
        match output {
            Ok(text) => {
                println!("{}", text);
                return;
            }
            Err(err) => {
                eprintln!("[HiveClaw] STT command failed: {}", err);
                std::process::exit(1);
            }
        }
    }

    if let Some(output) = run_replay_management_command(&config, &args) {
        match output {
            Ok(text) => {
                println!("{}", text);
                return;
            }
            Err(err) => {
                eprintln!("[HiveClaw] Replay command failed: {}", err);
                std::process::exit(1);
            }
        }
    }

    if let Some(output) = run_telemetry_management_command(&config, &args) {
        match output {
            Ok(text) => {
                println!("{}", text);
                return;
            }
            Err(err) => {
                eprintln!("[HiveClaw] Telemetry command failed: {}", err);
                std::process::exit(1);
            }
        }
    }

    if let Some(output) = run_skill_management_command(&config, &args) {
        match output {
            Ok(text) => {
                println!("{}", text);
                return;
            }
            Err(err) => {
                eprintln!("[HiveClaw] Skills command failed: {}", err);
                std::process::exit(1);
            }
        }
    }

    if let Err(err) = validate_config(&config) {
        eprintln!("[HiveClaw] Config validation error: {}", err);
        eprintln!("[HiveClaw] For Telegram: set TELEGRAM_BOT_TOKEN or add telegram_token to config");
        let _ = std::io::stderr().flush();
        std::process::exit(1);
    }
    let config = Arc::new(config);
    install_app_runtime(Arc::clone(&config));
    let runtime_pid_guard = match register_runtime_pid(&config.path) {
        Ok(guard) => Some(guard),
        Err(err) => {
            warn!(error = %err, "Failed to register runtime pid file");
            None
        }
    };
    let shutdown_coordinator = Arc::new(ShutdownCoordinator::new());

    RuntimeStore::configure_operator_retention(
        config.ssmu.operator_skill_signature_max_rows,
        config.ssmu.operator_shell_exec_audit_max_rows,
        config.ssmu.operator_scope_denial_max_rows,
        config.ssmu.operator_request_policy_audit_max_rows,
        config.ssmu.operator_repair_fallback_audit_max_rows,
        config.ssmu.operator_streaming_decision_audit_max_rows,
        config.ssmu.operator_browser_action_audit_max_rows,
        config.ssmu.operator_browser_challenge_event_max_rows,
    );

    if let Some(result) = run_operator_cli_command(&config, &args) {
        match result {
            Ok(text) => {
                println!("{}", text);
                return;
            }
            Err(err) => {
                eprintln!("[HiveClaw] Operator command failed: {}", err);
                std::process::exit(1);
            }
        }
    }
    if let Some(result) = run_robotics_command(&config, &args) {
        match result {
            Ok(text) => {
                println!("{}", text);
                return;
            }
            Err(err) => {
                eprintln!("[HiveClaw] Robotics command failed: {}", err);
                std::process::exit(1);
            }
        }
    }

    match run_admin_inspect_command(&config, &args) {
        Ok(Some(json)) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json)
                    .unwrap_or_else(|_| "{\"error\":\"serialize failed\"}".into())
            );
            return;
        }
        Ok(None) => {}
        Err(err) => {
            eprintln!("[HiveClaw] Inspect command failed: {}", err);
            std::process::exit(1);
        }
    }
    match run_admin_explain_command(&config, &args) {
        Ok(Some(text)) => {
            println!("{}", text);
            return;
        }
        Ok(None) => {}
        Err(err) => {
            eprintln!("[HiveClaw] Explain command failed: {}", err);
            std::process::exit(1);
        }
    }

    // Init tracing (RUST_LOG overrides config)
    init_tracing(&config);

    info!(
        node = %config.node.id,
        role = %config.node.role,
        instance_id = %runtime_instance_id(),
        llm = %config.llm.backend,
        model = %config.llm.model,
        "Config loaded"
    );
    let feature_flags = runtime_feature_flags();
    info!(
        multi_channel_gateway = feature_flags.multi_channel_gateway,
        append_only_session_log = feature_flags.append_only_session_log,
        resource_leases_enforced = feature_flags.resource_leases_enforced,
        outbox_delivery = feature_flags.outbox_delivery,
        "Runtime feature flags"
    );
    if config.simulator.enabled {
        info!(backend = %config.simulator.backend, "Simulator mode enabled");
    }

    // Initialize Cedar policy engine (fail fast — never run without valid policy)
    let policy_content = match std::fs::read_to_string(&config.policy.policy_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "[HiveClaw] Fatal: failed to read policy file '{}': {}",
                config.policy.policy_path, e
            );
            std::process::exit(1);
        }
    };
    let cedar = match aria_policy::CedarEvaluator::from_policy_str(&policy_content) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[HiveClaw] Fatal: failed to parse Cedar policies: {}", e);
            std::process::exit(1);
        }
    };
    let cedar = Arc::new(cedar);

    // Initialize Semantic Router with MiniLM-L6-v2 embedder (384-dim SBERT)
    let embedder = Arc::new(
        FastEmbedder::new().unwrap_or_else(|e| {
            warn!(error = %e, "FastEmbedder init failed, falling back to LocalHashEmbedder not available in this path");
            panic!("Cannot initialize embedding model: {}", e);
        })
    );
    let mut router = SemanticRouter::new();
    let agent_store = AgentConfigStore::load_from_dir(&config.agents_dir.path).unwrap_or_default();
    let mut tool_registry = ToolManifestStore::new();
    let mut vector_store = VectorStore::new();

    // Index workspace knowledge documents with real semantic embeddings
    vector_store.index_document(
        "workspace.files",
        "File system tools: list files, read source code, navigate project structure.",
        embedder.embed("list files read source navigate project workspace"),
        "workspace",
        vec!["files".into(), "source".into(), "workspace".into()],
        false,
    );
    vector_store.index_document(
        "workspace.rust",
        "Rust development: cargo build, cargo test, compile crates, fix errors.",
        embedder.embed("rust cargo build test compile crates errors"),
        "workspace",
        vec!["rust".into(), "cargo".into(), "build".into()],
        false,
    );
    vector_store.index_document(
        "security.policy",
        "Cedar policy engine: authorization decisions, access control, denied paths.",
        embedder.embed("security authorization cedar policy access control"),
        "policy",
        vec!["security".into(), "authorization".into(), "cedar".into()],
        false,
    );
    if agent_store.is_empty() {
        // Bootstrap fallback agents when no TOML configs found
        let _ = router.register_agent_text(
            "developer",
            "Write code, read files, search codebase, run tests, execute shell commands",
            &*embedder,
        );
        let _ = router.register_agent_text(
            "researcher",
            "Search the web, fetch URLs, summarise documents, query knowledge base",
            &*embedder,
        );
        warn!(
            path = %config.agents_dir.path,
            "No agent configs found; using bootstrap agents"
        );
    } else {
        // Register each loaded agent and index its full description + system prompt
        for cfg in agent_store.all() {
            // Register agent embedding using full description for better routing
            let _ = router.register_agent_text(&cfg.id, &cfg.description, &*embedder);

            // Index the agent as a knowledge document in the vector store
            let agent_doc_text = format!("{} {}", cfg.description, cfg.system_prompt);
            vector_store.index_document(
                format!("agent.{}", cfg.id),
                format!("{}: {}", cfg.id, cfg.description),
                embedder.embed(&agent_doc_text),
                "agent",
                vec![cfg.id.clone()],
                false,
            );

            // Register tools with real descriptions and schemas
            for tool_name in &cfg.base_tool_names {
                if !runtime_exposes_base_tool(tool_name) {
                    warn!(tool = %tool_name, agent = %cfg.id, "Skipping unavailable base tool during bootstrap registration");
                    continue;
                }
                let (desc, schema) = match tool_name.as_str() {
                    "read_file" => (
                        "Read the contents of a file at the given path. Returns the file content as text.",
                        r#"{"path": {"type":"string","description":"File path to read"}}"#,
                    ),
                    "write_file" => (
                        "Write content to a file at the given path. Creates the file if it does not exist.",
                        r#"{"path": {"type":"string","description":"File path to write to"}, "content": {"type":"string","description":"Text content to write"}}"#,
                    ),
                    "search_codebase" => (
                        "Search the codebase for a pattern or keyword. Returns matching file paths and snippets.",
                        r#"{"query": {"type":"string","description":"Search pattern or keyword"}}"#,
                    ),
                    "run_tests" => (
                        "Run the test suite and return pass/fail results.",
                        r#"{"target": {"type":"string","description":"Crate or test name to run, or empty for all"}}"#,
                    ),
                    "run_shell" => (
                        "Execute a shell command and return stdout/stderr output. Can optionally target a configured execution backend such as local, docker, or ssh.",
                        r#"{"command": {"type":"string","description":"Shell command to run"}, "backend_id": {"type":"string","description":"Optional execution backend id such as local-default, docker-sandbox, or a configured ssh backend profile"}, "docker_image": {"type":"string","description":"Optional docker image override when backend_id targets docker"}, "cwd": {"type":"string","description":"Optional working directory; required for docker backend and scoped filesystem execution, and mapped through remote workspace configuration for ssh backends"}, "allow_network_egress": {"type":"boolean","description":"Whether the selected backend may access the network if it supports egress"}}"#,
                    ),
                    "search_web" => (
                        "Search the web for information about a query. Returns a summary of top results.",
                        r#"{"query": {"type":"string","description":"Web search query"}}"#,
                    ),
                    "fetch_url" => (
                        "Fetch the content of a URL and return it as text.",
                        r#"{"url": {"type":"string","description":"URL to fetch"}}"#,
                    ),
                    "set_domain_access_decision" => (
                        "Persist a domain access decision for a target agent. This is sensitive and requires human approval.",
                        r#"{"domain": {"type":"string","description":"Domain or URL to normalize and store"}, "decision": {"type":"string","enum":["allow_once","allow_for_session","allow_always","deny_once","deny_always"],"description":"Decision to persist"}, "action_family": {"type":"string","enum":["fetch","crawl","screenshot","interactive_read","interactive_write","login","download"],"description":"Action family controlled by the decision"}, "scope": {"type":"string","enum":["domain","session","request"],"description":"Storage scope override"}, "agent_id": {"type":"string","description":"Optional target agent id; defaults to the invoking agent"}, "reason": {"type":"string","description":"Optional audit note"}}"#,
                    ),
                    "browser_profile_create" => (
                        "Create a managed browser profile for later authenticated or read-only browsing.",
                        r#"{"profile_id": {"type":"string","description":"Stable profile id"}, "display_name": {"type":"string","description":"Optional human-friendly name"}, "mode": {"type":"string","enum":["ephemeral","managed_persistent","attached_external","extension_bound"],"description":"Browser profile mode"}, "engine": {"type":"string","enum":["chromium","chrome","edge","safari_bridge"],"description":"Browser engine"}, "allowed_domains": {"type":"array","items":{"type":"string"},"description":"Optional default domain allowlist"}, "auth_enabled": {"type":"boolean","description":"Whether the profile can be used for authenticated flows"}, "write_enabled": {"type":"boolean","description":"Whether the profile can be used for write actions"}, "persistent": {"type":"boolean","description":"Whether the profile is persistent"}, "attached_source": {"type":"string","description":"Optional external browser/profile source identifier for attached profiles"}, "extension_binding_id": {"type":"string","description":"Optional extension binding id for extension-bound profiles"}}"#,
                    ),
                    "browser_profile_list" => (
                        "List managed browser profiles available to the runtime.",
                        r#"{}"#,
                    ),
                    "browser_profile_use" => (
                        "Bind a managed browser profile to the current session and agent.",
                        r#"{"profile_id": {"type":"string","description":"Managed browser profile id to bind for the current session"}}"#,
                    ),
                    "browser_session_start" => (
                        "Launch a managed browser session using a stored browser profile.",
                        r#"{"profile_id": {"type":"string","description":"Optional managed profile id; defaults to the current session binding"}, "url": {"type":"string","description":"Optional start URL"}} "#,
                    ),
                    "browser_session_list" => (
                        "List browser sessions for the current agent and session.",
                        r#"{}"#,
                    ),
                    "browser_session_status" => (
                        "Inspect a specific browser session record by id.",
                        r#"{"browser_session_id": {"type":"string","description":"Managed browser session id to inspect"}}"#,
                    ),
                    "browser_session_cleanup" => (
                        "Mark stale launched browser sessions as exited when their process is no longer alive.",
                        r#"{"browser_session_id": {"type":"string","description":"Optional managed browser session id to limit cleanup output"}} "#,
                    ),
                    "browser_session_persist_state" => (
                        "Persist the current browser storage state for a managed browser session as an encrypted state snapshot.",
                        r#"{"browser_session_id": {"type":"string","description":"Managed browser session id to persist state for"}}"#,
                    ),
                    "browser_session_restore_state" => (
                        "Restore the latest encrypted browser storage state for the managed profile backing a browser session.",
                        r#"{"browser_session_id": {"type":"string","description":"Managed browser session id to restore state into"}}"#,
                    ),
                    "browser_session_pause" => (
                        "Pause a managed browser session after a challenge or human handoff boundary.",
                        r#"{"browser_session_id": {"type":"string","description":"Managed browser session id to pause"}}"#,
                    ),
                    "browser_session_resume" => (
                        "Resume a previously paused managed browser session.",
                        r#"{"browser_session_id": {"type":"string","description":"Managed browser session id to resume"}}"#,
                    ),
                    "browser_session_record_challenge" => (
                        "Record a detected browser challenge event for a managed browser session.",
                        r#"{"browser_session_id": {"type":"string","description":"Managed browser session id"}, "challenge": {"type":"string","enum":["captcha","mfa","bot_defense","login_required","unknown"],"description":"Detected challenge kind"}, "url": {"type":"string","description":"Optional page URL"}, "message": {"type":"string","description":"Optional challenge message"}}"#,
                    ),
                    "browser_login_status" => (
                        "List persisted browser login state records for the current agent/session, optionally filtered by browser session id or domain.",
                        r#"{"browser_session_id": {"type":"string","description":"Optional managed browser session id to filter"}, "domain": {"type":"string","description":"Optional domain or URL to normalize and filter"}} "#,
                    ),
                    "browser_login_begin_manual" => (
                        "Mark a managed browser session as waiting for manual login on a target domain and pause the session.",
                        r#"{"browser_session_id": {"type":"string","description":"Managed browser session id"}, "domain": {"type":"string","description":"Domain or URL being authenticated"}, "notes": {"type":"string","description":"Optional login notes"}} "#,
                    ),
                    "browser_login_complete_manual" => (
                        "Mark a manually-assisted login as completed for a managed browser session and resume the session.",
                        r#"{"browser_session_id": {"type":"string","description":"Managed browser session id"}, "domain": {"type":"string","description":"Domain or URL that was authenticated"}, "credential_key_names": {"type":"array","items":{"type":"string"},"description":"Optional vault key names used for this login"}, "notes": {"type":"string","description":"Optional login notes"}} "#,
                    ),
                    "browser_login_fill_credentials" => (
                        "Fill approved credentials from the vault into a managed browser session through the browser automation bridge without exposing secret values to the model.",
                        r#"{"browser_session_id": {"type":"string","description":"Managed browser session id"}, "domain": {"type":"string","description":"Domain or URL being authenticated"}, "credentials": {"type":"array","description":"Credential fill descriptors","items":{"type":"object","properties":{"key_name":{"type":"string"},"selector":{"type":"string"},"field":{"type":"string"}},"required":["key_name"],"additionalProperties":false}}}"#,
                    ),
                    "browser_open" => (
                        "Open a URL in a managed browser profile and start a browser session.",
                        r#"{"profile_id": {"type":"string","description":"Optional managed profile id; defaults to the current binding"}, "url": {"type":"string","description":"URL to open in the browser session"}}"#,
                    ),
                    "browser_act" => (
                        "Perform a typed browser action. Navigate and wait are implemented; click/type/select/scroll remain gated until the DOM automation backend is enabled.",
                        r#"{"browser_session_id": {"type":"string","description":"Managed browser session id"}, "action": {"type":"string","enum":["navigate","wait","click","type","select","scroll"],"description":"Browser action to perform"}, "url": {"type":"string","description":"Target URL for navigate"}, "selector": {"type":"string","description":"Target selector for click/type/select/scroll"}, "text": {"type":"string","description":"Input text for type"}, "value": {"type":"string","description":"Selected value for select"}, "millis": {"type":"integer","description":"Wait duration in milliseconds for wait"}}"#,
                    ),
                    "browser_snapshot" => (
                        "Fetch and persist an HTML snapshot for a page within a managed browser session.",
                        r#"{"browser_session_id": {"type":"string","description":"Managed browser session id"}, "url": {"type":"string","description":"URL to snapshot"}}"#,
                    ),
                    "browser_screenshot" => (
                        "Capture a real PNG screenshot for a page within a managed browser session using the configured browser engine.",
                        r#"{"browser_session_id": {"type":"string","description":"Managed browser session id"}, "url": {"type":"string","description":"URL to capture as a screenshot"}}"#,
                    ),
                    "browser_extract" => (
                        "Fetch and persist extracted page text for a page within a managed browser session.",
                        r#"{"browser_session_id": {"type":"string","description":"Managed browser session id"}, "url": {"type":"string","description":"URL to extract"}}"#,
                    ),
                    "browser_download" => (
                        "Download a URL into a managed browser session artifact with audit.",
                        r#"{"browser_session_id": {"type":"string","description":"Managed browser session id"}, "url": {"type":"string","description":"URL to download"}, "filename": {"type":"string","description":"Optional output filename override"}}"#,
                    ),
                    "computer_profile_list" => (
                        "List configured computer execution profiles and active computer sessions for the current session.",
                        r#"{}"#,
                    ),
                    "computer_session_start" => (
                        "Prepare or reuse a local computer-runtime session on a selected execution profile.",
                        r#"{"profile_id":{"type":"string","description":"Optional computer execution profile id"}, "target_window_id":{"type":"string","description":"Optional initial target window id"}}"#,
                    ),
                    "computer_session_list" => (
                        "List computer-runtime sessions for the current agent and session.",
                        r#"{}"#,
                    ),
                    "computer_capture" | "computer_screenshot" => (
                        "Capture a real screenshot from the local computer runtime and persist it as a computer artifact.",
                        r#"{"computer_session_id":{"type":"string","description":"Optional computer session id to reuse"}, "profile_id":{"type":"string","description":"Optional computer execution profile id"}} "#,
                    ),
                    "computer_act" => (
                        "Perform a desktop computer-runtime action such as pointer move, pointer click, keyboard typing, key press, clipboard read, or clipboard write. High-risk actions require approval.",
                        r#"{"computer_session_id":{"type":"string","description":"Optional computer session id to reuse"}, "profile_id":{"type":"string","description":"Optional computer execution profile id"}, "target_window_id":{"type":"string","description":"Optional target window id for focused actions"}, "action":{"type":"string","enum":["pointer_move","pointer_click","keyboard_type","key_press","clipboard_read","clipboard_write"],"description":"Computer action to perform"}, "x":{"type":"integer","description":"Screen x coordinate for pointer actions"}, "y":{"type":"integer","description":"Screen y coordinate for pointer actions"}, "button":{"type":"string","enum":["left","right","middle"],"description":"Pointer button for clicks"}, "text":{"type":"string","description":"Text for keyboard_type or clipboard_write"}, "key":{"type":"string","description":"Key label for key_press"}}"#,
                    ),
                    "web_fetch" => (
                        "Fetch a URL over HTTP and return the response body and content type.",
                        r#"{"url": {"type":"string","description":"URL to fetch"}}"#,
                    ),
                    "web_extract" => (
                        "Fetch a URL over HTTP and return extracted text content.",
                        r#"{"url": {"type":"string","description":"URL to fetch and extract"}}"#,
                    ),
                    "crawl_page" => (
                        "Crawl a single page, extract text, and update website memory for the domain.",
                        r#"{"url": {"type":"string","description":"Page URL to crawl"}, "capture_screenshots": {"type":"boolean","description":"Reserved for future screenshot capture"}, "change_detection": {"type":"boolean","description":"Reserved for future change detection controls"}}"#,
                    ),
                    "crawl_site" => (
                        "Crawl a site within the requested scope, extract text from discovered pages, and update website memory.",
                        r#"{"url": {"type":"string","description":"Seed site URL to crawl"}, "scope": {"type":"string","enum":["single_page","same_origin","allowlisted_domains","scheduled_watch_allowed"],"description":"Crawl scope to apply"}, "allowed_domains": {"type":"array","items":{"type":"string"},"description":"Optional allowlisted domains for allowlisted_domains scope"}, "max_depth": {"type":"integer","description":"Maximum crawl depth"}, "max_pages": {"type":"integer","description":"Maximum number of pages to crawl"}, "capture_screenshots": {"type":"boolean","description":"Reserved for future screenshot capture"}, "change_detection": {"type":"boolean","description":"Reserved for future change detection controls"}}"#,
                    ),
                    "watch_page" => (
                        "Schedule periodic monitoring for a single page and summarize meaningful changes over time.",
                        r#"{"url": {"type":"string","description":"Page URL to monitor"}, "schedule": {"type":"object","description":"Structured schedule object. Examples: {\"kind\":\"at\",\"at\":\"2026-08-28T19:00:00+05:30\"}, {\"kind\":\"every\",\"seconds\":300}, {\"kind\":\"daily\",\"hour\":9,\"minute\":0,\"timezone\":\"Asia/Kolkata\"}, {\"kind\":\"weekly\",\"weekday\":\"mon\",\"hour\":10,\"minute\":30,\"timezone\":\"Asia/Kolkata\"}, {\"kind\":\"cron\",\"expr\":\"0 0 * * * *\",\"timezone\":\"Asia/Kolkata\"}"}, "agent_id": {"type":"string","description":"Agent that should execute the watch checks"}, "capture_screenshots": {"type":"boolean","description":"Whether to capture screenshots during checks"}, "change_detection": {"type":"boolean","description":"Whether to summarize only meaningful changes"}}"#,
                    ),
                    "watch_site" => (
                        "Schedule periodic monitoring for a site within the same domain and summarize meaningful changes.",
                        r#"{"url": {"type":"string","description":"Site URL to monitor"}, "schedule": {"type":"object","description":"Structured schedule object. Examples: {\"kind\":\"at\",\"at\":\"2026-08-28T19:00:00+05:30\"}, {\"kind\":\"every\",\"seconds\":300}, {\"kind\":\"daily\",\"hour\":9,\"minute\":0,\"timezone\":\"Asia/Kolkata\"}, {\"kind\":\"weekly\",\"weekday\":\"mon\",\"hour\":10,\"minute\":30,\"timezone\":\"Asia/Kolkata\"}, {\"kind\":\"cron\",\"expr\":\"0 0 * * * *\",\"timezone\":\"Asia/Kolkata\"}"}, "agent_id": {"type":"string","description":"Agent that should execute the watch checks"}, "capture_screenshots": {"type":"boolean","description":"Whether to capture screenshots during checks"}, "change_detection": {"type":"boolean","description":"Whether to summarize only meaningful changes"}}"#,
                    ),
                    "list_watch_jobs" => (
                        "List persisted page and site watch jobs for the current agent.",
                        r#"{}"#,
                    ),
                    "summarise_doc" => (
                        "Summarise a long document into concise bullet points.",
                        r#"{"text": {"type":"string","description":"Document text to summarise"}}"#,
                    ),
                    "query_rag" => (
                        "Query the local RAG knowledge base for relevant context about a topic.",
                        r#"{"query": {"type":"string","description":"Topic or question to search for"}}"#,
                    ),
                    "manage_cron" => (
                        "Manage scheduled jobs. Supports add, update, delete, list. Use a structured schedule object with kind=at/every/daily/weekly/cron. DO NOT use tool/agent prefixes in response tool field.",
                        r#"{"type":"object","properties":{"action": {"type":"string","enum":["add","update","delete","list"],"description":"CRUD action to perform"}, "id": {"type":"string","description":"Unique job ID, primarily for update and delete"}, "agent_id": {"type":"string","description":"Agent ID to trigger"}, "prompt": {"type":"string","description":"Prompt to send"}, "schedule": {"type":"object","description":"Structured schedule object. Examples: {\"kind\":\"at\",\"at\":\"2026-08-28T19:00:00+05:30\"}, {\"kind\":\"every\",\"seconds\":120}, {\"kind\":\"daily\",\"hour\":19,\"minute\":30,\"timezone\":\"Asia/Kolkata\"}, {\"kind\":\"weekly\",\"weekday\":\"sat\",\"hour\":11,\"minute\":0,\"interval_weeks\":2,\"timezone\":\"Asia/Kolkata\"}, {\"kind\":\"cron\",\"expr\":\"0 30 19 * * *\",\"timezone\":\"Asia/Kolkata\"}"}},"required":["action"],"additionalProperties":false}"#,
                    ),
                    "schedule_message" | "set_reminder" => (
                        "Schedule reminder behavior. Modes: notify (default, sends message at due time), defer (run task prompt at due time via agent), both (notify and defer).",
                        r#"{"type":"object","properties":{"task": {"type":"string","description":"Reminder text or deferred task prompt"}, "schedule": {"type":"object","description":"Structured schedule object. Examples: {\"kind\":\"at\",\"at\":\"2026-08-28T19:00:00+05:30\"}, {\"kind\":\"every\",\"seconds\":120}, {\"kind\":\"daily\",\"hour\":19,\"minute\":30,\"timezone\":\"Asia/Kolkata\"}, {\"kind\":\"weekly\",\"weekday\":\"sat\",\"hour\":11,\"minute\":0,\"interval_weeks\":2,\"timezone\":\"Asia/Kolkata\"}, {\"kind\":\"cron\",\"expr\":\"0 30 19 * * *\",\"timezone\":\"Asia/Kolkata\"}"}, "mode": {"type":"string","enum":["notify","defer","both"],"description":"Execution mode"}, "deferred_prompt": {"type":"string","description":"Optional task prompt executed at trigger time when mode is defer/both"}, "agent_id": {"type":"string","description":"Agent to execute deferred task with"}},"required":["task","schedule"],"additionalProperties":false}"#,
                    ),
                    _ => ("Execute a tool operation.", "{}"),
                };
                tool_registry
                    .register_with_embedding(
                    CachedTool {
                        name: tool_name.clone(),
                        description: desc.into(),
                        parameters_schema: schema.into(),
                        embedding: Vec::new(),
                        requires_strict_schema: false,
                        streaming_safe: false,
                        parallel_safe: true,
                        modalities: vec![aria_core::ToolModality::Text],
                    },
                    &embedder,
                )
                .unwrap_or_else(|e| panic!("invalid built-in tool schema for {}: {}", tool_name, e));
                // Index tool with real description text
                vector_store.index_tool_description(
                    tool_name.clone(), // Use clean tool name as ID
                    desc.to_string(),
                    embedder.embed(&format!("{} {}", tool_name, desc)),
                    tool_name,
                    vec![cfg.id.clone()],
                );
            }
        }
        info!(
            count = agent_store.len(),
            path = %config.agents_dir.path,
            "Loaded agent profiles"
        );
    }

    for tool_name in [
        "register_external_compat_tool",
        "register_remote_tool",
        "register_mcp_server",
        "sync_mcp_server_catalog",
        "setup_chrome_devtools_mcp",
        "import_mcp_tool",
        "import_mcp_prompt",
        "import_mcp_resource",
        "bind_mcp_import",
        "invoke_mcp_tool",
        "render_mcp_prompt",
        "read_mcp_resource",
        "browser_profile_create",
        "browser_profile_list",
        "browser_profile_use",
        "browser_session_start",
        "browser_session_list",
        "browser_session_status",
        "browser_open",
        "browser_snapshot",
        "browser_extract",
        "browser_screenshot",
        "browser_act",
        "browser_download",
        "computer_profile_list",
        "computer_session_start",
        "computer_session_list",
        "computer_capture",
        "computer_screenshot",
        "computer_act",
        "crawl_page",
        "crawl_site",
        "watch_page",
        "watch_site",
        "set_domain_access_decision",
    ] {
        register_discoverable_tool(
            &mut tool_registry,
            &mut vector_store,
            &*embedder,
            tool_name,
            "runtime",
        );
    }

    // Register meta tool: search_tool_registry
    let search_desc =
        "Search the tool registry and hot-swap the best matching tool for the current task.";
    tool_registry
        .register_with_embedding(
            CachedTool {
                name: "search_tool_registry".into(),
                description: search_desc.into(),
                parameters_schema:
                    r#"{"query": {"type":"string","description":"Description of the capability you need"}}"#
                        .into(),
                embedding: Vec::new(),
                requires_strict_schema: false,
                streaming_safe: false,
                parallel_safe: true,
                modalities: vec![aria_core::ToolModality::Text],
            },
            &embedder,
        )
        .unwrap_or_else(|e| panic!("invalid search_tool_registry schema: {}", e));
    vector_store.index_tool_description(
        "search_tool_registry", // Use clean tool name as ID
        search_desc.to_string(),
        embedder.embed("search tool registry find best tool capability"),
        "search_tool_registry",
        vec!["registry".into(), "meta".into()],
    );
    tool_registry
        .validate_strict_startup_contract()
        .unwrap_or_else(|e| panic!("tool registry startup validation failed: {}", e));
    // NOTE: sensor.bootstrap.imu removed — irrelevant for non-robotics agents.
    // Sensor annotations are only indexed when robotics_ctrl agent is active.
    let route_cfg = RouteConfig {
        confidence_threshold: config.router.confidence_threshold,
        tie_break_gap: config.router.tie_break_gap,
    };
    let router_index = router.build_index(route_cfg);
    aria_intelligence::install_provider_transport_config(
        aria_intelligence::ProviderTransportConfig {
            response_start_timeout: Duration::from_millis(config.llm.first_token_timeout_ms.max(1)),
        },
    );
    let llm_pool = LlmBackendPool::new(
        vec!["primary".into(), "fallback".into()],
        Duration::from_secs(30),
    )
    .with_provider_circuit_breaker(
        Duration::from_millis(config.llm.provider_circuit_breaker_cooldown_ms.max(1)),
        config.llm.provider_circuit_breaker_failure_threshold.max(1),
    );
    // Initialize Credential Vault
    let master_key_raw = config.runtime.master_key.clone().unwrap_or_else(|| {
        eprintln!("[HiveClaw] Fatal: HIVECLAW_MASTER_KEY or ARIA_MASTER_KEY is required");
        std::process::exit(1);
    });
    let mut master_key = [0u8; 32];
    let key_bytes = master_key_raw.as_bytes();
    for i in 0..32.min(key_bytes.len()) {
        master_key[i] = key_bytes[i];
    }
    let vault = Arc::new(CredentialVault::new(&config.vault.storage_path, master_key));

    // Check for --vault-set command
    if let Some(pos) = args.iter().position(|a| a == "--vault-set") {
        if args.len() > pos + 2 {
            let key_name = &args[pos + 1];
            let secret_value = &args[pos + 2];
            let allowed_domains = vec![
                "openrouter.ai".to_string(),
                "openai.com".to_string(),
                "anthropic.com".to_string(),
            ];
            if let Err(e) = vault.store_secret("system", key_name, secret_value, allowed_domains) {
                error!("Failed to store secret in vault: {}", e);
                std::process::exit(1);
            }
            info!("Successfully stored secret '{}' in vault", key_name);
            std::process::exit(0);
        } else {
            error!("Usage: --vault-set <key_name> <secret_value>");
            std::process::exit(1);
        }
    }

    let registry = Arc::new(Mutex::new(ProviderRegistry::new()));
    {
        let sessions_dir = std::path::PathBuf::from(&config.ssmu.sessions_dir);
        let provider_egress_broker = aria_intelligence::EgressCredentialBroker::new()
            .with_audit_sink(move |record| {
                let outcome = match record.outcome {
                    aria_intelligence::EgressSecretOutcome::Allowed => {
                        aria_core::SecretUsageOutcome::Allowed
                    }
                    aria_intelligence::EgressSecretOutcome::Denied => {
                        aria_core::SecretUsageOutcome::Denied
                    }
                };
                let _ = RuntimeStore::for_sessions_dir(&sessions_dir).append_secret_usage_audit(
                    &aria_core::SecretUsageAuditRecord {
                        audit_id: uuid::Uuid::new_v4().to_string(),
                        agent_id: "system".into(),
                        session_id: None,
                        tool_name: record.scope,
                        key_name: record.key_name,
                        target_domain: record.target_domain,
                        outcome,
                        detail: record.detail,
                        created_at_us: chrono::Utc::now().timestamp_micros() as u64,
                    },
                );
            });
        let mut reg = registry.lock().await;
        reg.register(Arc::new(backends::ollama::OllamaProvider {
            base_url: config.runtime.ollama_host.clone(),
        }));

        // Resolve remote API keys: Vault -> Env -> Placeholder
        let openrouter_key = match vault.retrieve_global_secret("openrouter_key", "openrouter_ai") {
            Ok(_) => SecretRef::Vault {
                key_name: "openrouter_key".to_string(),
                vault: (*vault).clone(),
            },
            Err(_) => {
                if let Some(key) = config.runtime.openrouter_api_key.clone() {
                    SecretRef::Literal(key)
                } else {
                    SecretRef::Literal("sk-or-placeholder".to_string())
                }
            }
        };
        let openai_key = match vault.retrieve_global_secret("openai_key", "api.openai.com") {
            Ok(_) => SecretRef::Vault {
                key_name: "openai_key".to_string(),
                vault: (*vault).clone(),
            },
            Err(_) => SecretRef::Literal(
                config
                    .runtime
                    .openai_api_key
                    .clone()
                    .unwrap_or_else(|| "sk-openai-placeholder".to_string()),
            ),
        };
        let anthropic_key = match vault.retrieve_global_secret("anthropic_key", "api.anthropic.com")
        {
            Ok(_) => SecretRef::Vault {
                key_name: "anthropic_key".to_string(),
                vault: (*vault).clone(),
            },
            Err(_) => SecretRef::Literal(
                config
                    .runtime
                    .anthropic_api_key
                    .clone()
                    .unwrap_or_else(|| "sk-ant-placeholder".to_string()),
            ),
        };
        let gemini_key = match vault
            .retrieve_global_secret("gemini_key", "generativelanguage.googleapis.com")
        {
            Ok(_) => SecretRef::Vault {
                key_name: "gemini_key".to_string(),
                vault: (*vault).clone(),
            },
            Err(_) => SecretRef::Literal(
                config
                    .runtime
                    .gemini_api_key
                    .clone()
                    .unwrap_or_else(|| "gemini-placeholder".to_string()),
            ),
        };

        reg.register(Arc::new(backends::openrouter::OpenRouterProvider {
            api_key: openrouter_key,
            site_url: "aria-x".into(),
            site_title: "HiveClaw".into(),
            egress_broker: Some(provider_egress_broker.clone()),
        }));
        reg.register(Arc::new(backends::openai::OpenAiProvider {
            api_key: openai_key,
            base_url: "https://api.openai.com/v1".into(),
            egress_broker: Some(provider_egress_broker.clone()),
        }));
        reg.register(Arc::new(backends::anthropic::AnthropicProvider {
            api_key: anthropic_key,
            base_url: "https://api.anthropic.com/v1".into(),
            egress_broker: Some(provider_egress_broker.clone()),
        }));
        reg.register(Arc::new(backends::gemini::GeminiProvider {
            api_key: gemini_key,
            base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
            egress_broker: Some(provider_egress_broker),
        }));
    }

    match run_live_admin_inspect_command(&config, &args, &registry).await {
        Ok(Some(json)) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json)
                    .unwrap_or_else(|_| "{\"error\":\"serialize failed\"}".into())
            );
            return;
        }
        Ok(None) => {}
        Err(err) => {
            eprintln!("[HiveClaw] Live inspect command failed: {}", err);
            std::process::exit(1);
        }
    }

    match config.llm.backend.to_lowercase().as_str() {
        "ollama" => {
            let now_us = chrono::Utc::now().timestamp_micros() as u64;
            let profile = resolve_model_capability_profile(
                &registry,
                Path::new(&config.ssmu.sessions_dir),
                Some(&config.llm),
                "ollama",
                &config.llm.model,
                now_us,
            )
            .await;
            if let Some(profile) = profile {
                let reg = registry.lock().await;
                if let Ok(ollama) = reg.create_backend_with_profile(&profile) {
                    llm_pool.register_backend(
                        "primary",
                        reg.create_backend_with_profile(&profile)
                            .unwrap_or_else(|_| Box::new(OllamaBackend::new(config.runtime.ollama_host.clone(), config.llm.model.clone()))),
                    );
                    llm_pool.register_backend("fallback", ollama);
                } else {
                    let ollama = OllamaBackend::new(config.runtime.ollama_host.clone(), config.llm.model.clone());
                    llm_pool.register_backend("primary", Box::new(ollama.clone()));
                    llm_pool.register_backend("fallback", Box::new(ollama));
                }
            } else {
                let ollama = OllamaBackend::new(config.runtime.ollama_host.clone(), config.llm.model.clone());
                llm_pool.register_backend("primary", Box::new(ollama.clone()));
                llm_pool.register_backend("fallback", Box::new(ollama));
            }
            info!(model = %config.llm.model, host = %config.runtime.ollama_host, "LLM: Ollama");
        }
        "openrouter" => {
            let now_us = chrono::Utc::now().timestamp_micros() as u64;
            let profile = resolve_model_capability_profile(
                &registry,
                Path::new(&config.ssmu.sessions_dir),
                Some(&config.llm),
                "openrouter",
                &config.llm.model,
                now_us,
            )
            .await;
            let reg = registry.lock().await;
            if let Some(profile) = profile {
                if let Ok(openrouter) = reg.create_backend_with_profile(&profile) {
                    llm_pool.register_backend("primary", openrouter.clone());
                    llm_pool.register_backend("fallback", openrouter);
                    info!(model = %config.llm.model, "LLM: OpenRouter (REST)");
                } else {
                    warn!("Failed to create OpenRouter backend with capability profile, falling back");
                    llm_pool.register_backend("primary", Box::new(LocalMockLLM));
                    llm_pool.register_backend("fallback", Box::new(LocalMockLLM));
                }
            } else if let Ok(openrouter) = reg.create_backend("openrouter", &config.llm.model) {
                llm_pool.register_backend("primary", openrouter.clone());
                llm_pool.register_backend("fallback", openrouter);
                info!(model = %config.llm.model, "LLM: OpenRouter (REST)");
            } else {
                warn!("Failed to create OpenRouter backend, falling back to mock");
                llm_pool.register_backend("primary", Box::new(LocalMockLLM));
                llm_pool.register_backend("fallback", Box::new(LocalMockLLM));
            }
        }
        "openai" | "anthropic" | "gemini" => {
            let provider_id = config.llm.backend.to_lowercase();
            let now_us = chrono::Utc::now().timestamp_micros() as u64;
            let profile = resolve_model_capability_profile(
                &registry,
                Path::new(&config.ssmu.sessions_dir),
                Some(&config.llm),
                &provider_id,
                &config.llm.model,
                now_us,
            )
            .await;
            let reg = registry.lock().await;
            if let Some(profile) = profile {
                if let Ok(backend) = reg.create_backend_with_profile(&profile) {
                    llm_pool.register_backend("primary", backend.clone());
                    llm_pool.register_backend("fallback", backend);
                    info!(provider = %provider_id, model = %config.llm.model, "LLM: remote provider");
                } else {
                    warn!(provider = %provider_id, "Failed to create backend with capability profile, falling back");
                    llm_pool.register_backend("primary", Box::new(LocalMockLLM));
                    llm_pool.register_backend("fallback", Box::new(LocalMockLLM));
                }
            } else if let Ok(backend) = reg.create_backend(&provider_id, &config.llm.model) {
                llm_pool.register_backend("primary", backend.clone());
                llm_pool.register_backend("fallback", backend);
                info!(provider = %provider_id, model = %config.llm.model, "LLM: remote provider");
            } else {
                warn!(provider = %provider_id, "Failed to create backend, falling back to mock");
                llm_pool.register_backend("primary", Box::new(LocalMockLLM));
                llm_pool.register_backend("fallback", Box::new(LocalMockLLM));
            }
        }
        _ => {
            llm_pool.register_backend("primary", Box::new(LocalMockLLM));
            llm_pool.register_backend("fallback", Box::new(LocalMockLLM));
            info!("LLM: mock (set backend=ollama/openrouter/openai/anthropic/gemini)");
        }
    }
    let llm_pool = Arc::new(llm_pool);

    // Initialize Session Memory
    let session_db_path = session_runtime_db_path(Path::new(&config.ssmu.sessions_dir));
    let session_memory = aria_ssmu::SessionMemory::new_sqlite_backed(100, &session_db_path);
    let load_report = session_memory
        .load_from_sqlite(&session_db_path)
        .or_else(|_| session_memory.load_from_dir(&config.ssmu.sessions_dir));
    if let Ok(report) = load_report {
        info!(
            loaded = report.loaded_sessions,
            skipped = report.skipped_files,
            "Loaded persisted sessions"
        );
        if report.loaded_sessions > 0 {
            let embedder_clone = Arc::clone(&embedder);
            let _ = session_memory
                .index_session_summaries_to(&mut vector_store, move |s| embedder_clone.embed(s));
            let _ = session_memory.save_to_sqlite(&session_db_path);
        }
    }
    // Build dynamic capability index: one node per loaded agent + bootstrap system nodes
    let capability_index = build_dynamic_capability_index(&agent_store);
    let capability_index = Arc::new(capability_index);
    let vector_store = Arc::new(vector_store);
    let session_tool_caches: Arc<SessionToolCacheStore> = Arc::new(SessionToolCacheStore::new(
        config.runtime.session_tool_cache_max_entries,
    ));

    // --- HookRegistry Setup for non-Telegram interfaces ---
    let session_locks = Arc::new(dashmap::DashMap::new());
    let embed_semaphore = Arc::new(tokio::sync::Semaphore::new(2));
    let mut hooks = HookRegistry::new();
    hooks.register_message_pre(Box::new(|req, vector_store, page_index| {
        let request_text = request_text_from_content(&req.content);
        Box::pin(async move {
            let document_index = build_document_index_from_vector_store(&vector_store);
            let hybrid = HybridMemoryEngine::new(
                &vector_store,
                document_index.as_tree(),
                QueryPlannerConfig::default(),
            )
            .retrieve(&request_text, &local_embed(&request_text, 64), 3, 3);
            let vector_context = hybrid.vector_context.join("\n");
            let capability_context = page_index
                .retrieve_relevant(&request_text, 3)
                .into_iter()
                .map(|n| format!("- {}: {}", n.title, n.summary))
                .collect::<Vec<_>>()
                .join("\n");
            let document_context = hybrid
                .document_context
                .into_iter()
                .map(|n| format!("- {}: {}", n.title, n.summary))
                .collect::<Vec<_>>()
                .join("\n");
            let rag_context = format!(
                "Plan: {:?}\nVector Context:\n{}\n\nCapability Index Context:\n{}\n\nDocument Index Context:\n{}",
                hybrid.plan, vector_context, capability_context, document_context
            );
            Ok(PromptHookAsset {
                label: "legacy_hybrid_memory".into(),
                content: rag_context,
            })
        })
    }));
    let hooks = Arc::new(hooks);

    // Build keyword index for BM25 hybrid search (RRF)
    let keyword_index = Arc::new(KeywordIndex::new().expect("Failed to create keyword index"));
    {
        // Batch-index all documents that are already in the vector store
        let mut kw_docs: Vec<(String, String)> = Vec::new();
        kw_docs.push((
            "workspace.files".into(),
            "File system tools: list files, read source code, navigate project structure.".into(),
        ));
        kw_docs.push((
            "workspace.rust".into(),
            "Rust development: cargo build, cargo test, compile crates, fix errors.".into(),
        ));
        kw_docs.push((
            "security.policy".into(),
            "Cedar policy engine: authorization decisions, access control, denied paths.".into(),
        ));
        for cfg in agent_store.all() {
            kw_docs.push((
                format!("agent.{}", cfg.id),
                format!("{} {}", cfg.description, cfg.system_prompt),
            ));
            for tool_name in &cfg.base_tool_names {
                if !runtime_exposes_base_tool(tool_name) {
                    continue;
                }
                let desc = match tool_name.as_str() {
                    "read_file" => "Read the contents of a file at the given path.",
                    "write_file" => "Write content to a file at the given path.",
                    "search_codebase" => "Search the codebase for a pattern or keyword.",
                    "run_tests" => "Run the test suite and return pass/fail results.",
                    "run_shell" => "Execute a shell command and return stdout/stderr output.",
                    "search_web" => "Search the web for information about a query.",
                    "fetch_url" => "Fetch the content of a URL and return it as text.",
                    "set_domain_access_decision" =>
                        "Persist a domain access decision for a target agent.",
                    "browser_profile_create" =>
                        "Create a managed browser profile for later browsing flows.",
                    "browser_profile_list" => "List managed browser profiles.",
                    "browser_profile_use" =>
                        "Bind a managed browser profile to the current session and agent.",
                    "browser_session_start" =>
                        "Launch a managed browser session using a stored profile.",
                    "browser_session_list" => "List managed browser sessions.",
                    "browser_session_status" => "Inspect a managed browser session record.",
                    "browser_session_cleanup" =>
                        "Mark stale launched browser sessions as exited after process death.",
                    "browser_session_persist_state" =>
                        "Persist encrypted browser session storage state for a managed session.",
                    "browser_session_restore_state" =>
                        "Restore encrypted browser session storage state for a managed session.",
                    "browser_session_pause" =>
                        "Pause a managed browser session after a challenge boundary.",
                    "browser_session_resume" =>
                        "Resume a previously paused managed browser session.",
                    "browser_session_record_challenge" =>
                        "Record a challenge event for a managed browser session.",
                    "browser_login_status" =>
                        "List persisted browser login state records for the current agent and session.",
                    "browser_login_begin_manual" =>
                        "Pause a managed browser session and mark manual login as pending.",
                    "browser_login_complete_manual" =>
                        "Mark a managed browser login flow as completed and authenticated.",
                    "browser_login_fill_credentials" =>
                        "Fill approved vault credentials into a managed browser session without exposing secret values to the model.",
                    "browser_open" => "Open a URL in a managed browser session.",
                    "browser_act" =>
                        "Perform a typed browser action against a managed browser session.",
                    "browser_snapshot" =>
                        "Persist an HTML snapshot for a page in a managed browser session.",
                    "browser_screenshot" =>
                        "Capture a real PNG screenshot for a page in a managed browser session.",
                    "browser_extract" =>
                        "Persist extracted page text for a page in a managed browser session.",
                    "browser_download" =>
                        "Download a URL into a managed browser session artifact with audit.",
                    "web_fetch" => "Fetch a URL over HTTP and return the raw response body.",
                    "web_extract" =>
                        "Fetch a URL over HTTP and return extracted page text.",
                    "crawl_page" =>
                        "Crawl a single page, extract text, and update website memory for the domain.",
                    "crawl_site" =>
                        "Crawl a site within scope, extract discovered pages, and update website memory.",
                    "watch_page" =>
                        "Schedule periodic monitoring for a single page and summarize meaningful changes.",
                    "watch_site" =>
                        "Schedule periodic monitoring for a site within the same domain and summarize meaningful changes.",
                    "list_watch_jobs" => "List persisted page and site watch jobs.",
                    "summarise_doc" => "Summarise a long document into concise bullet points.",
                    "query_rag" => "Query the local RAG knowledge base for relevant context.",
                    _ => "Execute a tool operation.",
                };
                kw_docs.push((format!("tool.{}", tool_name), desc.into()));
            }
        }
        kw_docs.push((
            "tool.search_tool_registry".into(),
            "Search the tool registry and hot-swap the best matching tool.".into(),
        ));
        if let Err(e) = keyword_index.add_documents_batch(&kw_docs) {
            warn!(error = %e, "Failed to populate keyword index");
        } else {
            info!(
                count = kw_docs.len(),
                "Keyword index populated for hybrid RAG"
            );
        }
    }

    // Initialize Credential Vault
    let master_key_raw = config.runtime.master_key.clone().unwrap_or_else(|| {
        eprintln!("[HiveClaw] Fatal: HIVECLAW_MASTER_KEY or ARIA_MASTER_KEY is required");
        std::process::exit(1);
    });
    let mut master_key = [0u8; 32];
    let key_bytes = master_key_raw.as_bytes();
    for i in 0..32.min(key_bytes.len()) {
        master_key[i] = key_bytes[i];
    }
    let vault = Arc::new(CredentialVault::new(&config.vault.storage_path, master_key));

    // Check for --vault-set command
    if let Some(pos) = args.iter().position(|a| a == "--vault-set") {
        if args.len() > pos + 2 {
            let key_name = &args[pos + 1];
            let secret_value = &args[pos + 2];
            let allowed_domains = vec![
                "openrouter.ai".to_string(),
                "openai.com".to_string(),
                "anthropic.com".to_string(),
            ];
            if let Err(e) = vault.store_secret("system", key_name, secret_value, allowed_domains) {
                error!("Failed to store secret in vault: {}", e);
                std::process::exit(1);
            }
            info!("Successfully stored secret '{}' in vault", key_name);
            std::process::exit(0);
        } else {
            error!("Usage: --vault-set <key_name> <secret_value>");
            std::process::exit(1);
        }
    }

    let mut bad_patterns = vec![
        "sk-".to_string(),
        "ghp_".to_string(),
        "AKIA".to_string(),
        "ignore all previous instructions".to_string(),
        "system prompt".to_string(),
    ];
    // Add all secrets from the vault to the leak scanner patterns
    if let Ok(secrets) = vault.decrypt_all() {
        for s in secrets {
            if s.len() > 5 {
                bad_patterns.push(s);
            }
        }
    }
    let firewall = Arc::new(aria_safety::DfaFirewall::new(bad_patterns));

    let shared_config = Arc::clone(&config);
    let agent_store = Arc::new(agent_store);
    let tool_registry = Arc::new(tool_registry);

    // Initialise Scheduler early so it runs for all gateways
    let (tx_cron, rx_cron) = tokio::sync::mpsc::channel::<aria_intelligence::CronCommand>(64);
    if shared_config.scheduler.enabled {
        let boot_job_count = seed_scheduler_runtime_store(
            Path::new(&shared_config.ssmu.sessions_dir),
            &shared_config.scheduler.jobs,
        )
        .unwrap_or(0);
        info!(jobs = boot_job_count, "Scheduler enabled");
    } else {
        info!(
            "Scheduler preloaded jobs disabled; runtime scheduler remains active for dynamic reminders"
        );
    }
    let node_role = shared_config.node.role.clone();
    let _scheduler_commands = if node_supports_scheduler(&node_role) {
        Some(spawn_scheduler_command_processor(
            Path::new(&shared_config.ssmu.sessions_dir).to_path_buf(),
            rx_cron,
        ))
    } else {
        info!(role = %node_role, "Skipping scheduler command processor for non-scheduler node role");
        None
    };

    // Spawn Background Scheduler Processor
    let sc_config = Arc::clone(&shared_config);
    let sc_router_index = router_index.clone();
    let sc_embedder = Arc::clone(&embedder);
    let sc_llm_pool = Arc::clone(&llm_pool);
    let sc_cedar = Arc::clone(&cedar);
    let sc_agent_store = Arc::clone(&agent_store);
    let sc_tool_registry = Arc::clone(&tool_registry);
    let sc_session_memory = session_memory.clone();
    let sc_page_index = Arc::clone(&capability_index);
    let sc_vector_store = Arc::clone(&vector_store);
    let sc_keyword_index = Arc::clone(&keyword_index);
    let sc_firewall = Arc::clone(&firewall);
    let sc_vault = Arc::clone(&vault);
    let sc_tx_cron = tx_cron.clone();
    let sc_registry = Arc::clone(&registry);
    let sc_caches = Arc::clone(&session_tool_caches);
    let sc_hooks = Arc::clone(&hooks);
    let sc_locks = Arc::clone(&session_locks);
    let sc_semaphore = Arc::clone(&embed_semaphore);
    let sc_worker_id = scheduler_worker_id(&sc_config);

    if node_supports_scheduler(&sc_config.node.role) {
    tokio::spawn(async move {
        info!(role = %sc_config.node.role, "Background scheduler processor started");
        let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(60));
        let mut due_tick = tokio::time::interval(std::time::Duration::from_secs(
            sc_config.scheduler.tick_seconds.max(1),
        ));
        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    debug!("Background scheduler heartbeat: alive");
                }
                _ = due_tick.tick() => {
                    let sessions_dir = std::path::Path::new(&sc_config.ssmu.sessions_dir);
                    let scheduler_shard = if sc_config.cluster.is_cluster() {
                        let total_shards = sc_config.cluster.scheduler_shards.max(1);
                        Some((
                            scheduler_shard_for_node(&sc_config.node.id, total_shards),
                            total_shards,
                        ))
                    } else {
                        None
                    };
                    if sc_config.cluster.is_cluster()
                        && sc_config.cluster.scheduler_shards <= 1
                        && !try_acquire_scheduler_leadership(
                            sessions_dir,
                            &sc_worker_id,
                            0,
                            sc_config.scheduler.tick_seconds.saturating_mul(4).max(30),
                        )
                        .await
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    let events = match poll_due_job_events_from_store(
                        sessions_dir,
                        &sc_worker_id,
                        sc_config.scheduler.tick_seconds.saturating_mul(4).max(30),
                        scheduler_shard,
                    )
                    .await {
                        Ok(events) => events,
                        Err(err) => {
                            error!(error = %err, "Failed to poll due job events from runtime store");
                            continue;
                        }
                    };
                    for ev in events {
                        info!(job_id = %ev.job_id, agent_id = %ev.agent_id, prompt = %ev.prompt, "Scheduled prompt fired (background)");

                        let session_id = execution_session_id_for_scheduled_event(&ev);
                        let session_uuid = uuid::Uuid::from_bytes(session_id);
                        if let Err(err) = sc_session_memory.update_overrides(
                            session_uuid,
                            Some(ev.agent_id.clone()),
                            None,
                        ) {
                            warn!(
                                session_id = %session_uuid,
                                agent_id = %ev.agent_id,
                                error = %err,
                                "failed to persist scheduled-event session override"
                            );
                        }

                        let req = aria_core::AgentRequest {
                            request_id: *uuid::Uuid::new_v4().as_bytes(),
                            session_id,
                            channel: ev.channel.unwrap_or(aria_core::GatewayChannel::Unknown),
                            user_id: ev.user_id.unwrap_or_else(|| "system".to_string()),
                            content: aria_core::MessageContent::Text(ev.prompt.clone()),
                            tool_runtime_policy: None,
                            timestamp_us: chrono::Utc::now().timestamp_micros() as u64,
                        };

                        if matches!(ev.kind, ScheduledJobKind::Notify) {
                            send_universal_response(&req, &ev.prompt, &sc_config).await;
                            let _ = sc_tx_cron
                                .send(aria_intelligence::CronCommand::UpdateStatus {
                                    id: ev.job_id.clone(),
                                    status: aria_intelligence::ScheduledJobStatus::Completed,
                                    detail: None,
                                    timestamp_us: chrono::Utc::now().timestamp_micros() as u64,
                                })
                                .await;
                            let _ = persist_scheduler_job_snapshot(
                                &sc_tx_cron,
                                sessions_dir,
                                &ev.job_id,
                            )
                            .await;
                            let _ = RuntimeStore::for_sessions_dir(sessions_dir)
                                .release_job_lease(&ev.job_id, &sc_worker_id);
                            continue;
                        }

                        match process_request(
                            &req,
                            &sc_config.learning,
                            &sc_router_index,
                            &*sc_embedder,
                            &sc_llm_pool,
                            &sc_cedar,
                            &*sc_agent_store,
                            &*sc_tool_registry,
                            &sc_session_memory,
                            &sc_page_index,
                            &sc_vector_store,
                            &sc_keyword_index,
                            &sc_firewall,
                            &sc_vault,
                            &sc_tx_cron,
                            &sc_registry,
                            sc_caches.as_ref(),
                            &*sc_hooks,
                            &sc_locks,
                            &sc_semaphore,
                            sc_config.llm.max_tool_rounds,
                            None,
                            Some(&Arc::new(std::sync::atomic::AtomicBool::new(false))),
                            std::path::Path::new(&sc_config.ssmu.sessions_dir),
                            sc_config.policy.whitelist.clone(),
                            sc_config.policy.forbid.clone(),
                            resolve_request_timezone(&sc_config, &req.user_id),
                        )
                        .await
                        {
                            Ok(aria_intelligence::OrchestratorResult::Completed(text)) => {
                                send_universal_response(&req, &text, &sc_config).await;
                                let _ = sc_tx_cron
                                    .send(aria_intelligence::CronCommand::UpdateStatus {
                                        id: ev.job_id.clone(),
                                        status: aria_intelligence::ScheduledJobStatus::Completed,
                                        detail: None,
                                        timestamp_us: chrono::Utc::now().timestamp_micros() as u64,
                                    })
                                    .await;
                                let _ = persist_scheduler_job_snapshot(
                                    &sc_tx_cron,
                                    sessions_dir,
                                    &ev.job_id,
                                )
                                .await;
                                let _ = RuntimeStore::for_sessions_dir(sessions_dir)
                                    .release_job_lease(&ev.job_id, &sc_worker_id);
                            }
                            Ok(aria_intelligence::OrchestratorResult::AgentElevationRequired { agent_id, message }) => {
                                let approval_result = aria_intelligence::OrchestratorResult::AgentElevationRequired {
                                    agent_id: agent_id.clone(),
                                    message: message.clone(),
                                };
                                let approval_text = persist_pending_approval_for_result(
                                    sessions_dir,
                                    &req,
                                    &approval_result,
                                )
                                .map(|(_, text)| text)
                                .unwrap_or(message);
                                send_universal_response(&req, &approval_text, &sc_config).await;
                                let _ = sc_tx_cron
                                    .send(aria_intelligence::CronCommand::UpdateStatus {
                                        id: ev.job_id.clone(),
                                        status: aria_intelligence::ScheduledJobStatus::ApprovalRequired,
                                        detail: Some(format!("Agent elevation required for {}", agent_id)),
                                        timestamp_us: chrono::Utc::now().timestamp_micros() as u64,
                                    })
                                    .await;
                                let _ = persist_scheduler_job_snapshot(
                                    &sc_tx_cron,
                                    sessions_dir,
                                    &ev.job_id,
                                )
                                .await;
                                let _ = RuntimeStore::for_sessions_dir(sessions_dir)
                                    .release_job_lease(&ev.job_id, &sc_worker_id);
                            }
                            Ok(result @ aria_intelligence::OrchestratorResult::ToolApprovalRequired { .. }) => {
                                let approval_text = persist_pending_approval_for_result(
                                    sessions_dir,
                                    &req,
                                    &result,
                                )
                                .map(|(_, text)| text)
                                .unwrap_or_else(|_| {
                                    "Scheduled task requires approval.".to_string()
                                });
                                send_universal_response(
                                    &req,
                                    &approval_text,
                                    &sc_config,
                                )
                                .await;
                                let _ = sc_tx_cron
                                    .send(aria_intelligence::CronCommand::UpdateStatus {
                                        id: ev.job_id.clone(),
                                        status:
                                            aria_intelligence::ScheduledJobStatus::ApprovalRequired,
                                        detail: Some(
                                            "Scheduled task requires approval".to_string(),
                                        ),
                                        timestamp_us: chrono::Utc::now().timestamp_micros() as u64,
                                    })
                                    .await;
                                let _ = persist_scheduler_job_snapshot(
                                    &sc_tx_cron,
                                    sessions_dir,
                                    &ev.job_id,
                                )
                                .await;
                                let _ = RuntimeStore::for_sessions_dir(sessions_dir)
                                    .release_job_lease(&ev.job_id, &sc_worker_id);
                            }
                            Err(e) => {
                                let detail = e.to_string();
                                error!(error = %detail, "Background scheduler orchestrator error");
                                let _ = sc_tx_cron
                                    .send(aria_intelligence::CronCommand::UpdateStatus {
                                        id: ev.job_id.clone(),
                                        status: aria_intelligence::ScheduledJobStatus::Failed,
                                        detail: Some(detail),
                                        timestamp_us: chrono::Utc::now().timestamp_micros() as u64,
                                    })
                                    .await;
                                let _ = persist_scheduler_job_snapshot(
                                    &sc_tx_cron,
                                    sessions_dir,
                                    &ev.job_id,
                                )
                                .await;
                                let _ = RuntimeStore::for_sessions_dir(sessions_dir)
                                    .release_job_lease(&ev.job_id, &sc_worker_id);
                            }
                        }
                    }
                }
            }
        }
    });
    } else {
        info!(role = %shared_config.node.role, "Skipping background scheduler processor for node role");
    }

    let ar_config = Arc::clone(&shared_config);
    let ar_router_index = router_index.clone();
    let ar_embedder = Arc::clone(&embedder);
    let ar_llm_pool = Arc::clone(&llm_pool);
    let ar_cedar = Arc::clone(&cedar);
    let ar_agent_store = Arc::clone(&agent_store);
    let ar_tool_registry = Arc::clone(&tool_registry);
    let ar_session_memory = session_memory.clone();
    let ar_page_index = Arc::clone(&capability_index);
    let ar_vector_store = Arc::clone(&vector_store);
    let ar_keyword_index = Arc::clone(&keyword_index);
    let ar_firewall = Arc::clone(&firewall);
    let ar_vault = Arc::clone(&vault);
    let ar_tx_cron = tx_cron.clone();
    let ar_registry = Arc::clone(&registry);
    let ar_caches = Arc::clone(&session_tool_caches);
    let ar_hooks = Arc::clone(&hooks);
    let ar_locks = Arc::clone(&session_locks);
    let ar_semaphore = Arc::clone(&embed_semaphore);

    tokio::spawn(async move {
        info!("Background sub-agent processor started");
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            tick.tick().await;
            let sessions_dir = std::path::Path::new(&ar_config.ssmu.sessions_dir);
            match process_next_queued_agent_run(sessions_dir, |run| {
                let ar_config = Arc::clone(&ar_config);
                let ar_llm_pool = Arc::clone(&ar_llm_pool);
                let ar_cedar = Arc::clone(&ar_cedar);
                let ar_agent_store = Arc::clone(&ar_agent_store);
                let ar_tool_registry = Arc::clone(&ar_tool_registry);
                let ar_page_index = Arc::clone(&ar_page_index);
                let ar_vector_store = Arc::clone(&ar_vector_store);
                let ar_keyword_index = Arc::clone(&ar_keyword_index);
                let ar_firewall = Arc::clone(&ar_firewall);
                let ar_vault = Arc::clone(&ar_vault);
                let ar_registry = Arc::clone(&ar_registry);
                let ar_caches = Arc::clone(&ar_caches);
                let ar_hooks = Arc::clone(&ar_hooks);
                let ar_locks = Arc::clone(&ar_locks);
                let ar_semaphore = Arc::clone(&ar_semaphore);
                let ar_router_index = ar_router_index.clone();
                let ar_embedder = Arc::clone(&ar_embedder);
                let ar_session_memory = ar_session_memory.clone();
                let ar_tx_cron = ar_tx_cron.clone();
                async move {
                    let child_session_id = agent_run_session_id(&run.run_id);
                    let child_session_uuid = uuid::Uuid::from_bytes(child_session_id);
                    if let Err(err) = ar_session_memory.update_overrides(
                        child_session_uuid,
                        Some(run.agent_id.clone()),
                        None,
                    ) {
                        warn!(
                            session_id = %child_session_uuid,
                            agent_id = %run.agent_id,
                            error = %err,
                            "failed to persist child-run session override"
                        );
                    }
                    let req = aria_core::AgentRequest {
                        request_id: *uuid::Uuid::new_v4().as_bytes(),
                        session_id: child_session_id,
                        channel: aria_core::GatewayChannel::Unknown,
                        user_id: run.user_id.clone(),
                        content: aria_core::MessageContent::Text(run.request_text.clone()),
                        tool_runtime_policy: None,
                        timestamp_us: chrono::Utc::now().timestamp_micros() as u64,
                    };

                    match process_request(
                        &req,
                        &ar_config.learning,
                        &ar_router_index,
                        &*ar_embedder,
                        &ar_llm_pool,
                        &ar_cedar,
                        &*ar_agent_store,
                        &*ar_tool_registry,
                        &ar_session_memory,
                        &ar_page_index,
                        &ar_vector_store,
                        &ar_keyword_index,
                        &ar_firewall,
                        &ar_vault,
                        &ar_tx_cron,
                        &ar_registry,
                        ar_caches.as_ref(),
                        &*ar_hooks,
                        &ar_locks,
                        &ar_semaphore,
                        ar_config.llm.max_tool_rounds,
                        None,
                        Some(&Arc::new(std::sync::atomic::AtomicBool::new(false))),
                        sessions_dir,
                        ar_config.policy.whitelist.clone(),
                        ar_config.policy.forbid.clone(),
                        resolve_request_timezone(&ar_config, &run.user_id),
                    )
                    .await
                    {
                        Ok(aria_intelligence::OrchestratorResult::Completed(text)) => Ok(text),
                        Ok(aria_intelligence::OrchestratorResult::AgentElevationRequired {
                            message,
                            ..
                        }) => Err(message),
                        Ok(aria_intelligence::OrchestratorResult::ToolApprovalRequired {
                            call,
                            ..
                        }) => Err(format!(
                            "sub-agent requires approval for tool '{}'",
                            call.name
                        )),
                        Err(err) => Err(err.to_string()),
                    }
                }
            })
            .await
            {
                Ok(Some(run)) => {
                    info!(
                        run_id = %run.run_id,
                        agent_id = %run.agent_id,
                        status = ?run.status,
                        "Processed queued sub-agent run"
                    );
                }
                Ok(None) => {}
                Err(err) => {
                    error!(error = %err, "Failed to process queued sub-agent run");
                }
            }
        }
    });

    let enabled_adapters = configured_gateway_adapters(&shared_config.gateway);
    let telegram_enabled = enabled_adapters.iter().any(|adapter| adapter == "telegram");
    let cli_enabled = enabled_adapters.iter().any(|adapter| adapter == "cli");
    let websocket_enabled = enabled_adapters.iter().any(|adapter| adapter == "websocket");
    let whatsapp_enabled = enabled_adapters.iter().any(|adapter| adapter == "whatsapp");
    let health_store_dir = shared_config.ssmu.sessions_dir.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let snapshots = crate::channel_health::snapshot_channel_health();
            if snapshots.is_empty() {
                continue;
            }
            let now_us = chrono::Utc::now().timestamp_micros() as u64;
            let _ = RuntimeStore::for_sessions_dir(Path::new(&health_store_dir))
                .append_channel_health_snapshot(&snapshots, now_us);
            info!(channels = ?snapshots, "Channel runtime health snapshot");
        }
    });
    if shared_config.features.outbox_delivery
        && shared_config
            .rollout
            .feature_enabled_for_node(&shared_config.node.id, "outbox_delivery")
        && node_supports_outbound(&shared_config.node.role)
    {
        let retry_config = Arc::clone(&shared_config);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                match retry_failed_outbound_deliveries_once(&retry_config, 64).await {
                    Ok(recovered) if recovered > 0 => {
                        info!(recovered = recovered, "Recovered failed outbound deliveries");
                    }
                    Ok(_) => {}
                    Err(err) => {
                        warn!(error = %err, "Outbound retry worker failed");
                    }
                }
            }
        });
    } else if shared_config.features.outbox_delivery {
        info!(role = %shared_config.node.role, "Skipping outbound retry worker for non-outbound node role");
    }

    if !node_supports_ingress(&shared_config.node.role) {
        info!(role = %shared_config.node.role, "Ingress adapters disabled for non-ingress node role");
        return;
    }

    let mut adapter_handles = Vec::new();

    if !cli_enabled {
        if telegram_enabled {
            let tg_config = Arc::clone(&shared_config);
            let tg_runtime_config_path = runtime_config_path.clone();
            let tg_router_index = router_index.clone();
            let tg_embedder = Arc::clone(&embedder);
            let tg_llm_pool = Arc::clone(&llm_pool);
            let tg_cedar = Arc::clone(&cedar);
            let tg_agent_store = (*agent_store).clone();
            let tg_tool_registry = (*tool_registry).clone();
            let tg_session_memory = session_memory.clone();
            let tg_page_index = Arc::clone(&capability_index);
            let tg_vector_store = Arc::clone(&vector_store);
            let tg_keyword_index = Arc::clone(&keyword_index);
            let tg_caches = Arc::clone(&session_tool_caches);
            let tg_firewall = Arc::clone(&firewall);
            let tg_vault = Arc::clone(&vault);
            let tg_tx_cron = tx_cron.clone();
            let tg_registry = Arc::clone(&registry);
            let tg_shutdown = Arc::clone(&shutdown_coordinator);
            adapter_handles.push(spawn_supervised_adapter("telegram", GatewayChannel::Telegram, tg_shutdown, move || {
                run_telegram_gateway(
                    Arc::clone(&tg_config),
                    tg_runtime_config_path.clone(),
                    tg_router_index.clone(),
                    Arc::clone(&tg_embedder),
                    Arc::clone(&tg_llm_pool),
                    Arc::clone(&tg_cedar),
                    tg_agent_store.clone(),
                    tg_tool_registry.clone(),
                    tg_session_memory.clone(),
                    Arc::clone(&tg_page_index),
                    Arc::clone(&tg_vector_store),
                    Arc::clone(&tg_keyword_index),
                    Arc::clone(&tg_caches),
                    Arc::clone(&tg_firewall),
                    Arc::clone(&tg_vault),
                    tg_tx_cron.clone(),
                    Arc::clone(&tg_registry),
                )
            }));
        }
        if websocket_enabled {
            let ws_config = Arc::clone(&shared_config);
            let ws_router_index = router_index.clone();
            let ws_embedder = Arc::clone(&embedder);
            let ws_llm_pool = Arc::clone(&llm_pool);
            let ws_cedar = Arc::clone(&cedar);
            let ws_agent_store = (*agent_store).clone();
            let ws_tool_registry = (*tool_registry).clone();
            let ws_session_memory = session_memory.clone();
            let ws_page_index = Arc::clone(&capability_index);
            let ws_vector_store = Arc::clone(&vector_store);
            let ws_keyword_index = Arc::clone(&keyword_index);
            let ws_caches = Arc::clone(&session_tool_caches);
            let ws_firewall = Arc::clone(&firewall);
            let ws_vault = Arc::clone(&vault);
            let ws_tx_cron = tx_cron.clone();
            let ws_registry = Arc::clone(&registry);
            let ws_session_locks = Arc::clone(&session_locks);
            let ws_embed_semaphore = Arc::clone(&embed_semaphore);
            let ws_shutdown = Arc::clone(&shutdown_coordinator);
            adapter_handles.push(spawn_supervised_adapter("websocket", GatewayChannel::WebSocket, ws_shutdown, move || {
                run_websocket_gateway(
                    Arc::clone(&ws_config),
                    ws_router_index.clone(),
                    Arc::clone(&ws_embedder),
                    Arc::clone(&ws_llm_pool),
                    Arc::clone(&ws_cedar),
                    ws_agent_store.clone(),
                    ws_tool_registry.clone(),
                    ws_session_memory.clone(),
                    Arc::clone(&ws_page_index),
                    Arc::clone(&ws_vector_store),
                    Arc::clone(&ws_keyword_index),
                    Arc::clone(&ws_caches),
                    Arc::clone(&ws_firewall),
                    Arc::clone(&ws_vault),
                    ws_tx_cron.clone(),
                    Arc::clone(&ws_registry),
                    Arc::clone(&ws_session_locks),
                    Arc::clone(&ws_embed_semaphore),
                )
            }));
        }
        if whatsapp_enabled {
            let wa_config = Arc::clone(&shared_config);
            let wa_router_index = router_index.clone();
            let wa_embedder = Arc::clone(&embedder);
            let wa_llm_pool = Arc::clone(&llm_pool);
            let wa_cedar = Arc::clone(&cedar);
            let wa_agent_store = (*agent_store).clone();
            let wa_tool_registry = (*tool_registry).clone();
            let wa_session_memory = session_memory.clone();
            let wa_page_index = Arc::clone(&capability_index);
            let wa_vector_store = Arc::clone(&vector_store);
            let wa_keyword_index = Arc::clone(&keyword_index);
            let wa_caches = Arc::clone(&session_tool_caches);
            let wa_firewall = Arc::clone(&firewall);
            let wa_vault = Arc::clone(&vault);
            let wa_tx_cron = tx_cron.clone();
            let wa_registry = Arc::clone(&registry);
            let wa_session_locks = Arc::clone(&session_locks);
            let wa_embed_semaphore = Arc::clone(&embed_semaphore);
            let wa_shutdown = Arc::clone(&shutdown_coordinator);
            adapter_handles.push(spawn_supervised_adapter("whatsapp", GatewayChannel::WhatsApp, wa_shutdown, move || {
                run_whatsapp_gateway(
                    Arc::clone(&wa_config),
                    wa_router_index.clone(),
                    Arc::clone(&wa_embedder),
                    Arc::clone(&wa_llm_pool),
                    Arc::clone(&wa_cedar),
                    wa_agent_store.clone(),
                    wa_tool_registry.clone(),
                    wa_session_memory.clone(),
                    Arc::clone(&wa_page_index),
                    Arc::clone(&wa_vector_store),
                    Arc::clone(&wa_keyword_index),
                    Arc::clone(&wa_caches),
                    Arc::clone(&wa_firewall),
                    Arc::clone(&wa_vault),
                    wa_tx_cron.clone(),
                    Arc::clone(&wa_registry),
                    Arc::clone(&wa_session_locks),
                    Arc::clone(&wa_embed_semaphore),
                )
            }));
        }

        if telegram_enabled || websocket_enabled || whatsapp_enabled {
            wait_for_runtime_shutdown(Arc::clone(&shutdown_coordinator)).await;
            for handle in adapter_handles {
                let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
            }
            drop(runtime_pid_guard);
            return;
        }
    }

    if telegram_enabled {
        let tg_config = Arc::clone(&shared_config);
        let tg_runtime_config_path = runtime_config_path.clone();
        let tg_router_index = router_index.clone();
        let tg_embedder = Arc::clone(&embedder);
        let tg_llm_pool = Arc::clone(&llm_pool);
        let tg_cedar = Arc::clone(&cedar);
        let tg_agent_store = (*agent_store).clone();
        let tg_tool_registry = (*tool_registry).clone();
        let tg_session_memory = session_memory.clone();
        let tg_page_index = Arc::clone(&capability_index);
        let tg_vector_store = Arc::clone(&vector_store);
        let tg_keyword_index = Arc::clone(&keyword_index);
        let tg_caches = Arc::clone(&session_tool_caches);
        let tg_firewall = Arc::clone(&firewall);
        let tg_vault = Arc::clone(&vault);
        let tg_tx_cron = tx_cron.clone();
        let tg_registry = Arc::clone(&registry);
        let tg_shutdown = Arc::clone(&shutdown_coordinator);
        adapter_handles.push(spawn_supervised_adapter("telegram", GatewayChannel::Telegram, tg_shutdown, move || {
            run_telegram_gateway(
                Arc::clone(&tg_config),
                tg_runtime_config_path.clone(),
                tg_router_index.clone(),
                Arc::clone(&tg_embedder),
                Arc::clone(&tg_llm_pool),
                Arc::clone(&tg_cedar),
                tg_agent_store.clone(),
                tg_tool_registry.clone(),
                tg_session_memory.clone(),
                Arc::clone(&tg_page_index),
                Arc::clone(&tg_vector_store),
                Arc::clone(&tg_keyword_index),
                Arc::clone(&tg_caches),
                Arc::clone(&tg_firewall),
                Arc::clone(&tg_vault),
                tg_tx_cron.clone(),
                Arc::clone(&tg_registry),
            )
        }));
    }

    if websocket_enabled {
        let ws_config = Arc::clone(&shared_config);
        let ws_router_index = router_index.clone();
        let ws_embedder = Arc::clone(&embedder);
        let ws_llm_pool = Arc::clone(&llm_pool);
        let ws_cedar = Arc::clone(&cedar);
        let ws_agent_store = (*agent_store).clone();
        let ws_tool_registry = (*tool_registry).clone();
        let ws_session_memory = session_memory.clone();
        let ws_page_index = Arc::clone(&capability_index);
        let ws_vector_store = Arc::clone(&vector_store);
        let ws_keyword_index = Arc::clone(&keyword_index);
        let ws_caches = Arc::clone(&session_tool_caches);
        let ws_firewall = Arc::clone(&firewall);
        let ws_vault = Arc::clone(&vault);
        let ws_tx_cron = tx_cron.clone();
        let ws_registry = Arc::clone(&registry);
        let ws_session_locks = Arc::clone(&session_locks);
        let ws_embed_semaphore = Arc::clone(&embed_semaphore);
        let ws_shutdown = Arc::clone(&shutdown_coordinator);
        adapter_handles.push(spawn_supervised_adapter("websocket", GatewayChannel::WebSocket, ws_shutdown, move || {
            run_websocket_gateway(
                Arc::clone(&ws_config),
                ws_router_index.clone(),
                Arc::clone(&ws_embedder),
                Arc::clone(&ws_llm_pool),
                Arc::clone(&ws_cedar),
                ws_agent_store.clone(),
                ws_tool_registry.clone(),
                ws_session_memory.clone(),
                Arc::clone(&ws_page_index),
                Arc::clone(&ws_vector_store),
                Arc::clone(&ws_keyword_index),
                Arc::clone(&ws_caches),
                Arc::clone(&ws_firewall),
                Arc::clone(&ws_vault),
                ws_tx_cron.clone(),
                Arc::clone(&ws_registry),
                Arc::clone(&ws_session_locks),
                Arc::clone(&ws_embed_semaphore),
            )
        }));
    }

    if whatsapp_enabled {
        let wa_config = Arc::clone(&shared_config);
        let wa_router_index = router_index.clone();
        let wa_embedder = Arc::clone(&embedder);
        let wa_llm_pool = Arc::clone(&llm_pool);
        let wa_cedar = Arc::clone(&cedar);
        let wa_agent_store = (*agent_store).clone();
        let wa_tool_registry = (*tool_registry).clone();
        let wa_session_memory = session_memory.clone();
        let wa_page_index = Arc::clone(&capability_index);
        let wa_vector_store = Arc::clone(&vector_store);
        let wa_keyword_index = Arc::clone(&keyword_index);
        let wa_caches = Arc::clone(&session_tool_caches);
        let wa_firewall = Arc::clone(&firewall);
        let wa_vault = Arc::clone(&vault);
        let wa_tx_cron = tx_cron.clone();
        let wa_registry = Arc::clone(&registry);
        let wa_session_locks = Arc::clone(&session_locks);
        let wa_embed_semaphore = Arc::clone(&embed_semaphore);
        let wa_shutdown = Arc::clone(&shutdown_coordinator);
        adapter_handles.push(spawn_supervised_adapter("whatsapp", GatewayChannel::WhatsApp, wa_shutdown, move || {
            run_whatsapp_gateway(
                Arc::clone(&wa_config),
                wa_router_index.clone(),
                Arc::clone(&wa_embedder),
                Arc::clone(&wa_llm_pool),
                Arc::clone(&wa_cedar),
                wa_agent_store.clone(),
                wa_tool_registry.clone(),
                wa_session_memory.clone(),
                Arc::clone(&wa_page_index),
                Arc::clone(&wa_vector_store),
                Arc::clone(&wa_keyword_index),
                Arc::clone(&wa_caches),
                Arc::clone(&wa_firewall),
                Arc::clone(&wa_vault),
                wa_tx_cron.clone(),
                Arc::clone(&wa_registry),
                Arc::clone(&wa_session_locks),
                Arc::clone(&wa_embed_semaphore),
            )
        }));
    }

    if !cli_enabled {
        error!(
            adapters = ?enabled_adapters,
            "No supported foreground gateway adapter enabled. Supported now: cli, telegram, websocket, whatsapp"
        );
        return;
    }

    // Wire Adapters — CLI mode
    let gateway = CliGateway;
    const CLI_INGRESS_QUEUE_CAPACITY: usize = 256;
    const CLI_INGRESS_PARTITIONS: usize = 4;
    let (ingress_bridge, ingress_receivers, ingress_metrics) =
        PartitionedIngressQueueBridge::<AgentRequest>::new(
            CLI_INGRESS_PARTITIONS,
            CLI_INGRESS_QUEUE_CAPACITY,
        );

    let worker_shared_config = Arc::clone(&shared_config);
    let worker_router_index = router_index.clone();
    let worker_embedder = Arc::clone(&embedder);
    let worker_llm_pool = Arc::clone(&llm_pool);
    let worker_cedar = Arc::clone(&cedar);
    let worker_agent_store = (*agent_store).clone();
    let worker_tool_registry = (*tool_registry).clone();
    let worker_session_memory = session_memory.clone();
    let worker_page_index = Arc::clone(&capability_index);
    let worker_vector_store = Arc::clone(&vector_store);
    let worker_keyword_index = Arc::clone(&keyword_index);
    let worker_firewall = Arc::clone(&firewall);
    let worker_vault = Arc::clone(&vault);
    let worker_tx_cron = tx_cron.clone();
    let worker_registry = Arc::clone(&registry);
    let worker_session_tool_caches = Arc::clone(&session_tool_caches);
    let worker_hooks = Arc::clone(&hooks);
    let worker_session_locks = Arc::clone(&session_locks);
    let worker_embed_semaphore = Arc::clone(&embed_semaphore);
    let mut cli_ingress_workers = Vec::new();
    for (lane_idx, mut ingress_rx) in ingress_receivers.into_iter().enumerate() {
        let ingress_bridge_worker = ingress_bridge.lane(lane_idx);
        let worker_shared_config = Arc::clone(&worker_shared_config);
        let worker_router_index = worker_router_index.clone();
        let worker_embedder = Arc::clone(&worker_embedder);
        let worker_llm_pool = Arc::clone(&worker_llm_pool);
        let worker_cedar = Arc::clone(&worker_cedar);
        let worker_agent_store = worker_agent_store.clone();
        let worker_tool_registry = worker_tool_registry.clone();
        let worker_session_memory = worker_session_memory.clone();
        let worker_page_index = Arc::clone(&worker_page_index);
        let worker_vector_store = Arc::clone(&worker_vector_store);
        let worker_keyword_index = Arc::clone(&worker_keyword_index);
        let worker_firewall = Arc::clone(&worker_firewall);
        let worker_vault = Arc::clone(&worker_vault);
        let worker_tx_cron = worker_tx_cron.clone();
        let worker_registry = Arc::clone(&worker_registry);
        let worker_session_tool_caches = Arc::clone(&worker_session_tool_caches);
        let worker_hooks = Arc::clone(&worker_hooks);
        let worker_session_locks = Arc::clone(&worker_session_locks);
        let worker_embed_semaphore = Arc::clone(&worker_embed_semaphore);
        cli_ingress_workers.push(tokio::spawn(async move {
            while let Some(req) = ingress_rx.recv().await {
                ingress_bridge_worker.mark_dequeued();
                crate::channel_health::record_channel_health_event(
                    req.channel,
                    crate::channel_health::ChannelHealthEventKind::IngressDequeued,
                );
                process_cli_ingress_request(
                    &req,
                    &worker_shared_config,
                    &worker_router_index,
                    worker_embedder.as_ref(),
                    &worker_llm_pool,
                    &worker_cedar,
                    &worker_agent_store,
                    &worker_tool_registry,
                    &worker_session_memory,
                    &worker_page_index,
                    &worker_vector_store,
                    &worker_keyword_index,
                    &worker_firewall,
                    &worker_vault,
                    &worker_tx_cron,
                    &worker_registry,
                    worker_session_tool_caches.as_ref(),
                    worker_hooks.as_ref(),
                    &worker_session_locks,
                    &worker_embed_semaphore,
                )
                .await;
            }
        }));
    }

    info!("All subsystems wired (Gateway → Router → Orchestrator → Exec)");
    info!("Interactive CLI started (press Ctrl+C or send SIGTERM to exit)");

    let shutdown = async {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = sigterm.recv() => {
                        info!("Received SIGTERM — shutting down gracefully");
                    }
                }
            } else {
                tokio::signal::ctrl_c().await.ok();
            }
        }
        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c().await.ok();
        }
    };
    tokio::pin!(shutdown);
    loop {
        let req = tokio::select! {
            _ = &mut shutdown => {
                shutdown_coordinator.signal_shutdown();
                break;
            }
            req_res = gateway.receive() => {
                match req_res {
                    Ok(r) => {
                        let request_text = request_text_from_content(&r.content);
                        if request_text.eq_ignore_ascii_case("exit") {
                            info!("Exiting...");
                            shutdown_coordinator.signal_shutdown();
                            break;
                        }
                        r
                    },
                    Err(_) => continue,
                }
            }
        };
        let mut req = req;
        apply_session_scope_policy(&mut req, &shared_config);

        if let Some(reply) = handle_cli_approval_command(
            &req,
            &shared_config,
            &session_memory,
            &vault,
            &cedar,
            &tx_cron,
        )
        .await
        {
            send_universal_response(&req, &reply, &shared_config).await;
            continue;
        }

        if let Some(output) = handle_runtime_control_command(
            &req,
            &shared_config,
            &llm_pool,
            &session_memory,
            None,
        )
        .await
        {
            send_universal_response(&req, &output.text, &shared_config).await;
            continue;
        }

        if let Some(reply) = handle_cli_control_command(
            &req,
            &shared_config,
            &llm_pool,
            &*agent_store,
            &session_memory,
        ) {
            send_universal_response(&req, &reply, &shared_config).await;
            continue;
        }

        let key = req.session_id;
        if ingress_bridge.try_enqueue_by_key(req, &key).is_err() {
            crate::channel_health::record_channel_health_event(
                GatewayChannel::Cli,
                crate::channel_health::ChannelHealthEventKind::IngressDropped,
            );
            warn!(
                queue_depth = ingress_metrics
                    .iter()
                    .map(|metrics| metrics.queue_depth.load(Ordering::Relaxed))
                    .sum::<usize>(),
                "CLI ingress queue full/closed; dropping request"
            );
        } else {
            crate::channel_health::record_channel_health_event(
                GatewayChannel::Cli,
                crate::channel_health::ChannelHealthEventKind::IngressEnqueued,
            );
        }
    }

    drop(ingress_bridge);
    for worker in cli_ingress_workers {
        let _ = worker.await;
    }
    shutdown_coordinator.signal_shutdown();
    for handle in adapter_handles {
        let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
    }

    // Cleanup
    if let Ok(saved) = session_memory.save_to_sqlite(session_runtime_db_path(Path::new(
        &shared_config.ssmu.sessions_dir,
    ))) {
        info!(saved = saved, "Persisted sessions");
    }
    drop(session_memory);
    drop(capability_index);
    drop(vector_store);
    drop(router);
    drop(runtime_pid_guard);
    info!("Shutdown complete. Goodbye!");
}

fn run_channel_onboarding_command(
    config_path: &Path,
    args: &[String],
) -> Option<Result<String, String>> {
    if args.len() < 3 || args.get(1).map(String::as_str) != Some("channels") {
        return None;
    }
    let subcommand = args.get(2).map(String::as_str).unwrap_or_default();
    Some(match subcommand {
        "list" => list_configured_channels(config_path),
        "status" => list_channel_status(config_path),
        "add" => match args.get(3) {
            Some(channel) => add_configured_channel(config_path, channel),
            None => Err("Usage: channels add <channel>".into()),
        },
        "remove" => match args.get(3) {
            Some(channel) => remove_configured_channel(config_path, channel),
            None => Err("Usage: channels remove <channel>".into()),
        },
        _ => Err("Usage: channels <add|list|status|remove> [channel]".into()),
    })
}

fn runtime_pid_path() -> Result<PathBuf, String> {
    let dirs = project_dirs().ok_or_else(|| "unable to resolve application data directory".to_string())?;
    let run_dir = dirs.data_local_dir().join("run");
    std::fs::create_dir_all(&run_dir)
        .map_err(|e| format!("create runtime dir '{}' failed: {}", run_dir.display(), e))?;
    Ok(run_dir.join("aria-x.pid"))
}

struct RuntimePidGuard {
    path: PathBuf,
}

impl Drop for RuntimePidGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn register_runtime_pid(config_path: &Path) -> Result<RuntimePidGuard, String> {
    let path = runtime_pid_path()?;
    let record = RuntimePidRecord {
        pid: std::process::id(),
        config_path: config_path.display().to_string(),
    };
    let json = serde_json::to_string(&record)
        .map_err(|e| format!("serialize runtime pid record failed: {}", e))?;
    std::fs::write(&path, json)
        .map_err(|e| format!("write runtime pid file '{}' failed: {}", path.display(), e))?;
    Ok(RuntimePidGuard { path })
}

fn read_runtime_pid_record() -> Result<Option<RuntimePidRecord>, String> {
    let path = runtime_pid_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("read runtime pid file '{}' failed: {}", path.display(), e))?;
    let record = serde_json::from_str::<RuntimePidRecord>(&content)
        .map_err(|e| format!("parse runtime pid file '{}' failed: {}", path.display(), e))?;
    Ok(Some(record))
}

fn process_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

fn run_process_control_command(args: &[String]) -> Option<Result<String, String>> {
    match args.get(1).map(String::as_str) {
        Some("status") => Some(render_runtime_status()),
        Some("stop") => Some(stop_runtime_process()),
        _ => None,
    }
}

fn render_runtime_status() -> Result<String, String> {
    let Some(record) = read_runtime_pid_record()? else {
        return Ok("HiveClaw is not running.".into());
    };
    if process_is_alive(record.pid) {
        Ok(format!(
            "HiveClaw is running.\npid: {}\nconfig: {}",
            record.pid, record.config_path
        ))
    } else {
        let path = runtime_pid_path()?;
        let _ = std::fs::remove_file(&path);
        Ok(format!(
            "HiveClaw is not running, but a stale pid file was found and removed.\nstale_pid: {}\nconfig: {}",
            record.pid, record.config_path
        ))
    }
}

fn stop_runtime_process() -> Result<String, String> {
    let Some(record) = read_runtime_pid_record()? else {
        return Ok("HiveClaw is not running.".into());
    };
    if !process_is_alive(record.pid) {
        let path = runtime_pid_path()?;
        let _ = std::fs::remove_file(&path);
        return Ok(format!(
            "HiveClaw was not running. Removed stale pid file for pid {}.",
            record.pid
        ));
    }
    #[cfg(unix)]
    {
        let status = std::process::Command::new("kill")
            .arg("-TERM")
            .arg(record.pid.to_string())
            .status()
            .map_err(|e| format!("failed to send SIGTERM to {}: {}", record.pid, e))?;
        if !status.success() {
            return Err(format!(
                "failed to stop pid {} using SIGTERM (status: {})",
                record.pid, status
            ));
        }
        Ok(format!("Sent SIGTERM to HiveClaw process {}.", record.pid))
    }
    #[cfg(not(unix))]
    {
        let _ = record;
        Err("`hiveclaw stop` is currently only implemented on Unix-like systems.".into())
    }
}

async fn wait_for_runtime_shutdown(shutdown: Arc<ShutdownCoordinator>) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("Received Ctrl+C — shutting down gracefully");
                }
                _ = sigterm.recv() => {
                    info!("Received SIGTERM — shutting down gracefully");
                }
            }
        } else {
            tokio::signal::ctrl_c().await.ok();
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.ok();
    }
    shutdown.signal_shutdown();
}

fn setup_ssh_backend_cli(
    config: &ResolvedAppConfig,
    args: &[String],
) -> Result<String, String> {
    setup_ssh_backend_cli_impl(config, args)
}

fn run_stt_management_command(
    config: &ResolvedAppConfig,
    args: &[String],
) -> Option<Result<String, String>> {
    match (args.get(1).map(String::as_str), args.get(2).map(String::as_str)) {
        (Some("doctor"), None) => Some(Ok(render_doctor_summary(config))),
        (Some("doctor"), Some("stt")) => Some(Ok(render_stt_doctor(config))),
        (Some("doctor"), Some("env")) => Some(Ok(render_env_doctor(config))),
        (Some("doctor"), Some("gateway")) => Some(Ok(render_gateway_doctor(config))),
        (Some("doctor"), Some("browser")) => Some(Ok(render_browser_doctor(config))),
        (Some("doctor"), Some("mcp")) => {
            let live = args.iter().any(|arg| arg == "--live" || arg == "live");
            let live_mode = args
                .windows(2)
                .find_map(|window| (window[0] == "--mode").then(|| window[1].clone()));
            Some(Ok(render_mcp_doctor(
                config,
                live,
                live_mode.as_deref(),
            )))
        }
        (Some("setup"), Some("stt")) => {
            let wants_local = args.iter().any(|arg| arg == "--local" || arg == "local");
            Some(if wants_local {
                setup_local_stt_env(config)
            } else {
                Err("Usage: hiveclaw setup stt --local".into())
            })
        }
        (Some("setup"), Some("chrome-devtools-mcp")) => {
            Some(setup_chrome_devtools_mcp_cli(config, args))
        }
        (Some("setup"), Some("ssh-backend")) => Some(setup_ssh_backend_cli(config, args)),
        _ => None,
    }
}

fn run_replay_management_command(
    config: &ResolvedAppConfig,
    args: &[String],
) -> Option<Result<String, String>> {
    match (args.get(1).map(String::as_str), args.get(2).map(String::as_str)) {
        (Some("replay"), Some("golden")) => Some(run_golden_replay_cli(config, args)),
        (Some("replay"), Some("contracts")) => Some(run_contract_regression_cli(config)),
        (Some("replay"), Some("providers")) => Some(run_provider_benchmark_cli(config, args)),
        (Some("replay"), Some("gate")) => Some(run_release_gate_cli(config, args)),
        _ => None,
    }
}

fn run_telemetry_management_command(
    config: &ResolvedAppConfig,
    args: &[String],
) -> Option<Result<String, String>> {
    match (args.get(1).map(String::as_str), args.get(2).map(String::as_str)) {
        (Some("telemetry"), Some("export")) => Some(run_telemetry_export_cli(config, args)),
        _ => None,
    }
}

fn run_telemetry_export_cli(config: &ResolvedAppConfig, args: &[String]) -> Result<String, String> {
    let mut scope = TelemetryExportScope::Local;
    let mut output_dir = None;
    let mut idx = 3usize;
    while let Some(arg) = args.get(idx) {
        match arg.as_str() {
            "--scope" => {
                let value = args
                    .get(idx + 1)
                    .ok_or_else(|| {
                        "Usage: hiveclaw telemetry export [--scope <local|shared>] [--output-dir <path>]".to_string()
                    })?;
                scope = match value.as_str() {
                    "local" => TelemetryExportScope::Local,
                    "shared" => TelemetryExportScope::Shared,
                    _ => {
                        return Err(
                            "Usage: hiveclaw telemetry export [--scope <local|shared>] [--output-dir <path>]".into(),
                        )
                    }
                };
                idx += 2;
            }
            "--output-dir" => {
                let value = args
                    .get(idx + 1)
                    .ok_or_else(|| {
                        "Usage: hiveclaw telemetry export [--scope <local|shared>] [--output-dir <path>]".to_string()
                    })?;
                output_dir = Some(PathBuf::from(value));
                idx += 2;
            }
            _ => {
                return Err(
                    "Usage: hiveclaw telemetry export [--scope <local|shared>] [--output-dir <path>]".into(),
                )
            }
        }
    }
    export_telemetry_bundle(config, scope, output_dir.as_deref())
}

fn run_golden_replay_cli(config: &ResolvedAppConfig, args: &[String]) -> Result<String, String> {
    let suite_path = args
        .get(3)
        .ok_or_else(|| "Usage: hiveclaw replay golden <suite.toml>".to_string())?;
    let suite_body = std::fs::read_to_string(suite_path)
        .map_err(|e| format!("read golden replay suite failed: {}", e))?;
    let suite: GoldenReplaySuite = toml::from_str(&suite_body)
        .map_err(|e| format!("parse golden replay suite failed: {}", e))?;
    let report = evaluate_golden_replay_suite(
        &RuntimeStore::for_sessions_dir(Path::new(&config.ssmu.sessions_dir)),
        &suite,
    )?;
    if report.failed_count > 0 {
        return Err(render_golden_replay_report(&report));
    }
    Ok(render_golden_replay_report(&report))
}

fn run_contract_regression_cli(config: &ResolvedAppConfig) -> Result<String, String> {
    let report = evaluate_contract_regression_suite(Path::new(&config.ssmu.sessions_dir))?;
    if report.failed_count > 0 {
        return Err(render_contract_regression_report(&report));
    }
    Ok(render_contract_regression_report(&report))
}

fn evaluate_golden_replay_suite(
    store: &RuntimeStore,
    suite: &GoldenReplaySuite,
) -> Result<GoldenReplayReport, String> {
    let mut results = Vec::with_capacity(suite.scenarios.len());
    for scenario in &suite.scenarios {
        let samples = store.build_replay_samples_for_fingerprint(&scenario.task_fingerprint)?;
        let latest = samples
            .iter()
            .max_by_key(|sample| sample.trace.recorded_at_us);
        let mut reasons = Vec::new();

        if samples.len() < scenario.min_samples {
            reasons.push(format!(
                "expected at least {} sample(s), found {}",
                scenario.min_samples,
                samples.len()
            ));
        }

        if let Some(sample) = latest {
            if sample.trace.outcome != scenario.expected_outcome {
                reasons.push(format!(
                    "expected outcome {:?}, got {:?}",
                    scenario.expected_outcome, sample.trace.outcome
                ));
            }
            for tool in &scenario.required_tools {
                if !sample.trace.tool_names.iter().any(|name| name == tool) {
                    reasons.push(format!("missing required tool '{}'", tool));
                }
            }
            for needle in &scenario.response_must_contain {
                if !sample.trace.response_summary.contains(needle) {
                    reasons.push(format!(
                        "response summary missing substring '{}'",
                        needle
                    ));
                }
            }
            if let Some(min_reward_score) = scenario.min_reward_score {
                if sample.reward_score < min_reward_score {
                    reasons.push(format!(
                        "expected reward score >= {}, got {}",
                        min_reward_score, sample.reward_score
                    ));
                }
            }
        } else {
            reasons.push("no replay samples found".into());
        }

        results.push(GoldenReplayScenarioResult {
            id: scenario.id.clone(),
            task_fingerprint: scenario.task_fingerprint.clone(),
            sample_count: samples.len(),
            latest_request_id: latest.map(|sample| sample.trace.request_id.clone()),
            latest_outcome: latest.map(|sample| sample.trace.outcome),
            latest_reward_score: latest.map(|sample| sample.reward_score),
            passed: reasons.is_empty(),
            reasons,
        });
    }

    Ok(GoldenReplayReport {
        scenario_count: results.len(),
        passed_count: results.iter().filter(|result| result.passed).count(),
        failed_count: results.iter().filter(|result| !result.passed).count(),
        results,
    })
}

fn default_contract_regression_scenarios() -> Vec<ContractRegressionScenario> {
    vec![
        ContractRegressionScenario {
            id: "artifact-create-file",
            request_text: "Create a hello.js file with console.log('hi')",
            expected_kind: aria_core::ExecutionContractKind::ArtifactCreate,
            expected_required_artifacts: vec![aria_core::ExecutionArtifactKind::File],
            expected_required_tools: vec!["write_file"],
            expected_approval_required: true,
            expected_tool_choice: Some("specific:write_file"),
            satisfied_tool_names: vec!["write_file"],
            expected_plain_text_failure: Some(
                aria_core::ContractFailureReason::MissingRequiredArtifact,
            ),
            approval_probe: Some(ContractApprovalProbe {
                tool_name: "write_file",
                arguments_json: serde_json::json!({
                    "path": "./hello.js",
                    "content": "console.log('hi');"
                }),
            }),
        },
        ContractRegressionScenario {
            id: "schedule-create-reminder",
            request_text: "Set a reminder in 2 minutes to stretch",
            expected_kind: aria_core::ExecutionContractKind::ScheduleCreate,
            expected_required_artifacts: vec![aria_core::ExecutionArtifactKind::Schedule],
            expected_required_tools: vec!["schedule_message"],
            expected_approval_required: false,
            expected_tool_choice: Some("specific:schedule_message"),
            satisfied_tool_names: vec!["schedule_message"],
            expected_plain_text_failure: Some(
                aria_core::ContractFailureReason::MissingRequiredArtifact,
            ),
            approval_probe: None,
        },
        ContractRegressionScenario {
            id: "browser-read-screenshot",
            request_text: "Open https://example.com and take a screenshot",
            expected_kind: aria_core::ExecutionContractKind::BrowserRead,
            expected_required_artifacts: vec![aria_core::ExecutionArtifactKind::Browser],
            expected_required_tools: vec!["browser_screenshot"],
            expected_approval_required: false,
            expected_tool_choice: Some("required"),
            satisfied_tool_names: vec!["browser_screenshot"],
            expected_plain_text_failure: Some(
                aria_core::ContractFailureReason::MissingRequiredArtifact,
            ),
            approval_probe: None,
        },
        ContractRegressionScenario {
            id: "browser-act-click",
            request_text: "Open https://example.com and click login",
            expected_kind: aria_core::ExecutionContractKind::BrowserAct,
            expected_required_artifacts: vec![aria_core::ExecutionArtifactKind::Browser],
            expected_required_tools: vec!["browser_act"],
            expected_approval_required: false,
            expected_tool_choice: Some("specific:browser_act"),
            satisfied_tool_names: vec!["browser_act"],
            expected_plain_text_failure: Some(
                aria_core::ContractFailureReason::MissingRequiredArtifact,
            ),
            approval_probe: None,
        },
        ContractRegressionScenario {
            id: "mcp-invoke",
            request_text: "Invoke mcp tool list_pages on server chrome_devtools",
            expected_kind: aria_core::ExecutionContractKind::McpInvoke,
            expected_required_artifacts: vec![aria_core::ExecutionArtifactKind::Mcp],
            expected_required_tools: vec!["invoke_mcp_tool"],
            expected_approval_required: false,
            expected_tool_choice: None,
            satisfied_tool_names: vec!["invoke_mcp_tool"],
            expected_plain_text_failure: Some(
                aria_core::ContractFailureReason::MissingRequiredArtifact,
            ),
            approval_probe: None,
        },
    ]
}

fn evaluate_contract_regression_suite(
    sessions_dir: &Path,
) -> Result<ContractRegressionReport, String> {
    evaluate_contract_regression_scenarios(sessions_dir, &default_contract_regression_scenarios())
}

fn evaluate_contract_regression_scenarios(
    sessions_dir: &Path,
    scenarios: &[ContractRegressionScenario],
) -> Result<ContractRegressionReport, String> {
    let mut results = Vec::with_capacity(scenarios.len());
    let now = chrono::Utc::now().with_timezone(&chrono_tz::Asia::Kolkata);
    for scenario in scenarios {
        let scheduling_intent = classify_scheduling_intent(scenario.request_text, now);
        let contract = resolve_execution_contract(scenario.request_text, scheduling_intent.as_ref());
        let mut reasons = Vec::new();
        if contract.kind != scenario.expected_kind {
            reasons.push(format!(
                "expected contract {:?}, got {:?}",
                scenario.expected_kind, contract.kind
            ));
        }
        if contract.required_artifact_kinds != scenario.expected_required_artifacts {
            reasons.push(format!(
                "expected artifact kinds {:?}, got {:?}",
                scenario.expected_required_artifacts, contract.required_artifact_kinds
            ));
        }
        if contract.approval_required != scenario.expected_approval_required {
            reasons.push(format!(
                "expected approval_required={}, got {}",
                scenario.expected_approval_required, contract.approval_required
            ));
        }

        let required_tools = required_runtime_tool_names_for_contract(&contract)
            .iter()
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>();
        for expected in &scenario.expected_required_tools {
            if !required_tools.iter().any(|tool| tool == expected) {
                reasons.push(format!("required tools missing '{}'", expected));
            }
        }

        let req = AgentRequest {
            request_id: [1; 16],
            session_id: [2; 16],
            channel: GatewayChannel::Cli,
            user_id: "contract-regression".into(),
            content: MessageContent::Text(scenario.request_text.to_string()),
            tool_runtime_policy: None,
            timestamp_us: 1,
        };
        let policy = effective_tool_runtime_policy_for_request(
            &req,
            scenario.request_text,
            scheduling_intent.as_ref(),
            &contract,
        );
        let tool_choice = policy
            .as_ref()
            .map(|policy| render_tool_choice_policy(&policy.tool_choice));
        if let Some(expected_choice) = scenario.expected_tool_choice {
            if tool_choice.as_deref() != Some(expected_choice) {
                reasons.push(format!(
                    "expected tool choice '{}', got {:?}",
                    expected_choice, tool_choice
                ));
            }
        }

        let satisfied_artifacts = infer_execution_artifacts(
            &scenario
                .satisfied_tool_names
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>(),
            "",
        );
        if let Err(reason) = validate_execution_contract(&contract, &satisfied_artifacts) {
            reasons.push(format!(
                "happy-path artifacts failed contract validation: {:?}",
                reason
            ));
        }

        if let Some(expected_failure) = scenario.expected_plain_text_failure {
            let plain_artifacts = infer_execution_artifacts(&[], "plain-text completion");
            match validate_execution_contract(&contract, &plain_artifacts) {
                Ok(_) => reasons.push("plain-text completion unexpectedly satisfied contract".into()),
                Err(actual) if actual != expected_failure => reasons.push(format!(
                    "expected plain-text failure {:?}, got {:?}",
                    expected_failure, actual
                )),
                Err(_) => {}
            }
        }

        if let Some(probe) = &scenario.approval_probe {
            let req = AgentRequest {
                request_id: *uuid::Uuid::new_v4().as_bytes(),
                session_id: *uuid::Uuid::new_v4().as_bytes(),
                channel: GatewayChannel::Cli,
                user_id: "contract-regression".into(),
                content: MessageContent::Text(scenario.request_text.to_string()),
                tool_runtime_policy: policy.clone(),
                timestamp_us: 1,
            };
            let call = ToolCall {
                invocation_id: None,
                name: probe.tool_name.to_string(),
                arguments: probe.arguments_json.to_string(),
            };
            let approval_sessions = sessions_dir.join(format!(
                "contract-regression-approval-{}",
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir_all(&approval_sessions).map_err(|e| {
                format!(
                    "create approval probe session dir '{}' failed: {}",
                    approval_sessions.display(),
                    e
                )
            })?;
            match persist_pending_approval_for_tool_error(
                &approval_sessions,
                &req,
                &call,
                &format!("{}::{}", aria_intelligence::APPROVAL_REQUIRED_PREFIX, probe.tool_name),
            ) {
                Ok((record, _)) if record.tool_name != probe.tool_name => reasons.push(format!(
                    "approval probe stored wrong tool '{}'",
                    record.tool_name
                )),
                Ok(_) => {}
                Err(err) => reasons.push(format!("approval probe failed: {}", err)),
            }
        }

        results.push(ContractRegressionScenarioResult {
            id: scenario.id.to_string(),
            contract_kind: contract.kind,
            passed: reasons.is_empty(),
            reasons,
            required_tools,
            tool_choice,
        });
    }
    let passed_count = results.iter().filter(|result| result.passed).count();
    Ok(ContractRegressionReport {
        scenario_count: results.len(),
        passed_count,
        failed_count: results.len().saturating_sub(passed_count),
        results,
    })
}

fn render_tool_choice_policy(policy: &aria_core::ToolChoicePolicy) -> String {
    match policy {
        aria_core::ToolChoicePolicy::Auto => "auto".into(),
        aria_core::ToolChoicePolicy::None => "none".into(),
        aria_core::ToolChoicePolicy::Required => "required".into(),
        aria_core::ToolChoicePolicy::Specific(name) => format!("specific:{}", name),
    }
}

fn render_golden_replay_report(report: &GoldenReplayReport) -> String {
    let mut lines = vec![
        "Golden replay report".to_string(),
        format!("scenarios: {}", report.scenario_count),
        format!("passed: {}", report.passed_count),
        format!("failed: {}", report.failed_count),
        String::new(),
    ];
    for result in &report.results {
        lines.push(format!(
            "- {}: {} (samples={}, latest_outcome={})",
            result.id,
            if result.passed { "PASS" } else { "FAIL" },
            result.sample_count,
            result
                .latest_outcome
                .map(|outcome| format!("{:?}", outcome).to_ascii_lowercase())
                .unwrap_or_else(|| "none".into())
        ));
        if !result.reasons.is_empty() {
            lines.push(format!("  reasons: {}", result.reasons.join("; ")));
        }
    }
    lines.join("\n")
}

fn render_contract_regression_report(report: &ContractRegressionReport) -> String {
    let mut lines = vec![
        "Contract regression report".to_string(),
        format!("scenarios: {}", report.scenario_count),
        format!("passed: {}", report.passed_count),
        format!("failed: {}", report.failed_count),
        String::new(),
    ];
    for result in &report.results {
        lines.push(format!(
            "- {}: {} (contract={:?}, required_tools={}, tool_choice={})",
            result.id,
            if result.passed { "PASS" } else { "FAIL" },
            result.contract_kind,
            if result.required_tools.is_empty() {
                "none".into()
            } else {
                result.required_tools.join(",")
            },
            result.tool_choice.as_deref().unwrap_or("none")
        ));
        if !result.reasons.is_empty() {
            lines.push(format!("  reasons: {}", result.reasons.join("; ")));
        }
    }
    lines.join("\n")
}

fn run_provider_benchmark_cli(config: &ResolvedAppConfig, args: &[String]) -> Result<String, String> {
    let suite_path = args
        .get(3)
        .ok_or_else(|| "Usage: hiveclaw replay providers <suite.toml>".to_string())?;
    let suite_body = std::fs::read_to_string(suite_path)
        .map_err(|e| format!("read provider benchmark suite failed: {}", e))?;
    let suite: ProviderBenchmarkSuite = toml::from_str(&suite_body)
        .map_err(|e| format!("parse provider benchmark suite failed: {}", e))?;
    let report =
        evaluate_provider_benchmark_suite(Path::new(&config.ssmu.sessions_dir), &suite)?;
    if report.failed_count > 0 {
        return Err(render_provider_benchmark_report(&report));
    }
    Ok(render_provider_benchmark_report(&report))
}

fn run_release_gate_cli(config: &ResolvedAppConfig, args: &[String]) -> Result<String, String> {
    let mut golden_suite = None;
    let mut provider_suite = None;
    let mut idx = 3usize;
    while let Some(arg) = args.get(idx) {
        match arg.as_str() {
            "--golden" => {
                golden_suite = Some(
                    args.get(idx + 1)
                        .ok_or_else(|| {
                            "Usage: hiveclaw replay gate --golden <suite.toml> [--providers <suite.toml>]".to_string()
                        })?
                        .clone(),
                );
                idx += 2;
            }
            "--providers" => {
                provider_suite = Some(
                    args.get(idx + 1)
                        .ok_or_else(|| {
                            "Usage: hiveclaw replay gate --golden <suite.toml> [--providers <suite.toml>]".to_string()
                        })?
                        .clone(),
                );
                idx += 2;
            }
            _ => {
                return Err(
                    "Usage: hiveclaw replay gate --golden <suite.toml> [--providers <suite.toml>]".into(),
                )
            }
        }
    }
    let golden_suite = golden_suite.ok_or_else(|| {
        "Usage: hiveclaw replay gate --golden <suite.toml> [--providers <suite.toml>]".to_string()
    })?;

    let golden_body = std::fs::read_to_string(&golden_suite)
        .map_err(|e| format!("read golden replay suite failed: {}", e))?;
    let golden_suite: GoldenReplaySuite = toml::from_str(&golden_body)
        .map_err(|e| format!("parse golden replay suite failed: {}", e))?;
    let golden_report = evaluate_golden_replay_suite(
        &RuntimeStore::for_sessions_dir(Path::new(&config.ssmu.sessions_dir)),
        &golden_suite,
    )?;
    let contract_report = evaluate_contract_regression_suite(Path::new(&config.ssmu.sessions_dir))?;
    let provider_report = if let Some(provider_suite_path) = provider_suite {
        let body = std::fs::read_to_string(provider_suite_path)
            .map_err(|e| format!("read provider benchmark suite failed: {}", e))?;
        let suite: ProviderBenchmarkSuite = toml::from_str(&body)
            .map_err(|e| format!("parse provider benchmark suite failed: {}", e))?;
        Some(evaluate_provider_benchmark_suite(
            Path::new(&config.ssmu.sessions_dir),
            &suite,
        )?)
    } else {
        None
    };

    let mut lines = vec![
        "Release gate report".to_string(),
        format!("golden_failed: {}", golden_report.failed_count),
        format!("contracts_failed: {}", contract_report.failed_count),
        format!(
            "provider_benchmark_failed: {}",
            provider_report.as_ref().map(|report| report.failed_count).unwrap_or(0)
        ),
        String::new(),
    ];
    lines.push(render_golden_replay_report(&golden_report));
    lines.push(String::new());
    lines.push(render_contract_regression_report(&contract_report));
    if let Some(provider_report) = &provider_report {
        lines.push(String::new());
        lines.push(render_provider_benchmark_report(provider_report));
    }

    if golden_report.failed_count > 0
        || contract_report.failed_count > 0
        || provider_report
            .as_ref()
            .map(|report| report.failed_count > 0)
            .unwrap_or(false)
    {
        return Err(lines.join("\n"));
    }
    Ok(lines.join("\n"))
}

fn evaluate_provider_benchmark_suite(
    sessions_dir: &Path,
    suite: &ProviderBenchmarkSuite,
) -> Result<ProviderBenchmarkReport, String> {
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    let context_records = store.list_context_inspections(None, None)?;
    let streaming_audits = store.list_streaming_decision_audits(None, None)?;
    let repair_fallback_audits = store.list_repair_fallback_audits(None, None)?;
    let mut context_by_request: BTreeMap<String, Vec<aria_core::ContextInspectionRecord>> =
        BTreeMap::new();
    for record in context_records {
        context_by_request
            .entry(uuid::Uuid::from_bytes(record.request_id).to_string())
            .or_default()
            .push(record);
    }
    let mut streaming_by_request: BTreeMap<String, Vec<StreamingDecisionAuditRecord>> =
        BTreeMap::new();
    for audit in streaming_audits {
        streaming_by_request
            .entry(audit.request_id.clone())
            .or_default()
            .push(audit);
    }
    let mut repair_by_request: BTreeMap<String, Vec<RepairFallbackAuditRecord>> = BTreeMap::new();
    for audit in repair_fallback_audits {
        repair_by_request
            .entry(audit.request_id.clone())
            .or_default()
            .push(audit);
    }

    let mut results = Vec::with_capacity(suite.scenarios.len());
    for scenario in &suite.scenarios {
        let traces = store.list_execution_traces_by_fingerprint(&scenario.task_fingerprint)?;
        let mut grouped: BTreeMap<String, ProviderBenchmarkProviderAccumulator> = BTreeMap::new();
        for trace in traces {
            let provider_records = context_by_request
                .get(&trace.request_id)
                .cloned()
                .unwrap_or_default();
            if provider_records.is_empty() {
                let key = "unknown/<unknown>".to_string();
                let entry = grouped
                    .entry(key.clone())
                    .or_insert_with(|| ProviderBenchmarkProviderAccumulator::new("unknown", "<unknown>"));
                entry.apply_trace(&trace, None);
                continue;
            }
            for record in provider_records {
                let model_ref = record
                    .provider_model
                    .clone()
                    .unwrap_or_else(|| "unknown/<unknown>".into());
                let provider_id = model_ref
                    .split_once('/')
                    .map(|(provider, _)| provider.to_string())
                    .unwrap_or_else(|| "unknown".into());
                let entry = grouped
                    .entry(model_ref.clone())
                    .or_insert_with(|| ProviderBenchmarkProviderAccumulator::new(&provider_id, &model_ref));
                let streaming = streaming_by_request
                    .get(&trace.request_id)
                    .map(|events| events.as_slice())
                    .unwrap_or(&[]);
                let repair = repair_by_request
                    .get(&trace.request_id)
                    .map(|events| events.as_slice())
                    .unwrap_or(&[]);
                entry.apply_trace(&trace, Some(&record));
                entry.apply_streaming(streaming);
                entry.apply_repair(repair);
            }
        }

        let providers = grouped
            .into_values()
            .map(|entry| entry.finish())
            .collect::<Vec<_>>();
        let mut reasons = Vec::new();
        for required in &scenario.required_providers {
            if !providers.iter().any(|provider| provider.provider_id == *required) {
                reasons.push(format!("missing required provider '{}'", required));
            }
        }
        for provider in &providers {
            if provider.sample_count < scenario.min_samples_per_provider {
                reasons.push(format!(
                    "provider '{}' expected at least {} sample(s), found {}",
                    provider.provider_id, scenario.min_samples_per_provider, provider.sample_count
                ));
            }
        }
        if scenario.require_fallback_visibility
            && !providers.iter().any(|provider| {
                provider.fallback_outcomes > 0 || provider.repair_fallback_calls > 0
            })
        {
            reasons.push("expected at least one provider with fallback visibility".into());
        }
        if providers.is_empty() {
            reasons.push("no provider benchmark samples found".into());
        }

        results.push(ProviderBenchmarkScenarioResult {
            id: scenario.id.clone(),
            task_fingerprint: scenario.task_fingerprint.clone(),
            passed: reasons.is_empty(),
            reasons,
            providers,
        });
    }
    let passed_count = results.iter().filter(|result| result.passed).count();
    Ok(ProviderBenchmarkReport {
        scenario_count: results.len(),
        passed_count,
        failed_count: results.len().saturating_sub(passed_count),
        results,
    })
}

#[derive(Debug, Clone)]
struct ProviderBenchmarkProviderAccumulator {
    provider_id: String,
    model_ref: String,
    sample_count: usize,
    success_count: usize,
    failure_count: usize,
    approval_required_count: usize,
    clarification_required_count: usize,
    latency_ms_total: u64,
    prompt_tokens_total: u64,
    fallback_outcomes: usize,
    repair_fallback_calls: usize,
}

impl ProviderBenchmarkProviderAccumulator {
    fn new(provider_id: &str, model_ref: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
            model_ref: model_ref.to_string(),
            sample_count: 0,
            success_count: 0,
            failure_count: 0,
            approval_required_count: 0,
            clarification_required_count: 0,
            latency_ms_total: 0,
            prompt_tokens_total: 0,
            fallback_outcomes: 0,
            repair_fallback_calls: 0,
        }
    }

    fn apply_trace(
        &mut self,
        trace: &aria_learning::ExecutionTrace,
        inspection: Option<&aria_core::ContextInspectionRecord>,
    ) {
        self.sample_count += 1;
        self.latency_ms_total += trace.latency_ms as u64;
        match trace.outcome {
            aria_learning::TraceOutcome::Succeeded => self.success_count += 1,
            aria_learning::TraceOutcome::Failed => self.failure_count += 1,
            aria_learning::TraceOutcome::ApprovalRequired => self.approval_required_count += 1,
            aria_learning::TraceOutcome::ClarificationRequired => {
                self.clarification_required_count += 1
            }
        }
        if let Some(inspection) = inspection {
            self.prompt_tokens_total += inspection.system_tokens as u64
                + inspection.history_tokens as u64
                + inspection.context_tokens as u64
                + inspection.user_tokens as u64;
        }
    }

    fn apply_streaming(&mut self, audits: &[StreamingDecisionAuditRecord]) {
        let mut seen = BTreeSet::new();
        for audit in audits {
            if audit.model_ref.as_deref() != Some(self.model_ref.as_str()) {
                continue;
            }
            if audit.mode == "fallback_used" && seen.insert((audit.request_id.clone(), audit.phase.clone())) {
                self.fallback_outcomes += 1;
            }
        }
    }

    fn apply_repair(&mut self, audits: &[RepairFallbackAuditRecord]) {
        self.repair_fallback_calls += audits
            .iter()
            .filter(|audit| audit.provider_id.as_deref() == Some(self.provider_id.as_str()))
            .count();
    }

    fn finish(self) -> ProviderBenchmarkProviderResult {
        ProviderBenchmarkProviderResult {
            provider_id: self.provider_id,
            model_ref: self.model_ref,
            sample_count: self.sample_count,
            success_count: self.success_count,
            failure_count: self.failure_count,
            approval_required_count: self.approval_required_count,
            clarification_required_count: self.clarification_required_count,
            average_latency_ms: if self.sample_count == 0 {
                0.0
            } else {
                self.latency_ms_total as f64 / self.sample_count as f64
            },
            average_prompt_tokens: if self.sample_count == 0 {
                0.0
            } else {
                self.prompt_tokens_total as f64 / self.sample_count as f64
            },
            fallback_outcomes: self.fallback_outcomes,
            repair_fallback_calls: self.repair_fallback_calls,
        }
    }
}

fn render_provider_benchmark_report(report: &ProviderBenchmarkReport) -> String {
    let mut lines = vec![
        "Provider benchmark report".to_string(),
        format!("scenarios: {}", report.scenario_count),
        format!("passed: {}", report.passed_count),
        format!("failed: {}", report.failed_count),
        String::new(),
    ];
    for result in &report.results {
        lines.push(format!(
            "- {}: {} (providers={})",
            result.id,
            if result.passed { "PASS" } else { "FAIL" },
            result.providers.len()
        ));
        for provider in &result.providers {
            lines.push(format!(
                "  {} [{}] samples={} success={} failure={} approval={} clarify={} avg_latency_ms={:.1} avg_prompt_tokens={:.1} fallback_outcomes={} repair_fallback_calls={}",
                provider.provider_id,
                provider.model_ref,
                provider.sample_count,
                provider.success_count,
                provider.failure_count,
                provider.approval_required_count,
                provider.clarification_required_count,
                provider.average_latency_ms,
                provider.average_prompt_tokens,
                provider.fallback_outcomes,
                provider.repair_fallback_calls
            ));
        }
        if !result.reasons.is_empty() {
            lines.push(format!("  reasons: {}", result.reasons.join("; ")));
        }
    }
    lines.join("\n")
}

fn run_skill_management_command(
    config: &ResolvedAppConfig,
    args: &[String],
) -> Option<Result<String, String>> {
    if args.get(1).map(String::as_str) != Some("skills") {
        return None;
    }

    let sessions_dir = Path::new(&config.ssmu.sessions_dir);
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    let command = args.get(2).map(String::as_str).unwrap_or("list");

    let result: Result<String, String> = match command {
        "list" => render_skill_list(&store),
        "doctor" => render_skill_doctor(&store, args.get(3).map(String::as_str)),
        "install" | "update" => run_skill_install_like_command(
            &store,
            command,
            &args[3..],
            config.ui.default_agent.as_str(),
        ),
        "enable" => args
            .get(3)
            .ok_or_else(|| "Usage: hiveclaw skills enable <skill_id>".to_string())
            .and_then(|skill_id| set_skill_enabled(&store, skill_id, true)),
        "disable" => args
            .get(3)
            .ok_or_else(|| "Usage: hiveclaw skills disable <skill_id>".to_string())
            .and_then(|skill_id| set_skill_enabled(&store, skill_id, false)),
        "bind" => run_skill_bind_command(
            &store,
            &args[3..],
            config.ui.default_agent.as_str(),
        ),
        "unbind" => run_skill_unbind_command(
            &store,
            &args[3..],
            config.ui.default_agent.as_str(),
        ),
        "export" => run_skill_export_command(&store, &args[3..]),
        other => Err(format!(
            "Unknown skills command '{}'. Usage: hiveclaw skills <list|install|update|enable|disable|bind|unbind|export|doctor>",
            other
        )),
    };

    Some(result)
}

fn render_skill_list(store: &RuntimeStore) -> Result<String, String> {
    let manifests = store.list_skill_packages()?;
    if manifests.is_empty() {
        return Ok("HiveClaw skills\ninstalled: 0".into());
    }
    let mut lines = vec![format!("HiveClaw skills\ninstalled: {}", manifests.len())];
    for manifest in manifests {
        let signatures = store.list_skill_signatures(Some(&manifest.skill_id))?;
        lines.push(format!(
            "- {} {} [{} | {} | {}]",
            manifest.skill_id,
            manifest.version,
            if manifest.enabled { "enabled" } else { "disabled" },
            skill_provenance_label(manifest.provenance.as_ref()),
            skill_trust_state_label(manifest.provenance.as_ref(), &signatures)
        ));
    }
    Ok(lines.join("\n"))
}

fn render_skill_doctor(store: &RuntimeStore, skill_id: Option<&str>) -> Result<String, String> {
    let manifests = store.list_skill_packages()?;
    let manifests = if let Some(skill_id) = skill_id {
        manifests
            .into_iter()
            .filter(|manifest| manifest.skill_id == skill_id)
            .collect::<Vec<_>>()
    } else {
        manifests
    };
    if manifests.is_empty() {
        return Ok(match skill_id {
            Some(skill_id) => format!("HiveClaw skills doctor\nskill: {}\nstatus: missing", skill_id),
            None => "HiveClaw skills doctor\nskills_total: 0".into(),
        });
    }

    let mut trusted = 0usize;
    let mut unsigned = 0usize;
    let mut disabled = 0usize;
    let mut sections = Vec::new();
    for manifest in manifests {
        let bindings = store.list_skill_bindings_for_skill(&manifest.skill_id)?;
        let activations = store.list_skill_activations_for_skill(&manifest.skill_id)?;
        let signatures = store.list_skill_signatures(Some(&manifest.skill_id))?;
        let trust = skill_trust_state_label(manifest.provenance.as_ref(), &signatures);
        if trust == "trusted" {
            trusted += 1;
        } else {
            unsigned += 1;
        }
        if !manifest.enabled {
            disabled += 1;
        }
        sections.push(format!(
            "skill: {}\n  name: {}\n  version: {}\n  enabled: {}\n  provenance: {}\n  source: {}\n  trust_state: {}\n  verified_signatures: {}\n  bindings: {}\n  active_activations: {}\n  tool_names: {}",
            manifest.skill_id,
            manifest.name,
            manifest.version,
            manifest.enabled,
            skill_provenance_label(manifest.provenance.as_ref()),
            manifest
                .provenance
                .as_ref()
                .and_then(|p| p.source_ref.clone())
                .unwrap_or_else(|| "<unknown>".into()),
            trust,
            signatures.iter().filter(|record| record.verified).count(),
            bindings.len(),
            activations.iter().filter(|record| record.active).count(),
            if manifest.tool_names.is_empty() {
                "<none>".into()
            } else {
                manifest.tool_names.join(", ")
            }
        ));
    }

    Ok(format!(
        "HiveClaw skills doctor\nskills_total: {}\ntrusted: {}\nunsigned: {}\ndisabled: {}\n\n{}",
        trusted + unsigned,
        trusted,
        unsigned,
        disabled,
        sections.join("\n\n")
    ))
}

fn run_skill_install_like_command(
    store: &RuntimeStore,
    verb: &str,
    args: &[String],
    _default_agent: &str,
) -> Result<String, String> {
    let mut dir: Option<String> = None;
    let mut signed_dir: Option<String> = None;
    let mut manifest_path: Option<String> = None;
    let mut codex_dir: Option<String> = None;
    let mut public_key: Option<String> = None;
    let mut idx = 0usize;
    while let Some(arg) = args.get(idx) {
        match arg.as_str() {
            "--dir" => {
                dir = Some(
                    args.get(idx + 1)
                        .ok_or_else(|| format!("Usage: hiveclaw skills {verb} --dir <skill_dir>"))?
                        .clone(),
                );
                idx += 2;
            }
            "--signed-dir" => {
                signed_dir = Some(
                    args.get(idx + 1)
                        .ok_or_else(|| format!("Usage: hiveclaw skills {verb} --signed-dir <skill_dir> [--public-key <hex>]"))?
                        .clone(),
                );
                idx += 2;
            }
            "--manifest" => {
                manifest_path = Some(
                    args.get(idx + 1)
                        .ok_or_else(|| format!("Usage: hiveclaw skills {verb} --manifest <skill.toml>"))?
                        .clone(),
                );
                idx += 2;
            }
            "--codex-dir" => {
                codex_dir = Some(
                    args.get(idx + 1)
                        .ok_or_else(|| format!("Usage: hiveclaw skills {verb} --codex-dir <skill_dir>"))?
                        .clone(),
                );
                idx += 2;
            }
            "--public-key" => {
                public_key = Some(
                    args.get(idx + 1)
                        .ok_or_else(|| format!("Usage: hiveclaw skills {verb} --signed-dir <skill_dir> [--public-key <hex>]"))?
                        .clone(),
                );
                idx += 2;
            }
            other => {
                return Err(format!("Unknown argument '{}'. See `hiveclaw help skills`.", other));
            }
        }
    }

    let now_us = chrono::Utc::now().timestamp_micros() as u64;
    let (manifest, trust_note) = if let Some(skill_dir) = dir {
        let mut manifest = aria_skill_runtime::load_skill_manifest_from_dir(Path::new(&skill_dir))
            .map_err(|e| e.to_string())?;
        manifest.provenance = Some(skill_provenance_from_install(
            aria_core::SkillProvenanceKind::Local,
            Some(skill_dir.clone()),
            now_us,
        ));
        (manifest, "unsigned local".to_string())
    } else if let Some(skill_dir) = signed_dir {
        let skill_dir_path = Path::new(&skill_dir);
        let manifest_path = skill_dir_path.join("skill.toml");
        let signature_path = skill_dir_path.join("skill.sig.json");
        let manifest_bytes = std::fs::read(&manifest_path)
            .map_err(|e| format!("read '{}': {}", manifest_path.display(), e))?;
        let signature_bytes = std::fs::read(&signature_path)
            .map_err(|e| format!("read '{}': {}", signature_path.display(), e))?;
        let signature: SkillManifestSignature =
            serde_json::from_slice(&signature_bytes).map_err(|e| {
                format!(
                    "invalid signature envelope '{}': {}",
                    signature_path.display(),
                    e
                )
            })?;
        verify_signed_skill_manifest(&manifest_bytes, &signature, public_key.as_deref())
            .map_err(|e| e.to_string())?;
        let mut manifest =
            aria_skill_runtime::load_skill_manifest_from_dir(skill_dir_path).map_err(|e| e.to_string())?;
        manifest.provenance = Some(skill_provenance_from_install(
            aria_core::SkillProvenanceKind::Imported,
            Some(skill_dir.clone()),
            now_us,
        ));
        store.append_skill_signature(&SkillSignatureRecord {
            record_id: format!("sig-{}", uuid::Uuid::new_v4()),
            skill_id: manifest.skill_id.clone(),
            version: manifest.version.clone(),
            algorithm: signature.algorithm.clone(),
            payload_sha256_hex: signature.payload_sha256_hex.clone(),
            public_key_hex: signature.public_key_hex.clone(),
            signature_hex: signature.signature_hex.clone(),
            source: format!("cli_{}_signed_dir", verb),
            verified: true,
            created_at_us: now_us,
        })?;
        (manifest, "trusted signed import".to_string())
    } else if let Some(manifest_path) = manifest_path {
        let manifest_toml =
            std::fs::read_to_string(&manifest_path).map_err(|e| format!("read '{}': {}", manifest_path, e))?;
        let mut manifest =
            aria_skill_runtime::parse_skill_manifest_toml(&manifest_toml).map_err(|e| e.to_string())?;
        manifest.provenance = Some(skill_provenance_from_install(
            aria_core::SkillProvenanceKind::Imported,
            Some(manifest_path.clone()),
            now_us,
        ));
        (manifest, "unsigned imported".to_string())
    } else if let Some(skill_dir) = codex_dir {
        let mut manifest =
            load_codex_compat_skill_manifest(Path::new(&skill_dir)).map_err(|e| e.to_string())?;
        manifest.provenance = Some(skill_provenance_from_install(
            aria_core::SkillProvenanceKind::CompatibilityImport,
            Some(skill_dir.clone()),
            now_us,
        ));
        (manifest, "compatibility import".to_string())
    } else {
        return Err(format!(
            "Usage: hiveclaw skills {verb} --dir <skill_dir> | --signed-dir <skill_dir> [--public-key <hex>] | --manifest <skill.toml> | --codex-dir <skill_dir>"
        ));
    };

    let existing = store
        .list_skill_packages()?
        .into_iter()
        .find(|item| item.skill_id == manifest.skill_id);
    let action = if existing.is_some() { "Updated" } else { "Installed" };
    store.upsert_skill_package(&manifest, now_us)?;
    Ok(format!(
        "{} skill '{}'.\nversion: {}\nprovenance: {}\ntrust_state: {}",
        action,
        manifest.skill_id,
        manifest.version,
        skill_provenance_label(manifest.provenance.as_ref()),
        trust_note
    ))
}

fn set_skill_enabled(store: &RuntimeStore, skill_id: &str, enabled: bool) -> Result<String, String> {
    let mut manifest = store
        .list_skill_packages()?
        .into_iter()
        .find(|manifest| manifest.skill_id == skill_id)
        .ok_or_else(|| format!("Unknown skill '{}'.", skill_id))?;
    manifest.enabled = enabled;
    store.upsert_skill_package(&manifest, chrono::Utc::now().timestamp_micros() as u64)?;
    Ok(format!(
        "{} skill '{}'.",
        if enabled { "Enabled" } else { "Disabled" },
        skill_id
    ))
}

fn run_skill_bind_command(
    store: &RuntimeStore,
    args: &[String],
    default_agent: &str,
) -> Result<String, String> {
    let skill_id = args
        .first()
        .ok_or_else(|| "Usage: hiveclaw skills bind <skill_id> [--agent <agent_id>] [--policy <manual|auto_suggest|auto_load_low_risk|approval_required>] [--version <requirement>]".to_string())?
        .clone();
    let mut agent_id = default_agent.to_string();
    let mut policy = aria_core::SkillActivationPolicy::Manual;
    let mut required_version: Option<String> = None;
    let mut idx = 1usize;
    while let Some(arg) = args.get(idx) {
        match arg.as_str() {
            "--agent" => {
                agent_id = args
                    .get(idx + 1)
                    .ok_or_else(|| "Missing value for --agent".to_string())?
                    .clone();
                idx += 2;
            }
            "--policy" => {
                let value = args
                    .get(idx + 1)
                    .ok_or_else(|| "Missing value for --policy".to_string())?;
                policy = parse_skill_activation_policy(value).map_err(|e| e.to_string())?;
                idx += 2;
            }
            "--version" => {
                required_version = Some(
                    args.get(idx + 1)
                        .ok_or_else(|| "Missing value for --version".to_string())?
                        .clone(),
                );
                idx += 2;
            }
            other => return Err(format!("Unknown argument '{}'. See `hiveclaw help skills`.", other)),
        }
    }

    let manifest = store
        .list_skill_packages()?
        .into_iter()
        .find(|manifest| manifest.skill_id == skill_id)
        .ok_or_else(|| format!("Unknown skill '{}'.", skill_id))?;
    if let Some(required_version) = &required_version {
        if !version_satisfies_requirement(&manifest.version, required_version) {
            return Err(format!(
                "Installed skill '{}' version '{}' does not satisfy '{}'.",
                manifest.skill_id, manifest.version, required_version
            ));
        }
    }
    let binding = aria_core::SkillBinding {
        binding_id: format!("skill-binding-{}", uuid::Uuid::new_v4()),
        agent_id: agent_id.clone(),
        skill_id: skill_id.clone(),
        activation_policy: policy,
        created_at_us: chrono::Utc::now().timestamp_micros() as u64,
    };
    store.upsert_skill_binding(&binding)?;
    Ok(format!(
        "Bound skill '{}' to agent '{}'.\nactivation_policy: {}",
        skill_id,
        agent_id,
        format!("{:?}", policy).to_ascii_lowercase()
    ))
}

fn run_skill_unbind_command(
    store: &RuntimeStore,
    args: &[String],
    default_agent: &str,
) -> Result<String, String> {
    let skill_id = args
        .first()
        .ok_or_else(|| "Usage: hiveclaw skills unbind <skill_id> [--agent <agent_id>]".to_string())?
        .clone();
    let mut agent_id = default_agent.to_string();
    let mut idx = 1usize;
    while let Some(arg) = args.get(idx) {
        match arg.as_str() {
            "--agent" => {
                agent_id = args
                    .get(idx + 1)
                    .ok_or_else(|| "Missing value for --agent".to_string())?
                    .clone();
                idx += 2;
            }
            other => return Err(format!("Unknown argument '{}'. See `hiveclaw help skills`.", other)),
        }
    }
    let deleted = store.delete_skill_binding(&agent_id, &skill_id)?;
    if deleted == 0 {
        return Err(format!(
            "No binding found for skill '{}' on agent '{}'.",
            skill_id, agent_id
        ));
    }
    Ok(format!("Unbound skill '{}' from agent '{}'.", skill_id, agent_id))
}

fn run_skill_export_command(store: &RuntimeStore, args: &[String]) -> Result<String, String> {
    let skill_id = args
        .first()
        .ok_or_else(|| "Usage: hiveclaw skills export <skill_id> [--output-dir <path>] [--signing-key-hex <hex>]".to_string())?
        .clone();
    let mut output_dir = String::from("./skills");
    let mut signing_key_hex: Option<String> = None;
    let mut format = String::from("native");
    let mut idx = 1usize;
    while let Some(arg) = args.get(idx) {
        match arg.as_str() {
            "--output-dir" => {
                output_dir = args
                    .get(idx + 1)
                    .ok_or_else(|| "Missing value for --output-dir".to_string())?
                    .clone();
                idx += 2;
            }
            "--signing-key-hex" => {
                signing_key_hex = Some(
                    args.get(idx + 1)
                        .ok_or_else(|| "Missing value for --signing-key-hex".to_string())?
                        .clone(),
                );
                idx += 2;
            }
            "--format" => {
                format = args
                    .get(idx + 1)
                    .ok_or_else(|| "Missing value for --format".to_string())?
                    .clone();
                idx += 2;
            }
            other => return Err(format!("Unknown argument '{}'. See `hiveclaw help skills`.", other)),
        }
    }

    let manifest = store
        .list_skill_packages()?
        .into_iter()
        .find(|manifest| manifest.skill_id == skill_id)
        .ok_or_else(|| format!("Unknown skill '{}'.", skill_id))?;
    let skill_dir = Path::new(&output_dir).join(&skill_id);
    std::fs::create_dir_all(&skill_dir)
        .map_err(|e| format!("create '{}': {}", skill_dir.display(), e))?;
    if format.eq_ignore_ascii_case("codex") {
        let source_root = manifest
            .provenance
            .as_ref()
            .and_then(|item| item.source_ref.as_deref())
            .map(PathBuf::from);
        export_codex_compat_skill(&manifest, source_root.as_deref(), &skill_dir)
            .map_err(|e| e.to_string())?;
        return Ok(format!(
            "Exported Codex-compatible skill '{}'.\npath: {}",
            skill_id,
            skill_dir.display()
        ));
    }
    let manifest_toml = toml::to_string_pretty(&manifest)
        .map_err(|e| format!("serialize manifest failed: {}", e))?;
    let manifest_path = skill_dir.join("skill.toml");
    std::fs::write(&manifest_path, &manifest_toml)
        .map_err(|e| format!("write '{}': {}", manifest_path.display(), e))?;

    if let Some(signing_key_hex) = signing_key_hex {
        let signing_key = parse_signing_key_hex(&signing_key_hex).map_err(|e| e.to_string())?;
        let signature = sign_skill_manifest_bytes(&manifest, manifest_toml.as_bytes(), &signing_key);
        let signature_path = skill_dir.join("skill.sig.json");
        let signature_json = serde_json::to_vec_pretty(&signature)
            .map_err(|e| format!("serialize signature failed: {}", e))?;
        std::fs::write(&signature_path, signature_json)
            .map_err(|e| format!("write '{}': {}", signature_path.display(), e))?;
        return Ok(format!(
            "Exported and signed skill '{}'.\nmanifest: {}\nsignature: {}",
            skill_id,
            manifest_path.display(),
            signature_path.display()
        ));
    }

    Ok(format!(
        "Exported skill '{}'.\nmanifest: {}",
        skill_id,
        manifest_path.display()
    ))
}

fn split_skill_markdown_frontmatter(markdown: &str) -> (&str, &str) {
    let trimmed = markdown.trim_start();
    let Some(rest) = trimmed.strip_prefix("---\n") else {
        return ("", markdown);
    };
    if let Some(idx) = rest.find("\n---\n") {
        let frontmatter = &rest[..idx];
        let body = &rest[idx + 5..];
        (frontmatter, body)
    } else {
        ("", markdown)
    }
}

fn parse_simple_frontmatter_field(frontmatter: &str, key: &str) -> Option<String> {
    frontmatter.lines().find_map(|line| {
        let trimmed = line.trim();
        let (lhs, rhs) = trimmed.split_once(':')?;
        if lhs.trim() != key {
            return None;
        }
        Some(rhs.trim().trim_matches('"').trim_matches('\'').to_string())
    })
}

fn derive_skill_id_from_path(skill_dir: &Path) -> String {
    skill_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("compat_skill")
        .chars()
        .map(|ch| match ch {
            'a'..='z' | '0'..='9' => ch,
            'A'..='Z' => ch.to_ascii_lowercase(),
            _ => '_',
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

fn load_codex_compat_skill_manifest(skill_dir: &Path) -> Result<aria_core::SkillPackageManifest, String> {
    let skill_path = skill_dir.join("SKILL.md");
    let markdown =
        std::fs::read_to_string(&skill_path).map_err(|e| format!("read '{}': {}", skill_path.display(), e))?;
    let (frontmatter, body) = split_skill_markdown_frontmatter(&markdown);
    let skill_id = derive_skill_id_from_path(skill_dir);
    let name = parse_simple_frontmatter_field(frontmatter, "name").unwrap_or_else(|| skill_id.clone());
    let description = parse_simple_frontmatter_field(frontmatter, "description")
        .or_else(|| {
            body.lines()
                .map(str::trim)
                .find(|line| !line.is_empty() && !line.starts_with('#'))
                .map(|line| truncate_trace_text(line, 160))
        })
        .unwrap_or_else(|| format!("Imported Codex skill '{}'", skill_id));
    Ok(aria_core::SkillPackageManifest {
        skill_id,
        name,
        description,
        version: "0.1.0".into(),
        entry_document: "SKILL.md".into(),
        tool_names: Vec::new(),
        mcp_server_dependencies: Vec::new(),
        retrieval_hints: Vec::new(),
        wasm_module_ref: None,
        config_schema: None,
        enabled: true,
        provenance: None,
    })
}

fn export_codex_compat_skill(
    manifest: &aria_core::SkillPackageManifest,
    source_root: Option<&Path>,
    output_dir: &Path,
) -> Result<(), String> {
    let body = if let Some(source_root) = source_root {
        let candidate = if source_root.is_dir() {
            source_root.join(&manifest.entry_document)
        } else {
            source_root
                .parent()
                .unwrap_or(source_root)
                .join(&manifest.entry_document)
        };
        std::fs::read_to_string(&candidate).unwrap_or_else(|_| {
            format!(
                "# {}\n\n{}\n",
                manifest.name,
                manifest.description
            )
        })
    } else {
        format!("# {}\n\n{}\n", manifest.name, manifest.description)
    };
    let (_, body_without_frontmatter) = split_skill_markdown_frontmatter(&body);
    let skill_markdown = format!(
        "---\nname: \"{}\"\ndescription: \"{}\"\n---\n\n{}",
        manifest.name.replace('"', "'"),
        manifest.description.replace('"', "'"),
        body_without_frontmatter.trim()
    );
    std::fs::write(output_dir.join("SKILL.md"), skill_markdown)
        .map_err(|e| format!("write '{}': {}", output_dir.join("SKILL.md").display(), e))?;
    Ok(())
}

fn skill_provenance_label(provenance: Option<&aria_core::SkillProvenance>) -> &'static str {
    match provenance.map(|item| item.kind) {
        Some(aria_core::SkillProvenanceKind::Local) => "local",
        Some(aria_core::SkillProvenanceKind::Imported) => "imported",
        Some(aria_core::SkillProvenanceKind::Generated) => "generated",
        Some(aria_core::SkillProvenanceKind::CompatibilityImport) => "compatibility_import",
        None => "unknown",
    }
}

fn skill_trust_state_label(
    provenance: Option<&aria_core::SkillProvenance>,
    signatures: &[SkillSignatureRecord],
) -> &'static str {
    if signatures.iter().any(|record| record.verified) {
        "trusted"
    } else {
        match provenance.map(|item| item.kind) {
            Some(aria_core::SkillProvenanceKind::Imported)
            | Some(aria_core::SkillProvenanceKind::CompatibilityImport) => "unsigned_imported",
            Some(aria_core::SkillProvenanceKind::Local)
            | Some(aria_core::SkillProvenanceKind::Generated) => "unsigned_local",
            None => "unsigned",
        }
    }
}

fn render_doctor_summary(config: &ResolvedAppConfig) -> String {
    let stt = crate::stt::inspect_stt_status(config);
    let adapters = configured_gateway_adapters(&config.gateway);
    let pid_status = render_runtime_status().unwrap_or_else(|err| format!("unavailable ({})", err));
    let install_bin_dir = default_install_bin_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "<unavailable>".into());
    let install_target = default_install_bin_dir()
        .map(|path| install_target_path(&path).display().to_string())
        .unwrap_or_else(|_| "<unavailable>".into());
    let current_exe = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "<unavailable>".into());
    let path_var = std::env::var("PATH").unwrap_or_default();
    let install_on_path = default_install_bin_dir()
        .map(|path| path_contains_dir(&path_var, &path))
        .unwrap_or(false);

    format!(
        "HiveClaw doctor\n\
         runtime_status:\n{}\n\
         config_path: {}\n\
         llm_backend: {}\n\
         llm_model: {}\n\
         configured_channels: {}\n\
         sessions_dir: {}\n\
         install_bin_dir: {}\n\
         install_target: {}\n\
         install_bin_on_path: {}\n\
         current_executable: {}\n\
         env_file_present: {}\n\
         stt_mode: {}\n\
         stt_effective_mode: {}\n\
         stt_reason: {}\n\
         stt_local_ready: {}\n\
         browser_automation_configured: {}\n",
        indent_block(&pid_status, "  "),
        config.path.display(),
        config.llm.backend,
        config.llm.model,
        adapters.join(", "),
        config.ssmu.sessions_dir,
        install_bin_dir,
        install_target,
        install_on_path,
        current_exe,
        Path::new(".env").is_file(),
        stt.configured_mode,
        stt.effective_mode,
        stt.reason,
        stt.whisper_model_exists && stt.whisper_bin_available && stt.ffmpeg_available,
        config
            .runtime
            .browser_automation_bin
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false),
    )
}

fn run_robotics_command(
    config: &ResolvedAppConfig,
    args: &[String],
) -> Option<Result<String, String>> {
    match (args.get(1).map(String::as_str), args.get(2).map(String::as_str)) {
        (Some("robotics"), Some("simulate")) => Some(run_robotics_simulation_command(config, args)),
        (Some("robotics"), Some("ros2-simulate")) => {
            Some(run_robotics_ros2_simulation_command(config, args))
        }
        _ => None,
    }
}

fn run_robotics_simulation_command(
    config: &ResolvedAppConfig,
    args: &[String],
) -> Result<String, String> {
    let fixture_path = args
        .get(3)
        .ok_or_else(|| "Usage: hiveclaw robotics simulate <fixture.json>".to_string())?;
    let fixture_text = std::fs::read_to_string(fixture_path)
        .map_err(|e| format!("read robotics fixture failed: {}", e))?;
    let fixture = crate::robotics_runtime::RoboticsSimulationFixture::from_json_str(&fixture_text)?;
    let store = RuntimeStore::for_sessions_dir(Path::new(&config.ssmu.sessions_dir));
    let record = crate::robotics_runtime::execute_robotics_simulation(&store, fixture)?;
    serde_json::to_string_pretty(&record).map_err(|e| format!("serialize failed: {}", e))
}

fn run_robotics_ros2_simulation_command(
    config: &ResolvedAppConfig,
    args: &[String],
) -> Result<String, String> {
    let fixture_path = args
        .get(3)
        .ok_or_else(|| "Usage: hiveclaw robotics ros2-simulate <fixture.json>".to_string())?;
    let fixture_text = std::fs::read_to_string(fixture_path)
        .map_err(|e| format!("read ros2 robotics fixture failed: {}", e))?;
    let fixture = crate::robotics_runtime::Ros2SimulationFixture::from_json_str(&fixture_text)?;
    let store = RuntimeStore::for_sessions_dir(Path::new(&config.ssmu.sessions_dir));
    let record = crate::robotics_runtime::execute_ros2_simulation(&store, fixture)?;
    serde_json::to_string_pretty(&record).map_err(|e| format!("serialize failed: {}", e))
}

fn indent_block(text: &str, prefix: &str) -> String {
    text.lines()
        .map(|line| format!("{}{}", prefix, line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_env_doctor(config: &ResolvedAppConfig) -> String {
    let runtime = &config.runtime;
    format!(
        "Environment doctor\n\
         config_path: {}\n\
         env_file_present: {}\n\
         rust_log_present: {}\n\
         telegram_token_present: {}\n\
         openrouter_api_key_present: {}\n\
         openai_api_key_present: {}\n\
         anthropic_api_key_present: {}\n\
         gemini_api_key_present: {}\n\
         whisper_model_present: {}\n\
         whisper_bin: {}\n\
         ffmpeg_bin: {}\n\
         browser_automation_bin_present: {}\n\
         browser_automation_allowlist_present: {}\n\
         master_key_present: {}\n",
        config.path.display(),
        Path::new(".env").is_file(),
        runtime
            .rust_log
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false),
        runtime.telegram_bot_token.is_some(),
        runtime.openrouter_api_key.is_some(),
        runtime.openai_api_key.is_some(),
        runtime.anthropic_api_key.is_some(),
        runtime.gemini_api_key.is_some(),
        runtime.whisper_cpp_model.is_some(),
        runtime.whisper_cpp_bin,
        runtime.ffmpeg_bin,
        runtime.browser_automation_bin.is_some(),
        !runtime.browser_automation_sha256_allowlist.is_empty(),
        runtime.master_key.is_some(),
    )
}

fn render_gateway_doctor(config: &ResolvedAppConfig) -> String {
    let adapters = configured_gateway_adapters(&config.gateway);
    format!(
        "Gateway doctor\n\
         configured_channels: {}\n\
         session_scope_policy: {:?}\n\
         telegram_mode: {}\n\
         telegram_port: {}\n\
         telegram_token_present: {}\n\
         websocket_bind: {}:{}\n\
         whatsapp_bind: {}:{}\n\
         whatsapp_outbound_configured: {}\n\
         fanout_rules: {}\n",
        adapters.join(", "),
        config.gateway.session_scope_policy,
        config.gateway.telegram_mode,
        config.gateway.telegram_port,
        !config.gateway.telegram_token.trim().is_empty(),
        config.gateway.websocket_bind_address,
        config.gateway.websocket_port,
        config.gateway.whatsapp_bind_address,
        config.gateway.whatsapp_port,
        config.gateway.whatsapp_outbound_url.is_some(),
        config.gateway.fanout.len(),
    )
}

fn render_browser_doctor(config: &ResolvedAppConfig) -> String {
    let runtime = &config.runtime;
    format!(
        "Browser doctor\n\
         chromium_bin: {}\n\
         chrome_bin: {}\n\
         edge_bin: {}\n\
         safari_bin: {}\n\
         automation_bin: {}\n\
         automation_allowlist_present: {}\n\
         automation_os_containment: {}\n\
         artifact_scan_bin: {}\n\
         browser_artifact_max_count: {}\n\
         browser_artifact_max_total_bytes: {}\n",
        runtime.browser_chromium_bin.as_deref().unwrap_or("<unset>"),
        runtime.browser_chrome_bin.as_deref().unwrap_or("<unset>"),
        runtime.browser_edge_bin.as_deref().unwrap_or("<unset>"),
        runtime.browser_safari_bin.as_deref().unwrap_or("<unset>"),
        runtime.browser_automation_bin.as_deref().unwrap_or("<unset>"),
        !runtime.browser_automation_sha256_allowlist.is_empty(),
        runtime.browser_automation_os_containment,
        runtime.artifact_scan_bin.as_deref().unwrap_or("<unset>"),
        runtime.browser_artifact_max_count,
        runtime.browser_artifact_max_bytes,
    )
}

fn render_mcp_doctor(config: &ResolvedAppConfig, live: bool, live_mode: Option<&str>) -> String {
    #[cfg(feature = "mcp-runtime")]
    {
        let npx_bin = resolve_executable_path("npx")
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unavailable>".into());
        let chrome_bin = config
            .runtime
            .browser_chrome_bin
            .clone()
            .or_else(|| {
                let candidate = std::path::PathBuf::from(
                    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                );
                candidate.is_file().then(|| candidate.display().to_string())
            })
            .unwrap_or_else(|| "<unset>".into());
        let store = RuntimeStore::for_sessions_dir(Path::new(&config.ssmu.sessions_dir));
        let servers = store.list_mcp_servers().unwrap_or_default();
        let chrome_server = servers.iter().find(|server| server.server_id == "chrome_devtools");
        let tool_count = store
            .list_mcp_imported_tools("chrome_devtools")
            .map(|items| items.len())
            .unwrap_or(0);
        let prompt_count = store
            .list_mcp_imported_prompts("chrome_devtools")
            .map(|items| items.len())
            .unwrap_or(0);
        let resource_count = store
            .list_mcp_imported_resources("chrome_devtools")
            .map(|items| items.len())
            .unwrap_or(0);
        let developer_bindings = store
            .list_mcp_bindings_for_agent("developer")
            .map(|bindings| {
                bindings
                    .into_iter()
                    .filter(|binding| binding.server_id == "chrome_devtools")
                    .count()
            })
            .unwrap_or(0);

        let mut text = format!(
            "MCP doctor\n\
             feature_enabled: true\n\
             npx_bin: {}\n\
             chrome_bin: {}\n\
             registered_servers: {}\n\
             chrome_devtools_registered: {}\n\
             chrome_devtools_transport: {}\n\
             chrome_devtools_endpoint: {}\n\
             chrome_devtools_imported_tools: {}\n\
             chrome_devtools_imported_prompts: {}\n\
             chrome_devtools_imported_resources: {}\n\
             developer_agent_bindings: {}\n\
             note: use `hiveclaw setup chrome-devtools-mcp` for managed launch, or `--mode auto_connect` to attach to an existing Chrome session.\n",
            npx_bin,
            chrome_bin,
            servers.len(),
            chrome_server.is_some(),
            chrome_server
                .map(|server| server.transport.as_str())
                .unwrap_or("<unregistered>"),
            chrome_server
                .map(|server| server.endpoint.as_str())
                .unwrap_or("<unregistered>"),
            tool_count,
            prompt_count,
            resource_count,
            developer_bindings,
        );
        if live {
            text.push_str(&render_mcp_live_probe(
                config,
                chrome_server.cloned(),
                live_mode,
            ));
        }
        text
    }
    #[cfg(not(feature = "mcp-runtime"))]
    {
        let _ = (config, live, live_mode);
        "MCP doctor\nfeature_enabled: false\nnote: rebuild with the `mcp-runtime` feature enabled to use Chrome DevTools MCP.\n".into()
    }
}

#[cfg(feature = "mcp-runtime")]
fn render_mcp_live_probe(
    _config: &ResolvedAppConfig,
    registered_server: Option<McpServerProfile>,
    live_mode: Option<&str>,
) -> String {
    let normalized_mode = live_mode
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("launch_managed");
    let (profile, source) = if let Some(server) = registered_server {
        (server, "registered_server")
    } else {
        (
            McpServerProfile {
                server_id: "chrome_devtools".into(),
                display_name: "Chrome DevTools MCP".into(),
                transport: "stdio".into(),
                endpoint: bootstrap_build_chrome_devtools_mcp_endpoint(
                    None,
                    Some(normalized_mode),
                    Some("stable"),
                    None,
                    None,
                    None,
                    &[],
                ),
                auth_ref: None,
                enabled: true,
            },
            if normalized_mode.eq_ignore_ascii_case("auto_connect")
                || normalized_mode.eq_ignore_ascii_case("attach_existing")
            {
                "ephemeral_auto_connect"
            } else {
                "ephemeral_managed_default"
            },
        )
    };

    let mut registry = aria_mcp::McpRegistry::new();
    registry.register_server(profile.clone());
    let mut client = aria_mcp::McpClient::new(registry, aria_mcp::TransportSelector::default());

    match client.discover_server_catalog("chrome_devtools") {
        Ok(catalog) => {
            let sample_tools = catalog
                .tools
                .iter()
                .take(5)
                .map(|tool| tool.tool_name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "live_probe: ok\nlive_probe_source: {}\nlive_endpoint: {}\nlive_protocol_version: {}\nlive_tool_count: {}\nlive_prompt_count: {}\nlive_resource_count: {}\nlive_sample_tools: {}\n",
                source,
                profile.endpoint,
                catalog.protocol_version.as_deref().unwrap_or("<unknown>"),
                catalog.tools.len(),
                catalog.prompts.len(),
                catalog.resources.len(),
                if sample_tools.is_empty() { "<none>" } else { &sample_tools }
            )
        }
        Err(err) => format!(
            "live_probe: failed\nlive_probe_source: {}\nlive_endpoint: {}\nlive_error: {}\n",
            source, profile.endpoint, err
        ),
    }
}

fn render_stt_doctor(config: &ResolvedAppConfig) -> String {
    let status = crate::stt::inspect_stt_status(config);
    let model_path = status
        .whisper_model_path
        .as_deref()
        .unwrap_or("<unset>");
    format!(
        "STT doctor\n\
         configured_mode: {}\n\
         effective_mode: {}\n\
         reason: {}\n\
         whisper_model: {}\n\
         whisper_model_exists: {}\n\
         whisper_bin: {}\n\
         whisper_bin_available: {}\n\
         ffmpeg_bin: {}\n\
         ffmpeg_available: {}\n\
         cloud_endpoint_configured: {}\n\
         cloud_fallback_enabled: {}\n\
         language_hint: {}\n",
        status.configured_mode,
        status.effective_mode,
        status.reason,
        model_path,
        status.whisper_model_exists,
        status.whisper_bin,
        status.whisper_bin_available,
        status.ffmpeg_bin,
        status.ffmpeg_available,
        status.cloud_endpoint_configured,
        status.cloud_fallback_enabled,
        status.language_hint.as_deref().unwrap_or("<auto>"),
    )
}

fn setup_local_stt_env(config: &ResolvedAppConfig) -> Result<String, String> {
    let mut bootstrap_steps = Vec::new();
    let status = crate::stt::inspect_stt_status(config);
    let default_model_path = default_local_whisper_model_path()?;
    if !status.whisper_bin_available || !status.ffmpeg_available {
        bootstrap_steps.extend(bootstrap_local_stt_binaries()?);
    }
    let model_path = status
        .whisper_model_path
        .clone()
        .unwrap_or_else(|| default_model_path.to_string_lossy().to_string());
    if !PathBuf::from(&model_path).is_file() {
        bootstrap_steps.push(download_default_local_whisper_model(Path::new(&model_path))?);
    }

    let whisper_bin = resolve_executable_path("whisper-cli")
        .map(|path| path.to_string_lossy().to_string())
        .or_else(|| {
            let configured = PathBuf::from(&status.whisper_bin);
            configured.is_file().then(|| configured.to_string_lossy().to_string())
        })
        .ok_or_else(|| "Whisper binary could not be resolved after bootstrap.".to_string())?;
    let ffmpeg_bin = resolve_executable_path("ffmpeg")
        .map(|path| path.to_string_lossy().to_string())
        .or_else(|| {
            let configured = PathBuf::from(&status.ffmpeg_bin);
            configured.is_file().then(|| configured.to_string_lossy().to_string())
        })
        .ok_or_else(|| "ffmpeg binary could not be resolved after bootstrap.".to_string())?;

    let env_path = PathBuf::from(".env");
    upsert_env_file_entries(
        &env_path,
        &[
            ("WHISPER_CPP_MODEL", model_path.as_str()),
            ("WHISPER_CPP_BIN", whisper_bin.as_str()),
            ("FFMPEG_BIN", ffmpeg_bin.as_str()),
        ],
    )?;

    let steps = if bootstrap_steps.is_empty() {
        "bootstrap_steps: none (existing local STT runtime detected)".to_string()
    } else {
        format!("bootstrap_steps:\n- {}", bootstrap_steps.join("\n- "))
    };
    Ok(format!(
        "Configured local STT in {}.\n\
         mode_hint: keep gateway.stt_mode=\"auto\" for auto-detect, or set \"local\" for strict local mode.\n\
         {}\n\
         whisper_model: {}\n\
         whisper_bin: {}\n\
         ffmpeg_bin: {}",
        env_path.display(),
        steps,
        model_path,
        whisper_bin,
        ffmpeg_bin
    ))
}

#[cfg(feature = "mcp-runtime")]
fn bootstrap_imported_tool_from_discovery(
    server_id: &str,
    tool: aria_mcp::McpDiscoveredTool,
) -> McpImportedTool {
    McpImportedTool {
        import_id: format!("mcp-tool:{}:{}", server_id, tool.tool_name),
        server_id: server_id.to_string(),
        tool_name: tool.tool_name,
        description: tool.description,
        parameters_schema: tool.parameters_schema,
    }
}

#[cfg(feature = "mcp-runtime")]
fn bootstrap_imported_prompt_from_discovery(
    server_id: &str,
    prompt: aria_mcp::McpDiscoveredPrompt,
) -> McpImportedPrompt {
    McpImportedPrompt {
        import_id: format!("mcp-prompt:{}:{}", server_id, prompt.prompt_name),
        server_id: server_id.to_string(),
        prompt_name: prompt.prompt_name,
        description: prompt.description,
        arguments_schema: prompt.arguments_schema,
    }
}

#[cfg(feature = "mcp-runtime")]
fn bootstrap_imported_resource_from_discovery(
    server_id: &str,
    resource: aria_mcp::McpDiscoveredResource,
) -> McpImportedResource {
    McpImportedResource {
        import_id: format!("mcp-resource:{}:{}", server_id, resource.resource_uri),
        server_id: server_id.to_string(),
        resource_uri: resource.resource_uri,
        description: resource.description,
        mime_type: resource.mime_type,
    }
}

#[cfg(feature = "mcp-runtime")]
fn bootstrap_refresh_mcp_import_cache(
    store: &RuntimeStore,
    server_id: &str,
    refreshed_at_us: u64,
) -> Result<(), String> {
    let server = store
        .list_mcp_servers()?
        .into_iter()
        .find(|server| server.server_id == server_id)
        .ok_or_else(|| format!("unknown MCP server '{}'", server_id))?;
    let record = McpImportCacheRecord {
        server_id: server.server_id.clone(),
        transport: server.transport,
        tool_count: store.list_mcp_imported_tools(server_id)?.len() as u32,
        prompt_count: store.list_mcp_imported_prompts(server_id)?.len() as u32,
        resource_count: store.list_mcp_imported_resources(server_id)?.len() as u32,
        refreshed_at_us,
    };
    store.upsert_mcp_import_cache_record(&record)
}

#[cfg(feature = "mcp-runtime")]
fn bootstrap_bind_discovered_mcp_entries(
    store: &RuntimeStore,
    agent_id: &str,
    server_id: &str,
    tools: &[McpImportedTool],
    prompts: &[McpImportedPrompt],
    resources: &[McpImportedResource],
    bind_prompts: bool,
    bind_resources: bool,
) -> Result<(usize, usize, usize), String> {
    let now_us = chrono::Utc::now().timestamp_micros() as u64;
    let mut bound_tools = 0usize;
    let mut bound_prompts_count = 0usize;
    let mut bound_resources_count = 0usize;
    for tool in tools {
        let binding = McpBindingRecord {
            binding_id: format!("mcp-binding-{}", uuid::Uuid::new_v4()),
            agent_id: agent_id.to_string(),
            server_id: server_id.to_string(),
            primitive_kind: McpPrimitiveKind::Tool,
            target_name: tool.tool_name.clone(),
            created_at_us: now_us,
        };
        store.upsert_mcp_binding(&binding)?;
        bound_tools += 1;
    }
    if bind_prompts {
        for prompt in prompts {
            let binding = McpBindingRecord {
                binding_id: format!("mcp-binding-{}", uuid::Uuid::new_v4()),
                agent_id: agent_id.to_string(),
                server_id: server_id.to_string(),
                primitive_kind: McpPrimitiveKind::Prompt,
                target_name: prompt.prompt_name.clone(),
                created_at_us: now_us,
            };
            store.upsert_mcp_binding(&binding)?;
            bound_prompts_count += 1;
        }
    }
    if bind_resources {
        for resource in resources {
            let binding = McpBindingRecord {
                binding_id: format!("mcp-binding-{}", uuid::Uuid::new_v4()),
                agent_id: agent_id.to_string(),
                server_id: server_id.to_string(),
                primitive_kind: McpPrimitiveKind::Resource,
                target_name: resource.resource_uri.clone(),
                created_at_us: now_us,
            };
            store.upsert_mcp_binding(&binding)?;
            bound_resources_count += 1;
        }
    }
    Ok((bound_tools, bound_prompts_count, bound_resources_count))
}

#[cfg(feature = "mcp-runtime")]
fn bootstrap_build_chrome_devtools_mcp_endpoint(
    executable: Option<&str>,
    mode: Option<&str>,
    channel: Option<&str>,
    headless: Option<bool>,
    isolated: Option<bool>,
    slim: Option<bool>,
    extra_args: &[String],
) -> String {
    let mut parts = vec![
        executable.unwrap_or("npx").trim().to_string(),
        "-y".to_string(),
        "chrome-devtools-mcp@latest".to_string(),
    ];
    let normalized_mode = mode
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("launch_managed");
    if normalized_mode.eq_ignore_ascii_case("auto_connect")
        || normalized_mode.eq_ignore_ascii_case("attach_existing")
    {
        parts.push("--autoConnect".to_string());
    } else {
        if headless.unwrap_or(true) {
            parts.push("--headless".to_string());
        }
        if isolated.unwrap_or(true) {
            parts.push("--isolated".to_string());
        }
        if slim.unwrap_or(true) {
            parts.push("--slim".to_string());
        }
    }
    if let Some(channel) = channel
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "stable")
    {
        parts.push(format!("--channel={}", channel));
    }
    for arg in extra_args {
        let trimmed = arg.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }
    parts.join(" ")
}

fn setup_ssh_backend_cli_impl(config: &ResolvedAppConfig, args: &[String]) -> Result<String, String> {
    let mut backend_id: Option<String> = None;
    let mut display_name: Option<String> = None;
    let mut host: Option<String> = None;
    let mut user: Option<String> = None;
    let mut port: u16 = 22;
    let mut identity_file: Option<String> = None;
    let mut remote_workspace_root: Option<String> = None;
    let mut known_hosts_policy = aria_core::ExecutionBackendKnownHostsPolicy::Strict;
    let mut allow_network_egress = true;
    let mut is_default = false;
    let mut trust_level = aria_core::ExecutionBackendTrustLevel::RemoteBounded;

    let usage = "Usage: hiveclaw setup ssh-backend --backend-id <id> --host <host> [--user <user>] [--port <port>] [--identity-file <path>] [--remote-workspace-root <path>] [--known-hosts-policy <strict|accept_new|insecure_ignore>] [--default] [--no-network-egress] [--trust <remote_bounded|remote_privileged>]".to_string();
    let mut idx = 3usize;
    while let Some(arg) = args.get(idx) {
        match arg.as_str() {
            "--backend-id" => {
                backend_id = Some(args.get(idx + 1).ok_or_else(|| usage.clone())?.clone());
                idx += 2;
            }
            "--display-name" => {
                display_name = Some(args.get(idx + 1).ok_or_else(|| usage.clone())?.clone());
                idx += 2;
            }
            "--host" => {
                host = Some(args.get(idx + 1).ok_or_else(|| usage.clone())?.clone());
                idx += 2;
            }
            "--user" => {
                user = Some(args.get(idx + 1).ok_or_else(|| usage.clone())?.clone());
                idx += 2;
            }
            "--port" => {
                port = args
                    .get(idx + 1)
                    .ok_or_else(|| usage.clone())?
                    .parse::<u16>()
                    .map_err(|_| usage.clone())?;
                idx += 2;
            }
            "--identity-file" => {
                identity_file = Some(args.get(idx + 1).ok_or_else(|| usage.clone())?.clone());
                idx += 2;
            }
            "--remote-workspace-root" => {
                remote_workspace_root =
                    Some(args.get(idx + 1).ok_or_else(|| usage.clone())?.clone());
                idx += 2;
            }
            "--known-hosts-policy" => {
                let value = args.get(idx + 1).ok_or_else(|| usage.clone())?;
                known_hosts_policy = match value.as_str() {
                    "strict" => aria_core::ExecutionBackendKnownHostsPolicy::Strict,
                    "accept_new" => aria_core::ExecutionBackendKnownHostsPolicy::AcceptNew,
                    "insecure_ignore" => {
                        aria_core::ExecutionBackendKnownHostsPolicy::InsecureIgnore
                    }
                    _ => return Err(usage.clone()),
                };
                idx += 2;
            }
            "--trust" => {
                let value = args.get(idx + 1).ok_or_else(|| usage.clone())?;
                trust_level = match value.as_str() {
                    "remote_bounded" => aria_core::ExecutionBackendTrustLevel::RemoteBounded,
                    "remote_privileged" => {
                        aria_core::ExecutionBackendTrustLevel::RemotePrivileged
                    }
                    _ => return Err(usage.clone()),
                };
                idx += 2;
            }
            "--default" => {
                is_default = true;
                idx += 1;
            }
            "--no-network-egress" => {
                allow_network_egress = false;
                idx += 1;
            }
            other => {
                return Err(format!("Unknown argument '{}'. {}", other, usage));
            }
        }
    }

    let backend_id = backend_id.ok_or_else(|| usage.clone())?;
    let host = host.ok_or_else(|| usage.clone())?;
    let sessions_dir = Path::new(&config.ssmu.sessions_dir);
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    ensure_default_execution_backend_profiles(sessions_dir)?;
    let now_us = chrono::Utc::now().timestamp_micros() as u64;
    let profile = aria_core::ExecutionBackendProfile {
        backend_id: backend_id.clone(),
        display_name: display_name.unwrap_or_else(|| format!("SSH {}", host)),
        kind: aria_core::ExecutionBackendKind::Ssh,
        config: Some(aria_core::ExecutionBackendConfig::Ssh(
            aria_core::ExecutionBackendSshConfig {
                host: host.clone(),
                port,
                user: user.clone(),
                identity_file: identity_file.clone(),
                remote_workspace_root: remote_workspace_root.clone(),
                known_hosts_policy,
            },
        )),
        is_default,
        requires_approval: true,
        supports_workspace_mount: true,
        supports_browser: false,
        supports_desktop: false,
        supports_artifact_return: true,
        supports_network_egress: allow_network_egress,
        trust_level,
    };
    store.upsert_execution_backend_profile(&profile, now_us)?;
    Ok(format!(
        "Registered SSH backend '{}' for {}:{}{}{}",
        backend_id,
        host,
        port,
        user
            .as_deref()
            .map(|value| format!(" as {}", value))
            .unwrap_or_default(),
        remote_workspace_root
            .as_deref()
            .map(|value| format!(" with remote workspace root {}", value))
            .unwrap_or_default()
    ))
}

#[cfg(feature = "mcp-runtime")]
fn setup_chrome_devtools_mcp_cli(config: &ResolvedAppConfig, args: &[String]) -> Result<String, String> {
    let mut agent_id = "developer".to_string();
    let mut mode = "launch_managed".to_string();
    let mut channel = "stable".to_string();
    let mut bind_prompts = false;
    let mut bind_resources = false;
    let mut idx = 3usize;
    while let Some(arg) = args.get(idx) {
        match arg.as_str() {
            "--agent" => {
                let value = args.get(idx + 1).ok_or_else(|| {
                    "Usage: hiveclaw setup chrome-devtools-mcp [--agent <agent_id>] [--mode <launch_managed|auto_connect>] [--channel <stable|beta|dev|canary>] [--bind-prompts] [--bind-resources]".to_string()
                })?;
                agent_id = value.clone();
                idx += 2;
            }
            "--mode" => {
                let value = args.get(idx + 1).ok_or_else(|| {
                    "Usage: hiveclaw setup chrome-devtools-mcp [--agent <agent_id>] [--mode <launch_managed|auto_connect>] [--channel <stable|beta|dev|canary>] [--bind-prompts] [--bind-resources]".to_string()
                })?;
                mode = value.clone();
                idx += 2;
            }
            "--channel" => {
                let value = args.get(idx + 1).ok_or_else(|| {
                    "Usage: hiveclaw setup chrome-devtools-mcp [--agent <agent_id>] [--mode <launch_managed|auto_connect>] [--channel <stable|beta|dev|canary>] [--bind-prompts] [--bind-resources]".to_string()
                })?;
                channel = value.clone();
                idx += 2;
            }
            "--bind-prompts" => {
                bind_prompts = true;
                idx += 1;
            }
            "--bind-resources" => {
                bind_resources = true;
                idx += 1;
            }
            other => {
                return Err(format!(
                    "Unknown argument '{}'. Usage: hiveclaw setup chrome-devtools-mcp [--agent <agent_id>] [--mode <launch_managed|auto_connect>] [--channel <stable|beta|dev|canary>] [--bind-prompts] [--bind-resources]",
                    other
                ));
            }
        }
    }

    let server_id = "chrome_devtools";
    let endpoint = bootstrap_build_chrome_devtools_mcp_endpoint(
        None,
        Some(&mode),
        Some(&channel),
        None,
        None,
        None,
        &[],
    );
    let profile = McpServerProfile {
        server_id: server_id.to_string(),
        display_name: "Chrome DevTools MCP".into(),
        transport: "stdio".into(),
        endpoint,
        auth_ref: None,
        enabled: true,
    };
    if aria_mcp::reserved_native_mcp_target(&profile.server_id)
        || aria_mcp::reserved_native_mcp_target(&profile.display_name)
    {
        return Err(format!(
            "MCP server '{}' is reserved for a native/internal subsystem boundary",
            profile.server_id
        ));
    }

    let now_us = chrono::Utc::now().timestamp_micros() as u64;
    let sessions_dir = Path::new(&config.ssmu.sessions_dir);
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    store.upsert_mcp_server(&profile, now_us)?;

    let mut registry = aria_mcp::McpRegistry::new();
    registry.register_server(profile.clone());
    let mut client = aria_mcp::McpClient::new(registry, aria_mcp::TransportSelector::default());
    let catalog = client
        .discover_server_catalog(server_id)
        .map_err(|e| e.to_string())?;

    let tools = catalog
        .tools
        .into_iter()
        .map(|tool| bootstrap_imported_tool_from_discovery(server_id, tool))
        .collect::<Vec<_>>();
    let prompts = catalog
        .prompts
        .into_iter()
        .map(|prompt| bootstrap_imported_prompt_from_discovery(server_id, prompt))
        .collect::<Vec<_>>();
    let resources = catalog
        .resources
        .into_iter()
        .map(|resource| bootstrap_imported_resource_from_discovery(server_id, resource))
        .collect::<Vec<_>>();

    store.replace_mcp_imported_tools(server_id, &tools, now_us)?;
    store.replace_mcp_imported_prompts(server_id, &prompts, now_us)?;
    store.replace_mcp_imported_resources(server_id, &resources, now_us)?;
    bootstrap_refresh_mcp_import_cache(&store, server_id, now_us)?;

    let (bound_tools, bound_prompts_count, bound_resources_count) =
        bootstrap_bind_discovered_mcp_entries(
            &store,
            &agent_id,
            server_id,
            &tools,
            &prompts,
            &resources,
            bind_prompts,
            bind_resources,
        )?;

    Ok(format!(
        "Configured Chrome DevTools MCP for agent '{}'.\nmode: {}\nchannel: {}\nendpoint: {}\nimported_tools: {}\nimported_prompts: {}\nimported_resources: {}\nbound_tools: {}\nbound_prompts: {}\nbound_resources: {}",
        agent_id,
        mode,
        channel,
        profile.endpoint,
        tools.len(),
        prompts.len(),
        resources.len(),
        bound_tools,
        bound_prompts_count,
        bound_resources_count,
    ))
}

#[cfg(not(feature = "mcp-runtime"))]
fn setup_chrome_devtools_mcp_cli(_config: &ResolvedAppConfig, _args: &[String]) -> Result<String, String> {
    Err("Chrome DevTools MCP setup requires the `mcp-runtime` feature.".into())
}

fn default_local_whisper_model_path() -> Result<PathBuf, String> {
    let user_dirs = directories::UserDirs::new()
        .ok_or_else(|| "unable to resolve user home directory for local STT setup".to_string())?;
    Ok(user_dirs
        .home_dir()
        .join(".hiveclaw")
        .join("models")
        .join("whisper")
        .join("ggml-small.bin"))
}

fn default_local_whisper_model_url() -> &'static str {
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin"
}

fn resolve_executable_path(command: &str) -> Option<PathBuf> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }
    let path = PathBuf::from(command);
    if path.is_absolute() || path.components().count() > 1 {
        return path.is_file().then_some(path);
    }
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|entry| entry.join(command))
        .find(|candidate| candidate.is_file())
}

fn run_bootstrap_command(program: &str, args: &[&str]) -> Result<(), String> {
    let status = std::process::Command::new(program)
        .args(args)
        .status()
        .map_err(|e| format!("failed to run '{} {}': {}", program, args.join(" "), e))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "command failed: {} {} (status: {})",
            program,
            args.join(" "),
            status
        ))
    }
}

fn bootstrap_local_stt_binaries() -> Result<Vec<String>, String> {
    if resolve_executable_path("ffmpeg").is_some() && resolve_executable_path("whisper-cli").is_some()
    {
        return Ok(Vec::new());
    }
    if resolve_executable_path("brew").is_some() {
        run_bootstrap_command("brew", &["install", "ffmpeg", "whisper-cpp"])?;
        return Ok(vec!["installed ffmpeg and whisper-cpp via Homebrew".to_string()]);
    }
    Err(
        "automatic local STT bootstrap currently supports Homebrew-based environments only. Install ffmpeg and whisper.cpp, then rerun `hiveclaw setup stt --local`."
            .to_string(),
    )
}

fn download_default_local_whisper_model(model_path: &Path) -> Result<String, String> {
    let parent = model_path
        .parent()
        .ok_or_else(|| format!("model path '{}' has no parent directory", model_path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("create model directory '{}' failed: {}", parent.display(), e))?;
    if resolve_executable_path("curl").is_some() {
        run_bootstrap_command(
            "curl",
            &[
                "-L",
                default_local_whisper_model_url(),
                "-o",
                model_path
                    .to_str()
                    .ok_or_else(|| format!("invalid model path '{}'", model_path.display()))?,
            ],
        )?;
        return Ok(format!(
            "downloaded default whisper model from {}",
            default_local_whisper_model_url()
        ));
    }
    Err(
        "curl is required to download the default whisper model automatically. Install curl or download the model manually, then rerun `hiveclaw setup stt --local`."
            .to_string(),
    )
}

fn upsert_env_file_entries(path: &Path, entries: &[(&str, &str)]) -> Result<(), String> {
    let mut lines = if path.exists() {
        std::fs::read_to_string(path)
            .map_err(|e| format!("read {} failed: {}", path.display(), e))?
            .lines()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    for (key, value) in entries {
        let prefix = format!("{}=", key);
        if let Some(index) = lines
            .iter()
            .position(|line| line.trim_start().starts_with(&prefix))
        {
            lines[index] = format!("{}={}", key, value);
        } else {
            lines.push(format!("{}={}", key, value));
        }
    }

    let mut content = lines.join("\n");
    if !content.ends_with('\n') {
        content.push('\n');
    }
    std::fs::write(path, content).map_err(|e| format!("write {} failed: {}", path.display(), e))
}

fn list_configured_channels(config_path: &Path) -> Result<String, String> {
    let config = load_config(config_path.to_string_lossy().as_ref())
        .map_err(|e| format!("load config failed: {}", e))?;
    let manifests = configured_gateway_adapters(&config.gateway)
        .into_iter()
        .filter_map(|adapter| parse_gateway_channel_label(&adapter))
        .map(aria_core::builtin_channel_plugin_manifest)
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&manifests).map_err(|e| format!("serialize channels failed: {}", e))
}

fn list_channel_status(config_path: &Path) -> Result<String, String> {
    let config = load_config(config_path.to_string_lossy().as_ref())
        .map_err(|e| format!("load config failed: {}", e))?;
    let statuses = configured_gateway_adapters(&config.gateway)
        .into_iter()
        .map(|adapter| {
            let channel = parse_gateway_channel_label(&adapter).unwrap_or(aria_core::GatewayChannel::Unknown);
            let manifest = aria_core::builtin_channel_plugin_manifest(channel);
            let validation = aria_core::validate_channel_plugin_manifest(&manifest);
            serde_json::json!({
                "adapter": adapter,
                "plugin_id": manifest.plugin_id,
                "transport": manifest.transport,
                "approval_capable": manifest.approval_capable,
                "fallback_mode": manifest.fallback_mode,
                "valid": validation.is_ok(),
                "error": validation.err(),
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&statuses).map_err(|e| format!("serialize channel status failed: {}", e))
}

fn add_configured_channel(config_path: &Path, channel: &str) -> Result<String, String> {
    let normalized = channel.trim().to_ascii_lowercase();
    let parsed = parse_gateway_channel_label(&normalized)
        .ok_or_else(|| format!("unknown channel '{}'", channel))?;
    let manifest = aria_core::builtin_channel_plugin_manifest(parsed);
    aria_core::validate_channel_plugin_manifest(&manifest)?;
    let mut doc = std::fs::read_to_string(config_path)
        .map_err(|e| format!("read config failed: {}", e))?
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("parse config failed: {}", e))?;
    if doc.get("gateway").is_none() {
        doc["gateway"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let gateway = doc["gateway"]
        .as_table_mut()
        .ok_or_else(|| "gateway config must be a table".to_string())?;
    let current = gateway
        .get("adapters")
        .and_then(|item| item.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut adapters = current;
    if !adapters.iter().any(|entry| entry == &normalized) {
        adapters.push(normalized.clone());
        adapters.sort();
        adapters.dedup();
    }
    let mut arr = toml_edit::Array::default();
    for adapter in adapters {
        arr.push(adapter);
    }
    gateway["adapters"] = toml_edit::value(arr);
    std::fs::write(config_path, doc.to_string()).map_err(|e| format!("write config failed: {}", e))?;
    Ok(format!("added channel '{}'", normalized))
}

fn remove_configured_channel(config_path: &Path, channel: &str) -> Result<String, String> {
    let normalized = channel.trim().to_ascii_lowercase();
    let mut doc = std::fs::read_to_string(config_path)
        .map_err(|e| format!("read config failed: {}", e))?
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("parse config failed: {}", e))?;
    if doc.get("gateway").is_none() {
        doc["gateway"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let gateway = doc["gateway"]
        .as_table_mut()
        .ok_or_else(|| "gateway config must be a table".to_string())?;
    let current = gateway
        .get("adapters")
        .and_then(|item| item.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let filtered = current
        .into_iter()
        .filter(|entry| entry != &normalized)
        .collect::<Vec<_>>();
    let mut arr = toml_edit::Array::default();
    for adapter in filtered {
        arr.push(adapter);
    }
    gateway["adapters"] = toml_edit::value(arr);
    std::fs::write(config_path, doc.to_string()).map_err(|e| format!("write config failed: {}", e))?;
    Ok(format!("removed channel '{}'", normalized))
}

#[allow(clippy::too_many_arguments)]
async fn process_cli_ingress_request(
    req: &AgentRequest,
    config: &ResolvedAppConfig,
    router_index: &RouterIndex,
    embedder: &FastEmbedder,
    llm_pool: &Arc<LlmBackendPool>,
    cedar: &Arc<aria_policy::CedarEvaluator>,
    agent_store: &AgentConfigStore,
    tool_registry: &ToolManifestStore,
    session_memory: &aria_ssmu::SessionMemory,
    capability_index: &Arc<aria_ssmu::CapabilityIndex>,
    vector_store: &Arc<VectorStore>,
    keyword_index: &Arc<KeywordIndex>,
    firewall: &Arc<aria_safety::DfaFirewall>,
    vault: &Arc<aria_vault::CredentialVault>,
    tx_cron: &tokio::sync::mpsc::Sender<aria_intelligence::CronCommand>,
    registry: &Arc<Mutex<ProviderRegistry>>,
    session_tool_caches: &SessionToolCacheStore,
    hooks: &HookRegistry,
    session_locks: &Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    embed_semaphore: &Arc<tokio::sync::Semaphore>,
) {
    if let Some(reply) = handle_cli_approval_command(
        req,
        config,
        session_memory,
        vault,
        cedar,
        tx_cron,
    )
    .await
    {
        send_universal_response(req, &reply, config).await;
        return;
    }

    if let Some(reply) = handle_cli_control_command(req, config, llm_pool, agent_store, session_memory) {
        send_universal_response(req, &reply, config).await;
        return;
    }

    if let Some(output) = handle_runtime_control_command(req, config, llm_pool, session_memory, None).await {
        send_universal_response(req, &output.text, config).await;
        return;
    }

    match process_request(
        req,
        &config.learning,
        router_index,
        embedder,
        llm_pool,
        cedar,
        agent_store,
        tool_registry,
        session_memory,
        capability_index,
        vector_store,
        keyword_index,
        firewall,
        vault,
        tx_cron,
        registry,
        session_tool_caches,
        hooks,
        session_locks,
        embed_semaphore,
        config.llm.max_tool_rounds,
        None,
        Some(&Arc::new(std::sync::atomic::AtomicBool::new(false))),
        std::path::Path::new(&config.ssmu.sessions_dir),
        config.policy.whitelist.clone(),
        config.policy.forbid.clone(),
        resolve_request_timezone(config, &req.user_id),
    )
    .await
    {
        Ok(aria_intelligence::OrchestratorResult::Completed(text)) => {
            send_universal_response(req, &text, config).await;
        }
        Ok(aria_intelligence::OrchestratorResult::AgentElevationRequired { agent_id, message }) => {
            let result = aria_intelligence::OrchestratorResult::AgentElevationRequired {
                agent_id,
                message,
            };
            let approval_text = persist_pending_approval_for_result(
                Path::new(&config.ssmu.sessions_dir),
                req,
                &result,
            )
            .map(|(_, text)| text)
            .unwrap_or_else(|_| "Approval required.".to_string());
            send_universal_response(req, &approval_text, config).await;
        }
        Ok(aria_intelligence::OrchestratorResult::ToolApprovalRequired {
            call,
            pending_prompt,
        }) => {
            let result = aria_intelligence::OrchestratorResult::ToolApprovalRequired {
                call,
                pending_prompt,
            };
            let approval_text = persist_pending_approval_for_result(
                Path::new(&config.ssmu.sessions_dir),
                req,
                &result,
            )
            .map(|(_, text)| text)
            .unwrap_or_else(|_| "Approval required.".to_string());
            send_universal_response(req, &approval_text, config).await;
        }
        Err(e) => {
            let raw = e.to_string();
            let message = if let Ok((_, approval_text)) =
                persist_pending_approval_for_error(Path::new(&config.ssmu.sessions_dir), req, &raw)
            {
                format!(
                    "{}\n\n{}",
                    format_orchestrator_error_for_user(&raw),
                    approval_text
                )
            } else {
                format_orchestrator_error_for_user(&raw)
            };
            send_universal_response(req, &message, config).await;
            error!(error = %e, "Orchestrator error");
        }
    };
}

fn register_discoverable_tool(
    tool_registry: &mut ToolManifestStore,
    vector_store: &mut VectorStore,
    embedder: &impl EmbeddingModel,
    tool_name: &str,
    owner_tag: &str,
) {
    let (desc, schema) = match tool_name {
        "register_external_compat_tool" => (
            "Register a local external compatibility tool that speaks the typed stdin/stdout JSON sidecar contract.",
            r#"{"tool_name":{"type":"string"},"command":{"type":"array","items":{"type":"string"}},"description":{"type":"string"},"parameters_schema":{"type":"string"}}"#,
        ),
        "register_remote_tool" => (
            "Register a remote HTTP tool endpoint that accepts a typed JSON envelope and returns a tool result envelope.",
            r#"{"tool_name":{"type":"string"},"endpoint":{"type":"string"},"description":{"type":"string"},"parameters_schema":{"type":"string"}}"#,
        ),
        "register_mcp_server" => (
            "Register an MCP server process for later import, binding, and invocation.",
            r#"{"server_id":{"type":"string"},"display_name":{"type":"string"},"transport":{"type":"string","enum":["stdio","stdio_once"]},"endpoint":{"type":"string"},"auth_ref":{"type":"string"},"enabled":{"type":"boolean"}}"#,
        ),
        "sync_mcp_server_catalog" => (
            "Connect to a registered MCP server, discover its tools/prompts/resources, persist imports, and optionally bind them to an agent.",
            r#"{"server_id":{"type":"string"},"agent_id":{"type":"string"},"bind_tools":{"type":"boolean"},"bind_prompts":{"type":"boolean"},"bind_resources":{"type":"boolean"}}"#,
        ),
        "setup_chrome_devtools_mcp" => (
            "Register and sync the Chrome DevTools MCP server so the agent can use Chrome-backed browser tools over MCP. Defaults to a managed launched Chrome session; use mode=auto_connect to attach to an already-running Chrome session.",
            r#"{"server_id":{"type":"string"},"display_name":{"type":"string"},"endpoint_override":{"type":"string"},"executable":{"type":"string"},"mode":{"type":"string","enum":["launch_managed","auto_connect","attach_existing"]},"channel":{"type":"string","enum":["stable","beta","dev","canary"]},"headless":{"type":"boolean"},"isolated":{"type":"boolean"},"slim":{"type":"boolean"},"extra_args":{"type":"array","items":{"type":"string"}},"enabled":{"type":"boolean"},"agent_id":{"type":"string"},"bind_tools":{"type":"boolean"},"bind_prompts":{"type":"boolean"},"bind_resources":{"type":"boolean"}}"#,
        ),
        "import_mcp_tool" => (
            "Persist a discovered MCP tool import manually.",
            r#"{"import_id":{"type":"string"},"server_id":{"type":"string"},"tool_name":{"type":"string"},"description":{"type":"string"},"parameters_schema":{"type":"string"}}"#,
        ),
        "import_mcp_prompt" => (
            "Persist a discovered MCP prompt import manually.",
            r#"{"import_id":{"type":"string"},"server_id":{"type":"string"},"prompt_name":{"type":"string"},"description":{"type":"string"},"arguments_schema":{"type":"string"}}"#,
        ),
        "import_mcp_resource" => (
            "Persist a discovered MCP resource import manually.",
            r#"{"import_id":{"type":"string"},"server_id":{"type":"string"},"resource_uri":{"type":"string"},"description":{"type":"string"},"mime_type":{"type":"string"}}"#,
        ),
        "bind_mcp_import" => (
            "Bind an imported MCP tool, prompt, or resource to an agent so it becomes visible and usable.",
            r#"{"server_id":{"type":"string"},"primitive_kind":{"type":"string","enum":["tool","prompt","resource"]},"target_name":{"type":"string"},"agent_id":{"type":"string"}}"#,
        ),
        "invoke_mcp_tool" => (
            "Invoke a bound imported MCP tool for the current agent.",
            r#"{"server_id":{"type":"string"},"tool_name":{"type":"string"},"input":{"type":"object","additionalProperties":true}}"#,
        ),
        "render_mcp_prompt" => (
            "Render a bound imported MCP prompt for the current agent.",
            r#"{"server_id":{"type":"string"},"prompt_name":{"type":"string"},"arguments":{"type":"object","additionalProperties":true}}"#,
        ),
        "read_mcp_resource" => (
            "Read a bound imported MCP resource for the current agent.",
            r#"{"server_id":{"type":"string"},"resource_uri":{"type":"string"}}"#,
        ),
        "browser_profile_create" => (
            "Create a managed browser profile for later authenticated or read-only browsing.",
            r#"{"profile_id": {"type":"string","description":"Stable profile id"}, "display_name": {"type":"string","description":"Optional human-friendly name"}, "mode": {"type":"string","enum":["ephemeral","managed_persistent","attached_external","extension_bound"],"description":"Browser profile mode"}, "engine": {"type":"string","enum":["chromium","chrome","edge","safari_bridge"],"description":"Browser engine"}, "allowed_domains": {"type":"array","items":{"type":"string"}}, "auth_enabled": {"type":"boolean"}, "write_enabled": {"type":"boolean"}, "persistent": {"type":"boolean"}, "attached_source": {"type":"string"}, "extension_binding_id": {"type":"string"}}"#,
        ),
        "browser_profile_list" => ("List managed browser profiles available to the runtime.", r#"{}"#),
        "browser_profile_use" => ("Bind a managed browser profile to the current session and agent.", r#"{"profile_id":{"type":"string"}}"#),
        "browser_session_start" => ("Launch a managed browser session using a stored browser profile.", r#"{"profile_id":{"type":"string"},"url":{"type":"string"}}"#),
        "browser_session_list" => ("List browser sessions for the current agent and session.", r#"{}"#),
        "browser_session_status" => ("Inspect a specific browser session record by id.", r#"{"browser_session_id":{"type":"string"}}"#),
        "browser_open" => ("Open a URL in a managed browser session.", r#"{"browser_session_id":{"type":"string"},"url":{"type":"string"}}"#),
        "browser_snapshot" => ("Capture a DOM snapshot artifact for a URL or browser session.", r#"{"browser_session_id":{"type":"string"},"url":{"type":"string"}}"#),
        "browser_extract" => ("Extract readable text from a URL or browser session.", r#"{"browser_session_id":{"type":"string"},"url":{"type":"string"}}"#),
        "browser_screenshot" => ("Capture a screenshot artifact for a URL or browser session.", r#"{"browser_session_id":{"type":"string"},"url":{"type":"string"}}"#),
        "browser_act" => ("Perform a typed browser action like navigate, click, type, select, scroll, or wait.", r#"{"browser_session_id":{"type":"string"},"action":{"type":"string"},"selector":{"type":"string"},"value":{"type":"string"},"url":{"type":"string"}}"#),
        "browser_download" => ("Download remote content through a managed browser workflow.", r#"{"browser_session_id":{"type":"string"},"url":{"type":"string"}}"#),
        "computer_profile_list" => ("List configured computer execution profiles.", r#"{}"#),
        "computer_session_start" => ("Prepare or reuse a computer-runtime session.", r#"{"profile_id":{"type":"string"},"target_window_id":{"type":"string"}}"#),
        "computer_session_list" => ("List computer-runtime sessions for the current agent.", r#"{}"#),
        "computer_capture" | "computer_screenshot" => ("Capture a screenshot artifact from the local computer runtime.", r#"{"computer_session_id":{"type":"string"},"profile_id":{"type":"string"}}"#),
        "computer_act" => ("Perform a local desktop computer-runtime action such as pointer move, pointer click, keyboard typing, key press, clipboard read, or clipboard write.", r#"{"computer_session_id":{"type":"string"},"profile_id":{"type":"string"},"target_window_id":{"type":"string"},"action":{"type":"string","enum":["pointer_move","pointer_click","keyboard_type","key_press","clipboard_read","clipboard_write"]},"x":{"type":"integer"},"y":{"type":"integer"},"button":{"type":"string","enum":["left","right","middle"]},"text":{"type":"string"},"key":{"type":"string"}}"#),
        "crawl_page" => ("Crawl a single page, extract content, and update website memory.", r#"{"url":{"type":"string"},"capture_screenshots":{"type":"boolean"},"change_detection":{"type":"boolean"}}"#),
        "crawl_site" => ("Crawl a site within the requested scope and update website memory.", r#"{"url":{"type":"string"},"scope":{"type":"string"},"allowed_domains":{"type":"array","items":{"type":"string"}},"max_depth":{"type":"integer"},"max_pages":{"type":"integer"},"capture_screenshots":{"type":"boolean"},"change_detection":{"type":"boolean"}}"#),
        "watch_page" => ("Schedule periodic monitoring for a single page.", r#"{"url":{"type":"string"},"schedule":{"type":"object"},"agent_id":{"type":"string"},"capture_screenshots":{"type":"boolean"},"change_detection":{"type":"boolean"}}"#),
        "watch_site" => ("Schedule periodic monitoring for a site.", r#"{"url":{"type":"string"},"schedule":{"type":"object"},"agent_id":{"type":"string"},"capture_screenshots":{"type":"boolean"},"change_detection":{"type":"boolean"}}"#),
        "set_domain_access_decision" => ("Persist a domain access decision for a target agent.", r#"{"domain":{"type":"string"},"decision":{"type":"string"},"action_family":{"type":"string"},"scope":{"type":"string"},"agent_id":{"type":"string"},"reason":{"type":"string"}}"#),
        _ => return,
    };

    tool_registry
        .register_with_embedding(
        CachedTool {
            name: tool_name.to_string(),
            description: desc.into(),
            parameters_schema: schema.into(),
            embedding: Vec::new(),
            requires_strict_schema: false,
            streaming_safe: false,
            parallel_safe: true,
            modalities: vec![aria_core::ToolModality::Text],
        },
        embedder,
    )
    .unwrap_or_else(|e| panic!("invalid discoverable tool schema for {}: {}", tool_name, e));
    vector_store.index_tool_description(
        tool_name.to_string(),
        desc.to_string(),
        embedder.embed(&format!("{} {}", tool_name, desc)),
        tool_name,
        vec![owner_tag.to_string()],
    );
}

fn handle_cli_control_command(
    req: &AgentRequest,
    config: &Config,
    llm_pool: &LlmBackendPool,
    agent_store: &AgentConfigStore,
    session_memory: &aria_ssmu::SessionMemory,
) -> Option<String> {
    handle_shared_control_command(req, config, llm_pool, agent_store, session_memory)
        .map(|output| output.text)
}

async fn handle_runtime_control_command(
    req: &AgentRequest,
    config: &Config,
    llm_pool: &LlmBackendPool,
    session_memory: &aria_ssmu::SessionMemory,
    session_steering_tx: Option<
        &dashmap::DashMap<
            String,
            tokio::sync::mpsc::Sender<aria_intelligence::SteeringCommand>,
        >,
    >,
) -> Option<ControlCommandOutput> {
    let text = req.content.as_text()?.trim();
    let intent = aria_core::parse_control_intent(text, req.channel)?;
    let sessions_dir = Path::new(&config.ssmu.sessions_dir);
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    let session_uuid = uuid::Uuid::from_bytes(req.session_id);

    let plain = |text: String| ControlCommandOutput {
        text,
        parse_mode: None,
        reply_markup: None,
    };

    match intent {
        aria_core::ControlIntent::ListRuns => Some(match store.list_agent_runs_for_session(session_uuid) {
            Ok(runs) if runs.is_empty() => plain("No sub-agent runs found for this session.".into()),
            Ok(runs) => {
                let mut lines = vec!["Sub-agent runs for this session:".to_string()];
                for run in runs {
                    lines.push(format!(
                        "• {} [{}] agent={} created_at={}",
                        run.run_id,
                        serde_json::to_string(&run.status)
                            .unwrap_or_else(|_| "\"unknown\"".into())
                            .replace('"', ""),
                        run.agent_id,
                        run.created_at_us
                    ));
                }
                plain(lines.join("\n"))
            }
            Err(err) => plain(format!("Failed to list runs: {}", err)),
        }),
        aria_core::ControlIntent::InspectRunTree { session_id } => Some(match session_id {
            None => plain("Usage: /run_tree <session_id>".into()),
            Some(session_id) => match inspect_agent_run_tree_json(sessions_dir, &session_id, None) {
                Ok(tree) => plain(
                    serde_json::to_string_pretty(&tree)
                        .unwrap_or_else(|_| tree.to_string()),
                ),
                Err((_, err)) => plain(format!("Failed to inspect run tree: {}", err)),
            },
        }),
        aria_core::ControlIntent::InspectRun { run_id } => Some(match run_id {
            None => plain("Usage: /run <run_id>".into()),
            Some(run_id) => match store.read_agent_run(&run_id) {
                Ok(run) => plain(format!(
                    "Run {}\nstatus={}\nagent={}\nrequested_by={}\ncreated_at={}\nstarted_at={:?}\nfinished_at={:?}\nresult={}",
                    run.run_id,
                    serde_json::to_string(&run.status)
                        .unwrap_or_else(|_| "\"unknown\"".into())
                        .replace('"', ""),
                    run.agent_id,
                    run.requested_by_agent.unwrap_or_else(|| "user".into()),
                    run.created_at_us,
                    run.started_at_us,
                    run.finished_at_us,
                    run.result
                        .and_then(|r| r.response_summary.or(r.error))
                        .unwrap_or_else(|| "<none>".into())
                )),
                Err(err) => plain(format!("Failed to read run '{}': {}", run_id, err)),
            },
        }),
        aria_core::ControlIntent::InspectRunEvents { run_id } => Some(match run_id {
            None => plain("Usage: /run_events <run_id>".into()),
            Some(run_id) => match store.list_agent_run_events(&run_id) {
                Ok(events) if events.is_empty() => plain(format!("No events for run '{}'.", run_id)),
                Ok(events) => {
                    let mut lines = vec![format!("Events for run {}:", run_id)];
                    for event in events {
                        lines.push(format!(
                            "• {} [{}] {}",
                            event.event_id,
                            serde_json::to_string(&event.kind)
                                .unwrap_or_else(|_| "\"unknown\"".into())
                                .replace('"', ""),
                            event.summary
                        ));
                    }
                    plain(lines.join("\n"))
                }
                Err(err) => plain(format!("Failed to list run events: {}", err)),
            },
        }),
        aria_core::ControlIntent::InspectMailbox { run_id } => Some(match run_id {
            None => plain("Usage: /mailbox <run_id>".into()),
            Some(run_id) => match store.list_agent_mailbox_messages(&run_id) {
                Ok(messages) if messages.is_empty() => {
                    plain(format!("No mailbox messages for run '{}'.", run_id))
                }
                Ok(messages) => {
                    let mut lines = vec![format!("Mailbox for run {}:", run_id)];
                    for msg in messages {
                        lines.push(format!(
                            "• from={} to={} delivered={} {}",
                            msg.from_agent_id.as_deref().unwrap_or("unknown"),
                            msg.to_agent_id.as_deref().unwrap_or("unknown"),
                            msg.delivered,
                            msg.body
                        ));
                    }
                    plain(lines.join("\n"))
                }
                Err(err) => plain(format!("Failed to read mailbox: {}", err)),
            },
        }),
        aria_core::ControlIntent::CancelRun { run_id } => Some(match run_id {
            None => plain("Usage: /run_cancel <run_id>".into()),
            Some(run_id) => {
                let now_us = chrono::Utc::now().timestamp_micros() as u64;
                match store.cancel_agent_run_tree(&run_id, "cancelled by user command", now_us) {
                    Ok(updated) if updated.is_empty() => plain(format!("Run '{}' not found.", run_id)),
                    Ok(updated) => {
                        let root = updated.last().expect("non-empty updated list");
                        plain(format!(
                            "Run tree rooted at '{}' is now {:?} ({} run(s) updated).",
                            root.run_id,
                            root.status,
                            updated.len()
                        ))
                    }
                    Err(err) => plain(format!("Failed to cancel run: {}", err)),
                }
            }
        }),
        aria_core::ControlIntent::RetryRun { run_id } => Some(match run_id {
            None => plain("Usage: /run_retry <run_id>".into()),
            Some(run_id) => match store.read_agent_run(&run_id) {
                Ok(original) => {
                    let now_us = chrono::Utc::now().timestamp_micros() as u64;
                    match store.retry_agent_run(
                        &run_id,
                        original.requested_by_agent.as_deref(),
                        now_us,
                    ) {
                        Err(err) => plain(format!("Failed to queue retry run: {}", err)),
                        Ok(Some(retried)) => plain(format!(
                            "Retry queued: new run '{}' created from '{}'.",
                            retried.run_id, original.run_id
                        )),
                        Ok(None) => plain(format!("Run '{}' not found.", run_id)),
                    }
                }
                Err(err) => plain(format!("Retry lookup failed: {}", err)),
            },
        }),
        aria_core::ControlIntent::TakeoverRun { run_id, agent_id } => Some(match (run_id, agent_id) {
            (None, _) | (_, None) => plain("Usage: /run_takeover <run_id> <agent_id>".into()),
            (Some(run_id), Some(agent_id)) => match store.read_agent_run(&run_id) {
                Ok(original) => {
                    let now_us = chrono::Utc::now().timestamp_micros() as u64;
                    match store.take_over_agent_run(
                        &run_id,
                        &agent_id,
                        original.requested_by_agent.as_deref(),
                        now_us,
                    ) {
                        Err(err) => plain(format!("Failed to queue takeover run: {}", err)),
                        Ok(Some(takeover)) => plain(format!(
                            "Takeover queued: new run '{}' created from '{}' for agent '{}'.",
                            takeover.run_id, original.run_id, takeover.agent_id
                        )),
                        Ok(None) => plain(format!("Run '{}' not found.", run_id)),
                    }
                }
                Err(err) => plain(format!("Takeover lookup failed: {}", err)),
            },
        }),
        aria_core::ControlIntent::ListProviderHealth => {
            Some(render_provider_health_for_channel(req.channel, llm_pool))
        }
        aria_core::ControlIntent::ListWorkspaceLocks => Some(render_workspace_locks_for_channel(
            req.channel,
        )),
        aria_core::ControlIntent::InstallSkill { signed_module_json } => {
            Some(match signed_module_json {
                None => plain("Usage: /install_skill <SignedModule JSON>".into()),
                Some(json_part) => match serde_json::from_str::<aria_skill_runtime::SignedModule>(&json_part) {
                    Ok(signed) => {
                        if let Err(err) = aria_skill_runtime::verify_module(&signed) {
                            plain(format!("Verification failed: {}", err))
                        } else {
                            let hash = aria_skill_runtime::wasm_module_hash(&signed.bytes);
                            let hex_hash = hex::encode(&hash[..8]);
                            let target = format!("./tools/{}.wasm", hex_hash);
                            match std::fs::write(&target, &signed.bytes) {
                                Ok(()) => plain(format!("Skill installed successfully as '{}'.", target)),
                                Err(err) => plain(format!("Failed to save tool: {}", err)),
                            }
                        }
                    }
                    Err(err) => plain(format!("Invalid SignedModule JSON: {}", err)),
                },
            })
        }
        aria_core::ControlIntent::StopCurrent => Some(match session_steering_tx {
            Some(map) => {
                if let Some(tx) = map.get(&session_uuid.to_string()) {
                    let _ = tx.send(aria_intelligence::SteeringCommand::Abort).await;
                    plain("Signal sent: aborting current operation.".into())
                } else {
                    plain("No active operation to stop.".into())
                }
            }
            None => plain("Stop is not available on this runtime path.".into()),
        }),
        aria_core::ControlIntent::Pivot { instructions } => Some(match session_steering_tx {
            Some(map) => {
                let Some(instructions) = instructions else {
                    return Some(plain("Usage: /pivot <new instructions>".into()));
                };
                if let Some(tx) = map.get(&session_uuid.to_string()) {
                    let _ = tx
                        .send(aria_intelligence::SteeringCommand::Pivot(instructions.clone()))
                        .await;
                    plain("Signal sent: pivoting current operation.".into())
                } else {
                    plain("No active operation to pivot.".into())
                }
            }
            None => plain("Pivot is not available on this runtime path.".into()),
        }),
        _ => {
            let _ = session_memory;
            None
        }
    }
}

#[derive(Debug, Clone)]
struct ControlCommandOutput {
    text: String,
    parse_mode: Option<&'static str>,
    reply_markup: Option<serde_json::Value>,
}

fn render_agent_list_for_channel(
    channel: GatewayChannel,
    sessions_dir: &Path,
    agent_store: &AgentConfigStore,
    current_agent: Option<&str>,
) -> ControlCommandOutput {
    let presence_by_agent = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_agent_presence()
        .unwrap_or_default()
        .into_iter()
        .map(|record| (record.agent_id.clone(), record))
        .collect::<std::collections::HashMap<_, _>>();
    match channel {
        GatewayChannel::Telegram => {
            let escape = |s: &str| -> String {
                s.replace("&", "&amp;")
                    .replace("<", "&lt;")
                    .replace(">", "&gt;")
            };
            let mut lines = vec!["<b>Available agents:</b>".to_string()];
            let mut keyboard = Vec::new();
            for cfg in agent_store.all() {
                let presence = presence_by_agent.get(&cfg.id);
                let presence_note = presence
                    .map(|record| {
                        format!(
                            " [{}{}]",
                            serde_json::to_string(&record.availability)
                                .unwrap_or_else(|_| "\"available\"".into())
                                .replace('"', ""),
                            if record.active_run_count == 0 {
                                String::new()
                            } else {
                                format!(", active={}", record.active_run_count)
                            }
                        )
                    })
                    .unwrap_or_default();
                lines.push(format!(
                    "• <b>{}</b>{}: {}",
                    escape(&cfg.id),
                    escape(&presence_note),
                    escape(&cfg.description)
                ));
                keyboard.push(vec![serde_json::json!({
                    "text": format!("Switch to {}", cfg.id),
                    "callback_data": format!("/agent {}", cfg.id)
                })]);
            }
            if let Some(agent) = current_agent {
                lines.push(format!("\n<b>Current agent:</b> {}", escape(agent)));
            }
            ControlCommandOutput {
                text: lines.join("\n"),
                parse_mode: Some("HTML"),
                reply_markup: Some(serde_json::json!({ "inline_keyboard": keyboard })),
            }
        }
        _ => {
            let mut lines = vec!["Available agents:".to_string()];
            for cfg in agent_store.all() {
                let presence = presence_by_agent.get(&cfg.id);
                let suffix = presence
                    .map(|record| {
                        let availability = serde_json::to_string(&record.availability)
                            .unwrap_or_else(|_| "\"available\"".into())
                            .replace('"', "");
                        if record.active_run_count == 0 {
                            format!(" [{}]", availability)
                        } else {
                            format!(" [{}, active={}]", availability, record.active_run_count)
                        }
                    })
                    .unwrap_or_default();
                lines.push(format!(" - {}{}: {}", cfg.id, suffix, cfg.description));
            }
            if let Some(agent) = current_agent {
                lines.push(format!("Current agent override: {}", agent));
            }
            ControlCommandOutput {
                text: lines.join("\n"),
                parse_mode: None,
                reply_markup: None,
            }
        }
    }
}

fn render_session_summary_for_channel(
    channel: GatewayChannel,
    session_uuid: uuid::Uuid,
    current_agent: Option<&str>,
    current_model: Option<&str>,
) -> ControlCommandOutput {
    let agent_label = current_agent.unwrap_or("default");
    let model_label = current_model.unwrap_or("default");
    match channel {
        GatewayChannel::Telegram => ControlCommandOutput {
            text: format!(
                "<b>Session</b> <code>{}</code>\nagent_override={}\nmodel_override={}",
                session_uuid,
                agent_label,
                model_label,
            ),
            parse_mode: Some("HTML"),
            reply_markup: None,
        },
        _ => ControlCommandOutput {
            text: format!(
                "Session {}\nagent_override={}\nmodel_override={}",
                session_uuid, agent_label, model_label,
            ),
            parse_mode: None,
            reply_markup: None,
        },
    }
}

fn render_pending_approvals_for_channel(
    channel: GatewayChannel,
    pending: Vec<(usize, aria_core::ApprovalRecord, ApprovalDisplayDescriptor, String)>,
) -> ControlCommandOutput {
    if pending.is_empty() {
        return ControlCommandOutput {
            text: "No pending approvals.".to_string(),
            parse_mode: None,
            reply_markup: None,
        };
    }

    match channel {
        GatewayChannel::Telegram => {
            let mut lines = vec!["<b>Pending approvals:</b>".to_string()];
            let mut keyboard = Vec::new();
            for (idx, record, descriptor, handle) in pending.into_iter().take(10) {
                let target = descriptor
                    .target_summary
                    .as_deref()
                    .map(|value| format!(" ({})", value))
                    .unwrap_or_default();
                lines.push(format!(
                    "{}. {}{} [<code>{}</code>]",
                    idx, descriptor.action_summary, target, handle
                ));
                keyboard.push(vec![
                    serde_json::json!({
                        "text": format!("Approve {}", idx),
                        "callback_data": format!("/approve {}", handle)
                    }),
                    serde_json::json!({
                        "text": format!("Deny {}", idx),
                        "callback_data": format!("/deny {}", handle)
                    }),
                    serde_json::json!({
                        "text": record.tool_name,
                        "callback_data": format!("/approve {}", handle)
                    }),
                ]);
            }
            ControlCommandOutput {
                text: lines.join("\n"),
                parse_mode: Some("HTML"),
                reply_markup: Some(serde_json::json!({ "inline_keyboard": keyboard })),
            }
        }
        _ => {
            let mut lines = vec!["Pending approvals:".to_string()];
            for (idx, record, descriptor, handle) in pending {
                let target = descriptor
                    .target_summary
                    .as_deref()
                    .map(|value| format!(" ({})", value))
                    .unwrap_or_default();
                lines.push(format!(
                    " {}. {}{} [#{} | {}]",
                    idx, descriptor.action_summary, target, handle, record.approval_id
                ));
            }
            lines.push(
                "Approve with `/approve <number>`, `/approve <handle>`, or `/approve <approval_id>`."
                    .to_string(),
            );
            lines.push(
                "Deny with `/deny <number>`, `/deny <handle>`, or `/deny <approval_id>`."
                    .to_string(),
            );
            ControlCommandOutput {
                text: lines.join("\n"),
                parse_mode: None,
                reply_markup: None,
            }
        }
    }
}

fn render_workspace_locks_for_channel(channel: GatewayChannel) -> ControlCommandOutput {
    let locks = workspace_lock_manager().snapshot();
    match channel {
        GatewayChannel::Telegram => {
            if locks.is_empty() {
                return ControlCommandOutput {
                    text: "<b>Workspace locks:</b>\nNo active workspace locks.".into(),
                    parse_mode: Some("HTML"),
                    reply_markup: None,
                };
            }
            let mut lines = vec!["<b>Workspace locks:</b>".to_string()];
            for lock in locks.iter().take(8) {
                let holder = lock.current_holder.as_deref().unwrap_or("unknown");
                lines.push(format!(
                    "• <code>{}</code> holder=<code>{}</code> waiters={} active={}",
                    escape_html_fragment(&lock.workspace_key),
                    escape_html_fragment(holder),
                    lock.waiting_runs,
                    lock.active_holders
                ));
            }
            ControlCommandOutput {
                text: lines.join("\n"),
                parse_mode: Some("HTML"),
                reply_markup: None,
            }
        }
        _ => {
            if locks.is_empty() {
                return ControlCommandOutput {
                    text: "Workspace locks:\nNo active workspace locks.".into(),
                    parse_mode: None,
                    reply_markup: None,
                };
            }
            let mut lines = vec!["Workspace locks:".to_string()];
            for lock in locks.iter().take(8) {
                let holder = lock.current_holder.as_deref().unwrap_or("unknown");
                lines.push(format!(
                    " - {} [holder={} | waiters={} | active={}]",
                    lock.workspace_key, holder, lock.waiting_runs, lock.active_holders
                ));
            }
            ControlCommandOutput {
                text: lines.join("\n"),
                parse_mode: None,
                reply_markup: None,
            }
        }
    }
}

fn escape_html_fragment(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn render_provider_health_for_channel(
    channel: GatewayChannel,
    llm_pool: &LlmBackendPool,
) -> ControlCommandOutput {
    let states = llm_pool.provider_circuit_state();
    match channel {
        GatewayChannel::Telegram => {
            if states.is_empty() {
                return ControlCommandOutput {
                    text: "<b>Provider health:</b>\nNo provider circuits are open.".into(),
                    parse_mode: Some("HTML"),
                    reply_markup: None,
                };
            }
            let mut lines = vec!["<b>Provider health:</b>".to_string()];
            for state in states.iter().take(8) {
                lines.push(format!(
                    "• <code>{}</code> open={} failures={} backends=<code>{}</code>",
                    escape_html_fragment(&state.provider_family),
                    state.circuit_open,
                    state.consecutive_failures,
                    escape_html_fragment(&state.impacted_backends.join(","))
                ));
            }
            ControlCommandOutput {
                text: lines.join("\n"),
                parse_mode: Some("HTML"),
                reply_markup: None,
            }
        }
        _ => {
            if states.is_empty() {
                return ControlCommandOutput {
                    text: "Provider health:\nNo provider circuits are open.".into(),
                    parse_mode: None,
                    reply_markup: None,
                };
            }
            let mut lines = vec!["Provider health:".to_string()];
            for state in states.iter().take(8) {
                lines.push(format!(
                    " - {} [open={} | failures={} | backends={}]",
                    state.provider_family,
                    state.circuit_open,
                    state.consecutive_failures,
                    state.impacted_backends.join(",")
                ));
            }
            ControlCommandOutput {
                text: lines.join("\n"),
                parse_mode: None,
                reply_markup: None,
            }
        }
    }
}

fn handle_shared_control_command(
    req: &AgentRequest,
    config: &Config,
    llm_pool: &LlmBackendPool,
    agent_store: &AgentConfigStore,
    session_memory: &aria_ssmu::SessionMemory,
) -> Option<ControlCommandOutput> {
    let text = req.content.as_text()?.trim();
    if text.is_empty() {
        return None;
    }
    let intent = aria_core::parse_control_intent(text, req.channel)?;
    let session_uuid = uuid::Uuid::from_bytes(req.session_id);
    let (current_agent, current_model) = get_effective_session_overrides(
        session_memory,
        req.session_id,
        req.channel,
        &req.user_id,
    )
    .unwrap_or((None, None));
    let current_agent = normalize_override_value(current_agent);
    let current_model = normalize_override_value(current_model);

    match intent {
        aria_core::ControlIntent::ListAgents => {
            return Some(render_agent_list_for_channel(
                req.channel,
                Path::new(&config.ssmu.sessions_dir),
                agent_store,
                current_agent.as_deref(),
            ));
        }
        aria_core::ControlIntent::InspectSession => {
            return Some(render_session_summary_for_channel(
                req.channel,
                session_uuid,
                current_agent.as_deref(),
                current_model.as_deref(),
            ));
        }
        aria_core::ControlIntent::ClearSession => {
            let _ = session_memory.clear_history(&session_uuid);
            let _ = persist_session_overrides(
                session_memory,
                req.session_id,
                req.channel,
                &req.user_id,
                Some(String::new()),
                Some(String::new()),
            );
            record_learning_reward(
                &config.learning,
                Path::new(&config.ssmu.sessions_dir),
                req.request_id,
                req.session_id,
                RewardKind::OverrideApplied,
                Some("session cleared".to_string()),
                req.timestamp_us,
            );
            return Some(ControlCommandOutput {
                text: "Session history cleared. Agent/model overrides were reset to default routing."
                    .to_string(),
                parse_mode: None,
                reply_markup: None,
            });
        }
        aria_core::ControlIntent::ListApprovals => {
            let pending = list_cli_pending_approvals(
                Path::new(&config.ssmu.sessions_dir),
                req.session_id,
                &req.user_id,
            );
            return Some(render_pending_approvals_for_channel(req.channel, pending));
        }
        aria_core::ControlIntent::ListProviderHealth => {
            return Some(render_provider_health_for_channel(req.channel, llm_pool));
        }
        aria_core::ControlIntent::ListWorkspaceLocks => {
            return Some(render_workspace_locks_for_channel(req.channel));
        }
        aria_core::ControlIntent::SwitchAgent {
            agent_id: Some(agent_name),
        } => {
            if matches!(agent_name.as_str(), "clear" | "reset") {
                let _ = persist_session_overrides(
                    session_memory,
                    req.session_id,
                    req.channel,
                    &req.user_id,
                    Some(String::new()),
                    Some(String::new()),
                );
                record_learning_reward(
                    &config.learning,
                    Path::new(&config.ssmu.sessions_dir),
                    req.request_id,
                    req.session_id,
                    RewardKind::OverrideApplied,
                    Some("agent override cleared".to_string()),
                    req.timestamp_us,
                );
                return Some(ControlCommandOutput {
                    text: "Agent/model override cleared. Session history was not cleared; use /session clear to reset the session."
                        .to_string(),
                    parse_mode: None,
                    reply_markup: None,
                });
            }
            if agent_store.get(&agent_name).is_some() {
                let _ = persist_session_overrides(
                    session_memory,
                    req.session_id,
                    req.channel,
                    &req.user_id,
                    Some(agent_name.clone()),
                    None,
                );
                record_learning_reward(
                    &config.learning,
                    Path::new(&config.ssmu.sessions_dir),
                    req.request_id,
                    req.session_id,
                    RewardKind::OverrideApplied,
                    Some(format!("agent override set to {}", agent_name)),
                    req.timestamp_us,
                );
                return Some(ControlCommandOutput {
                    text: format!("Session override set to agent: {}.", agent_name),
                    parse_mode: None,
                    reply_markup: None,
                });
            }
            return Some(ControlCommandOutput {
                text: format!("Agent '{}' not found. Use /agents to list.", agent_name),
                parse_mode: None,
                reply_markup: None,
            });
        }
        aria_core::ControlIntent::SwitchAgent { agent_id: None } => {
            return Some(ControlCommandOutput {
                text:
                    "Usage: /agent <persona_name> (for example: /agent developer, /agent omni)"
                        .to_string(),
                parse_mode: None,
                reply_markup: None,
            });
        }
        _ => {}
    }

    // Parsed as control intent but not handled by CLI control router;
    // caller may route it to dedicated handlers (e.g. approval flow).
    if text.starts_with('/') {
        return None;
    }

    None
}

fn list_cli_pending_approvals(
    sessions_dir: &Path,
    session_id: [u8; 16],
    user_id: &str,
) -> Vec<(usize, aria_core::ApprovalRecord, ApprovalDisplayDescriptor, String)> {
    RuntimeStore::for_sessions_dir(sessions_dir)
        .list_approvals(
            Some(session_id),
            Some(user_id),
            Some(aria_core::ApprovalStatus::Pending),
        )
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(idx, record)| {
            let descriptor = build_approval_descriptor(&record);
            let handle = ensure_approval_handle(sessions_dir, &record)
                .unwrap_or_else(|_| record.approval_id.clone());
            (idx + 1, record, descriptor, handle)
        })
        .collect()
}

fn resolve_cli_approval_id(
    sessions_dir: &Path,
    session_id: [u8; 16],
    user_id: &str,
    token: &str,
) -> Result<String, String> {
    if token.chars().all(|c| c.is_ascii_digit()) {
        let index = token
            .parse::<usize>()
            .map_err(|_| format!("Invalid approval selection '{}'.", token))?;
        let pending = list_cli_pending_approvals(sessions_dir, session_id, user_id);
        let Some((_, record, _, _)) = pending.into_iter().find(|(idx, _, _, _)| *idx == index) else {
            return Err(format!("No pending approval at index {}.", index));
        };
        Ok(record.approval_id)
    } else {
        resolve_approval_selector(sessions_dir, session_id, user_id, token)
    }
}

fn apply_session_scope_policy(req: &mut AgentRequest, config: &Config) {
    let scoped = aria_core::derive_scoped_session_id(
        req.session_id,
        req.channel,
        &req.user_id,
        config.gateway.session_scope_policy,
    );
    req.session_id = scoped;
}

async fn handle_cli_approval_command(
    req: &AgentRequest,
    config: &Config,
    session_memory: &aria_ssmu::SessionMemory,
    vault: &Arc<aria_vault::CredentialVault>,
    cedar: &Arc<aria_policy::CedarEvaluator>,
    tx_cron: &tokio::sync::mpsc::Sender<aria_intelligence::CronCommand>,
) -> Option<String> {
    let text = req.content.as_text()?.trim();
    if text.is_empty() {
        return None;
    }
    let (approving, selector) = match aria_core::parse_control_intent(text, req.channel) {
        Some(aria_core::ControlIntent::ResolveApproval {
            decision,
            target: Some(target),
            ..
        }) => (
            matches!(decision, aria_core::ApprovalResolutionDecision::Approve),
            target,
        ),
        Some(aria_core::ControlIntent::ResolveApproval { target: None, .. }) => {
            return Some(
                "Usage: /approve <approval_id|number> or /deny <approval_id|number>".to_string(),
            );
        }
        _ => return None,
    };

    let sessions_dir = Path::new(&config.ssmu.sessions_dir);
    let approval_id =
        match resolve_cli_approval_id(sessions_dir, req.session_id, &req.user_id, &selector) {
            Ok(id) => id,
            Err(err) => return Some(err),
        };
    let decision = if approving {
        aria_core::ApprovalResolutionDecision::Approve
    } else {
        aria_core::ApprovalResolutionDecision::Deny
    };
    let record = match resolve_approval_record(sessions_dir, &approval_id, decision) {
        Ok(record) => record,
        Err(err) => return Some(err),
    };

    if !approving {
        return Some(format!("Denied approval '{}'.", approval_id));
    }
    if record.tool_name == AGENT_ELEVATION_TOOL_NAME {
        let requested_agent = serde_json::from_str::<serde_json::Value>(&record.arguments_json)
            .ok()
            .and_then(|value| {
                value
                    .get("agent_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| record.agent_id.clone());
        let now_us = chrono::Utc::now().timestamp_micros() as u64;
        let grant = aria_core::ElevationGrant {
            session_id: req.session_id,
            user_id: req.user_id.clone(),
            agent_id: requested_agent.clone(),
            granted_at_us: now_us,
            expires_at_us: Some(now_us + 3_600_000_000),
        };
        let _ = write_elevation_grant(sessions_dir, &grant);
        let _ = persist_session_overrides(
            session_memory,
            req.session_id,
            req.channel,
            &req.user_id,
            Some(requested_agent.clone()),
            None,
        );
        return Some(format!(
            "Approved elevation for agent '{}'.",
            requested_agent
        ));
    }
    let (current_agent, _) = get_effective_session_overrides(
        session_memory,
        req.session_id,
        req.channel,
        &req.user_id,
    )
    .unwrap_or((None, None));
    let invoking_agent = normalize_override_value(current_agent).unwrap_or_else(|| "omni".into());
    let executor = MultiplexToolExecutor::new(
        vault.clone(),
        invoking_agent,
        req.session_id,
        req.user_id.clone(),
        req.channel,
        tx_cron.clone(),
        session_memory.clone(),
        cedar.clone(),
        sessions_dir.to_path_buf(),
        None,
        None,
        resolve_request_timezone(config, &req.user_id),
    );
    let call = aria_intelligence::ToolCall {
        invocation_id: None,
        name: record.tool_name.clone(),
        arguments: record.arguments_json.clone(),
    };
    let result = executor.execute(&call).await;
    Some(match result {
        Ok(value) => format!(
            "Approved '{}'.\n{}",
            record.tool_name,
            value.render_for_prompt()
        ),
        Err(err) => format!(
            "Approved '{}', but execution failed: {}",
            record.tool_name, err
        ),
    })
}

pub(crate) fn format_orchestrator_error_for_user(message: &str) -> String {
    if let Some(path) = message
        .strip_prefix("tool error: tool 'read_file' denied by policy for resource '")
        .and_then(|rest| rest.strip_suffix('\''))
    {
        return format!(
            "Access denied: read_file is not permitted for '{}'.",
            path
        );
    }
    if let Some(path) = message
        .strip_prefix("tool error: tool 'write_file' denied by policy for resource '")
        .and_then(|rest| rest.strip_suffix('\''))
    {
        return format!(
            "Access denied: write_file is not permitted for '{}'.",
            path
        );
    }
    if let Some(resource) = message
        .strip_prefix("tool error: policy denied action 'web_domain_fetch' on resource '")
        .and_then(|rest| rest.strip_suffix('\''))
    {
        return format!(
            "Domain access is not approved for '{}'. Approve the domain first, then retry.",
            resource
        );
    }
    if let Some(resource) = message
        .strip_prefix("tool error: policy denied action 'web_domain_crawl' on resource '")
        .and_then(|rest| rest.strip_suffix('\''))
    {
        return format!(
            "Crawl access is not approved for '{}'. Approve the domain first, then retry.",
            resource
        );
    }
    if let Some(resource) = message
        .strip_prefix("tool error: policy denied action 'browser_profile_use' on resource '")
        .and_then(|rest| rest.strip_suffix('\''))
    {
        return format!(
            "Browser profile access denied for '{}'.",
            resource
        );
    }
    if let Some(tool) = message.strip_prefix("tool error: APPROVAL_REQUIRED::") {
        return format!(
            "Approval required before '{}' can run. Inspect pending approvals and approve the request, then retry.",
            tool
        );
    }
    message.to_string()
}
