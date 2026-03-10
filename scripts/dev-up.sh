#!/usr/bin/env bash
set -euo pipefail

NETWORK="${DOCKER_NETWORK:-supahost}"

if [ ! -f .env ]; then
  cp .env.example .env
  echo "==> Created .env from .env.example"
fi

mkdir -p infra/local/traefik/dynamic

cat > infra/local/traefik/dynamic/platform-ui.yaml <<'EOF'
http:
  routers:
    platform-ui:
      rule: Host(`localhost`)
      entryPoints:
        - web
      service: platform-ui
  services:
    platform-ui:
      loadBalancer:
        servers:
          - url: http://web:3000
EOF

echo "==> Ensuring docker network: ${NETWORK}"
docker network inspect "${NETWORK}" >/dev/null 2>&1 || docker network create "${NETWORK}"

echo "==> Starting NATS"
docker compose -f infra/local/nats/docker-compose.nats.yaml up -d

echo "==> Starting Redis"
docker compose -f infra/local/redis/docker-compose.redis.yaml up -d

echo "==> Starting Traefik (file provider)"
docker compose -f infra/local/traefik/docker-compose.traefik.yaml up -d

echo "==> Starting platform DB + web UI"
docker compose -f infra/local/platform/docker-compose.platform.yaml -f infra/local/web/docker-compose.web.yaml up -d

echo ""
echo "Next steps:"
echo "  1) source .env"
echo "  2) cargo run -p platform-api"
echo "  3) cargo run -p provisioner"
echo "  4) optional: cargo run -p billing-service   # when BILLING_MODE=stripe"
echo "  5) open http://localhost:3000"
echo "  6) tenant hostnames like http://<tenant-id>.supabase.localhost open that tenant's Supabase Studio"
