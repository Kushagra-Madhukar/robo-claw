#!/usr/bin/env bash
# Check Telegram webhook status. Requires TELEGRAM_BOT_TOKEN or pass token as arg.
set -e
TOKEN="${1:-$TELEGRAM_BOT_TOKEN}"
if [ -z "$TOKEN" ]; then
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
