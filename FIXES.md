# v6 fixes and architecture changes

This bundle adds the requested local-mode deployment controls and shared UI architecture.

## Core behavior changes

- each tenant now advertises `deployment_mode=single`
- `active_deployment_count` is tracked as `0` or `1`
- tenants can **pause**, **resume**, or **delete** deployments from the shared customer UI
- admins can **reconcile**, **pause**, **resume**, or **delete** deployments from the admin UI
- lifecycle requests are accepted from provisioning, active, paused, suspended, and reconciling states
- if a pause/delete lands while provisioning is still finishing, the provisioner honors the latest `desired_state` after startup and immediately tears the deployment back down if required

## Shared UI routing

- the dedicated per-tenant Supabase Studio container is removed from each local tenant deployment
- Traefik now routes tenant hostnames in two layers:
  - root and non-API paths → the shared `web` container
  - API prefixes (`/auth`, `/rest`, `/graphql`, `/realtime`, `/storage`, `/functions`, `/pg`) → the tenant Kong gateway
- tenant root hosts redirect into the shared dashboard detail page for that tenant

## New control-plane lifecycle subjects

- `tenant.pause`
- `tenant.resume`
- `tenant.delete`

Legacy aliases still work:

- `tenant.deprovision` behaves like pause
- admin `deprovision` maps to pause

## Shared UI / tenant host routing cleanup

- Tenant subdomains no longer redirect to `localhost:3000`; they stay on `http://<tenant>.supabase.localhost/dashboard/tenants/<tenant>` through the shared Traefik-routed UI.
- Host port `3000` remains exclusive to the shared web UI. No tenant Studio container is published on host port `3000` in local shared-UI mode.

- default platform API bind changed to `0.0.0.0:8000` so the Dockerized shared UI can reach the host-run Rust API via `host.docker.internal:8000`
- create-tenant route now reports `platform_unreachable` instead of a generic failure when the UI cannot reach the platform API

## v10
- replaced the `web-migrate` shell-based startup path with a Node-based migrator (`apps/web/docker-migrate.mjs`) so Alpine `/bin/sh` parsing issues are removed from the migration flow
- fixed tenant `console_url` generation so it now points at the tenant Studio root (`http://<tenant>.supabase.localhost`) instead of a platform `/dashboard/...` path that Studio does not serve
- made Traefik tenant router labels more explicit (`entrypoints=web` and explicit router service binding) to reduce tenant-host 404s
- if you created tenants with earlier shared-UI bundles, delete and recreate or reconcile them after upgrading so the patched ingress labels and Studio URL are applied

- v11: reconcile now rehydrates older tenant folders by restoring the upstream Supabase compose, Studio, and Traefik labels before `docker compose up -d`, which fixes persistent 404s on existing tenant Studio URLs.

- v12: fixed Traefik Docker provider on macOS/desktop setups by detecting the real host Docker socket path and mounting it into the Traefik container via DOCKER_SOCKET_PATH. Without this, Traefik only showed internal routers and tenant Studio hosts returned 404.

- v13: switched Traefik local routing to the file provider so tenant Studio routes do not depend on Docker socket discovery.
