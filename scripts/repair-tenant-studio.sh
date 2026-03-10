#!/usr/bin/env bash
set -euo pipefail

TENANT_ID="${1:-}"
if [[ -z "$TENANT_ID" ]]; then
  echo "usage: $0 <tenant-id>" >&2
  exit 1
fi

ADMIN_TOKEN="${PLATFORM_ADMIN_TOKEN:-dev-admin-token-change-me}"
API_URL="${PLATFORM_API_URL:-http://localhost:8000}"

echo "==> Requesting admin reconcile for tenant: ${TENANT_ID}"
resp=$(curl -sS -X POST   -H "Authorization: Bearer ${ADMIN_TOKEN}"   "${API_URL}/v1/admin/tenants/${TENANT_ID}/reconcile")

echo "$resp" | python -m json.tool 2>/dev/null || echo "$resp"
