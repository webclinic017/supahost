#!/bin/sh
set -e

# By default, the web container does NOT run migrations.
# Use the dedicated "web-migrate" service (target: migrator) in docker-compose for reliable startup ordering.
if [ "${RUN_DB_MIGRATIONS:-0}" = "1" ] && [ -n "${DATABASE_URL:-}" ]; then
  if [ -x "./node_modules/.bin/prisma" ]; then
    echo "==> prisma migrate deploy (with retries)"
    attempt=0
    until ./node_modules/.bin/prisma migrate deploy; do
      attempt=$((attempt + 1))
      if [ "$attempt" -ge 20 ]; then
        echo "prisma migrate deploy failed after ${attempt} attempts; continuing" >&2
        break
      fi
      echo "database not ready yet; retrying in 2s (${attempt}/20)" >&2
      sleep 2
    done

    echo "==> prisma seed"
    node prisma/seed.js || true
  else
    echo "==> prisma CLI not present in this image; skipping migrations/seed"
  fi
fi

exec "$@"
