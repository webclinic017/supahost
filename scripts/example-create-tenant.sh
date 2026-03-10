#!/usr/bin/env bash
set -euo pipefail

EMAIL="${1:-dev@example.com}"
PLAN="${2:-starter}"
API_URL="${PLATFORM_API_URL:-http://localhost:8000}"

if ! curl -fsS "${API_URL}/healthz" >/dev/null 2>&1; then
  echo "Platform API is not reachable at ${API_URL}." >&2
  echo "Start it first: source .env && cargo run -p platform-api" >&2
  exit 1
fi

if command -v python3 >/dev/null 2>&1; then
  PAYLOAD=$(EMAIL="$EMAIL" PLAN="$PLAN" python3 - <<'PYJSON'
import json, os
print(json.dumps({"email": os.environ["EMAIL"], "plan": os.environ["PLAN"]}))
PYJSON
)
elif command -v jq >/dev/null 2>&1; then
  PAYLOAD=$(jq -nc --arg email "$EMAIL" --arg plan "$PLAN" '{email:$email, plan:$plan}')
else
  PAYLOAD=$(printf '{"email":"%s","plan":"%s"}' "$EMAIL" "$PLAN")
fi

BODY_FILE=$(mktemp)
STATUS=$(curl -sS -o "$BODY_FILE" -w "%{http_code}"   -X POST "${API_URL}/v1/tenants"   -H 'content-type: application/json'   -d "$PAYLOAD")

if [ "$STATUS" -lt 200 ] || [ "$STATUS" -ge 300 ]; then
  echo "Tenant create failed with HTTP ${STATUS}" >&2
  cat "$BODY_FILE" >&2
  rm -f "$BODY_FILE"
  exit 1
fi

if command -v jq >/dev/null 2>&1; then
  jq . < "$BODY_FILE" || cat "$BODY_FILE"
else
  cat "$BODY_FILE"
fi

rm -f "$BODY_FILE"
