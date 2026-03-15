#!/usr/bin/env bash
# Build ARIA-X production release bundle
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OUT_DIR="${OUT_DIR:-$PROJECT_ROOT/dist}"
BUNDLE_NAME="aria-x-$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)"

cd "$PROJECT_ROOT"

echo "[build] Building release binary..."
# Ensure we use project target dir (not sandbox cache)
unset CARGO_TARGET_DIR
cargo build --release -p aria-x

echo "[build] Creating bundle at $OUT_DIR/$BUNDLE_NAME"
mkdir -p "$OUT_DIR/$BUNDLE_NAME"

# Copy binary (must come from project target, not sandbox)
rm -f "$OUT_DIR/$BUNDLE_NAME/aria-x"  # Remove symlink if present
cp "$PROJECT_ROOT/target/release/aria-x" "$OUT_DIR/$BUNDLE_NAME/"

# Copy config and assets (use production paths for default config.toml in bundle)
cp -r aria-x/config.example.toml "$OUT_DIR/$BUNDLE_NAME/" 2>/dev/null || true
cp -r aria-policy/policies "$OUT_DIR/$BUNDLE_NAME/" 2>/dev/null || true
cp -r agents "$OUT_DIR/$BUNDLE_NAME/" 2>/dev/null || true
mkdir -p "$OUT_DIR/$BUNDLE_NAME/workspace/sessions"

# Create config.toml and config.production.toml (both use bundle-relative paths)
cat > "$OUT_DIR/$BUNDLE_NAME/config.toml" << 'EOF'
# ARIA-X Production Configuration
# Copy to config.toml and customize

[llm]
backend = "openrouter"
model = "arcee-ai/trinity-large-preview:free"
max_tool_rounds = 5

[policy]
policy_path = "./policies/default.cedar"

[gateway]
adapter = "telegram"
# Token: telegram_token, TELEGRAM_BOT_TOKEN env, or telegram_token_file
telegram_token = ""
# telegram_token_file = "/run/secrets/telegram_token"  # Docker/k8s secret
telegram_port = 8080
# polling = no webhook/ngrok (default). webhook = HTTP server (needs public URL)
telegram_mode = "polling"

[mesh]
mode = "peer"
endpoints = []

[agents_dir]
path = "./agents"

[router]
confidence_threshold = 0.70
tie_break_gap = 0.05

[ssmu]
sessions_dir = "./workspace/sessions"

[scheduler]
enabled = false
tick_seconds = 1

[simulator]
enabled = false
backend = "none"

[telemetry]
enabled = true
log_level = "debug"

[node]
id = "orchestrator-1"
role = "orchestrator"
tier = "orchestrator"
EOF

cp "$OUT_DIR/$BUNDLE_NAME/config.toml" "$OUT_DIR/$BUNDLE_NAME/config.production.toml"

# Run script: ensures we run from bundle dir so config paths resolve correctly
cat > "$OUT_DIR/$BUNDLE_NAME/run.sh" << 'RUNEOF'
#!/usr/bin/env bash
cd "$(dirname "$0")"
if [[ -f ".env" ]]; then
  set -a
  # shellcheck disable=SC1091
  source ".env"
  set +a
fi
exec ./aria-x config.toml
RUNEOF
chmod +x "$OUT_DIR/$BUNDLE_NAME/run.sh"

echo "[build] Bundle ready: $OUT_DIR/$BUNDLE_NAME"
echo "[build] Run: cd $OUT_DIR/$BUNDLE_NAME && ./run.sh"
echo "[build] Or:  cd $OUT_DIR/$BUNDLE_NAME && ./aria-x config.toml"
