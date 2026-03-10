#!/usr/bin/env bash
set -euo pipefail

: "${STRIPE_WEBHOOK_SECRET:?Set STRIPE_WEBHOOK_SECRET}"

echo "==> Forwarding Stripe webhooks to billing-service"
stripe listen --forward-to localhost:8080/stripe/webhook
