#!/usr/bin/env bash
# Check Telegram webhook status. Requires TELEGRAM_BOT_TOKEN or pass token as arg.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [[ -f "$PROJECT_ROOT/.env" ]]; then
  set -a
  # shellcheck disable=SC1091
  source "$PROJECT_ROOT/.env"
  set +a
fi

TOKEN="${1:-$TELEGRAM_BOT_TOKEN}"
if [[ -z "$TOKEN" ]]; then
  echo "Usage: $0 <bot_token>"
  echo "   or: TELEGRAM_BOT_TOKEN=xxx $0"
  exit 1
fi
echo "Checking webhook for bot..."
curl -s "https://api.telegram.org/bot${TOKEN}/getWebhookInfo" | jq . 2>/dev/null || curl -s "https://api.telegram.org/bot${TOKEN}/getWebhookInfo"
echo ""
echo "If url is empty: set webhook with:"
echo "  curl \"https://api.telegram.org/bot${TOKEN}/setWebhook?url=<YOUR_PUBLIC_HTTPS_URL>/webhook\""
echo "  (For local: ngrok http 8080, then use the https URL)"
