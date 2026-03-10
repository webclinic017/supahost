#!/bin/sh
set -eu

PRISMA="./node_modules/.bin/prisma"

if [ ! -x "$PRISMA" ]; then
  echo "prisma CLI not found at $PRISMA" >&2
  exit 1
fi

attempt=0
while true; do
  if "$PRISMA" migrate deploy; then
    break
  fi

  attempt=`expr "$attempt" + 1`
  if [ "$attempt" -ge 20 ]; then
    echo "migrate failed after $attempt attempts" >&2
    exit 1
  fi

  echo "database not ready yet; retrying in 2s; attempt $attempt of 20" >&2
  sleep 2
done

node prisma/seed.js || true
