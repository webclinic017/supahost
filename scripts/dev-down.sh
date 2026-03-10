#!/usr/bin/env bash
set -euo pipefail

echo "==> Stopping web UI + platform DB"
docker compose -f infra/local/platform/docker-compose.platform.yaml -f infra/local/web/docker-compose.web.yaml down --remove-orphans || true

echo "==> Stopping Traefik + Redis + NATS"
docker compose -f infra/local/traefik/docker-compose.traefik.yaml down --remove-orphans || true
docker compose -f infra/local/redis/docker-compose.redis.yaml down --remove-orphans || true
docker compose -f infra/local/nats/docker-compose.nats.yaml down --remove-orphans || true

echo "==> Done"
