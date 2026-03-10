# Architecture notes

This starter splits the platform into:

- wasmCloud control plane (`components/platform-api`)
  - Operator/API surface
  - Stores tenant records in Redis through `wasi:keyvalue`
  - Talks to billing-service via NATS request/reply (through `wasmcloud:messaging` provider)

- billing-service (`services/billing-service`)
  - The only thing that talks to Stripe
  - Emits internal events over NATS (`billing.subscription_active`, etc.)

- provisioner (`services/provisioner`)
  - Consumes billing events or direct provisioning commands
  - Provisions per-tenant Supabase stacks using the **official self-hosted docker-compose**
    vendored in `vendor/supabase-docker/`
  - Writes provisioning outputs (URL + keys) back to Redis

This separation keeps secrets and “slow” external integrations outside the wasm runtime,
while keeping core platform logic portable and easy to sandbox in WebAssembly.

## Tenant isolation models

This repo defaults to **project-per-tenant** (a dedicated Postgres + supabase services per tenant).

Production alternatives:
- Namespace-per-tenant on Kubernetes
- Firecracker microVM per tenant (stronger isolation; more ops overhead)
- Shared Postgres with RLS and schema-per-tenant (harder to get right)
