# HiveClaw Production Deployment Guide

This guide walks you through building the production bundle and connecting it to Telegram and Ollama.

## 1. Prerequisites

- **Rust** (latest stable): `rustup update stable`
- **Ollama** installed and running: [ollama.ai](https://ollama.ai)
- **Telegram Bot Token** from [@BotFather](https://t.me/BotFather)
- **Public URL** for webhook (ngrok, cloud VM, or your domain)

## 2. Build the Production Bundle

```bash
# From project root
./scripts/build-release.sh
```

Or manually:

```bash
cargo build --release -p aria-x
# Binary: target/release/aria-x
```

The script creates `dist/aria-x-<os>-<arch>/` with the binary, config templates, policies, and agents.

## 3. Set Up Ollama

```bash
# Install Ollama (macOS)
brew install ollama

# Start Ollama (runs in background)
ollama serve

# Pull a model (if using local fallback)
ollama pull qwen3.5:0.8b
```

Verify Ollama is running:

```bash
curl http://localhost:11434/api/tags
```

**Environment variables:**

- `OLLAMA_HOST` — default `http://localhost:11434` (override if Ollama runs elsewhere)

## 4. Configure Telegram

### 4.1 Create a Bot

1. Open Telegram and message [@BotFather](https://t.me/BotFather)
2. Send `/newbot` and follow the prompts
3. Copy the bot token (e.g. `123456789:ABCdefGHIjklMNOpqrsTUVwxyz`)

### 4.2 Production Config

Create or edit `config.toml`:

[llm]
backend = "openrouter"
model = "arcee-ai/trinity-large-preview:free"
max_tool_rounds = 5

[policy]
policy_path = "./policies/default.cedar"

[gateway]
adapter = "telegram"
telegram_token = "YOUR_BOT_TOKEN"   # Or use TELEGRAM_BOT_TOKEN env
telegram_port = 8080

[agents_dir]
path = "./agents"

[ssmu]
sessions_dir = "./workspace/sessions"

# ... rest from config.example.toml
```

**Credential resolution** (order: config → env → file):

| Method | Use case |
|--------|----------|
| `telegram_token = "..."` in config | Quick setup, avoid env |
| `TELEGRAM_BOT_TOKEN` env var | Recommended for production |
| `~/.hiveclaw/.env`, legacy `~/.aria/.env`, or `./.env` | Persistent env (e.g. `TELEGRAM_BOT_TOKEN=...`) |
| `telegram_token_file = "/path"` | Docker secrets, k8s secret mounts |

Example `.env` (in project dir or `~/.hiveclaw/`):

```
TELEGRAM_BOT_TOKEN=123456789:ABCdef...
OLLAMA_HOST=http://localhost:11434
```

### 4.3 Telegram mode: polling (default) vs webhook

**Default: `telegram_mode = "polling"`** — No webhook, no ngrok, no public URL. The gateway long-polls Telegram (`getUpdates`). Just set your bot token and run; messages are received automatically. Works locally and in production.

**Optional: `telegram_mode = "webhook"`** — Telegram POSTs updates to your server. Requires a public HTTPS URL (e.g. ngrok for local, or your domain in production). Set the webhook with `setWebhook` as below.

**If you use webhook mode**, check webhook status with `./scripts/check-telegram-webhook.sh`. For **polling mode** (default), skip webhook and ngrok.

**For webhook mode only**, use ngrok to expose your server:

```bash
# Install ngrok
brew install ngrok

# Start HiveClaw
./aria-x config.toml

# In another terminal, expose port 8080
ngrok http 8080
```

Copy the HTTPS URL (e.g. `https://abc123.ngrok.io`).

### 4.4 Register Webhook with Telegram (only if telegram_mode = "webhook")

```bash
# Replace YOUR_BOT_TOKEN and YOUR_PUBLIC_URL
curl "https://api.telegram.org/botYOUR_BOT_TOKEN/setWebhook?url=YOUR_PUBLIC_URL/webhook"
```

Example:

```bash
curl "https://api.telegram.org/bot123456789:ABCdef/setWebhook?url=https://abc123.ngrok.io/webhook"
```

Response: `{"ok":true,"result":true}`

### 4.5 Production (Cloud VM)

If you run on a server with a public IP:

1. Open port 8080 (or your `telegram_port`).
2. Use your domain (e.g. `https://aria.yourdomain.com/webhook`).
3. Set up HTTPS (nginx reverse proxy or similar).
4. Ensure the config `policy_path` and `agents_dir` use paths relative to the working directory.

## 5. Logging and Debug

**Live logs:** All output goes to stdout. For production, run with:

```bash
# All logs (debug level)
RUST_LOG=aria_x=debug ./aria-x config.toml

# Or in config.toml:
[telemetry]
enabled = true
log_level = "debug"
```

**Gateway debug:** With `debug` level you'll see webhook received, chat_id, request text, response sent, and errors.

**Save logs to file:**
```bash
RUST_LOG=aria_x=debug ./aria-x config.toml 2>&1 | tee aria.log
```

**RUST_LOG** overrides config. Examples: `aria_x=info`, `aria_x=debug`, `debug` (everything).

## 6. Run HiveClaw

```bash
# From bundle directory
./run.sh

# Or with env overrides
TELEGRAM_BOT_TOKEN=xxx RUST_LOG=debug ./aria-x config.toml
```

Expected output:

```
[HiveClaw] Loading config from: config.toml
[HiveClaw] Config loaded (node=orchestrator-1 role=orchestrator tier=orchestrator | LLM: openrouter/arcee-ai/trinity-large-preview:free)
[HiveClaw] LLM: OpenRouter (model=arcee-ai/trinity-large-preview:free)
[HiveClaw] ✅ Telegram gateway listening on http://0.0.0.0:8080/webhook
[HiveClaw] Set webhook: https://api.telegram.org/bot<TOKEN>/setWebhook?url=<YOUR_URL>/webhook
```

## 7. Test

1. Open Telegram and find your bot.
2. Send a message (e.g. "Hello").
3. The bot should reply using the Ollama model.

## 8. Troubleshooting

| Issue | Fix |
|-------|-----|
| `OpenRouter request failed` | Ensure `OPENROUTER_API_KEY` is set in `.env` or Vault. |
| `Ollama request failed` | (If using fallback) Ensure Ollama is running (`ollama serve`) and model is pulled. |
| `TELEGRAM_BOT_TOKEN not set` | Set env, add to `.env`, or use `telegram_token` / `telegram_token_file` |
| Webhook not receiving | Set webhook with `setWebhook`; ensure URL is HTTPS and publicly reachable |
| `policy_path` not found | Use paths relative to CWD when running from the bundle directory |
| Port 8080 in use | Change `telegram_port` in config |

## 9. CLI Mode (No Telegram)

For local testing without Telegram:

[gateway]
adapter = "cli"

[llm]
backend = "openrouter"
model = "arcee-ai/trinity-large-preview:free"
```

Run `./aria-x config.toml` and type prompts at the terminal.
