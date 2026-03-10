# Kubernetes deployment approach (recommended for production)

A practical multi-tenant strategy on Kubernetes:

- wasmCloud: run via the wasmCloud Operator + wadm + NATS
- billing-service: Deployment + Service (ClusterIP) + Ingress (for Stripe webhooks)
- provisioner: Deployment with RBAC that can create namespaces + apply tenant manifests
- tenant-per-namespace:
  - postgres StatefulSet (or managed Postgres per tenant)
  - supabase services (auth/rest/realtime/storage) as Deployments
  - ingress route: `*.supabase.example.com` → per-tenant Kong
  - resource quotas + network policies per namespace

## Why namespace-per-tenant?

- Simple mental model
- Good isolation primitives (NetworkPolicy, ResourceQuota, etc.)
- You can map billing plan → Kubernetes limits

## Confidential compute on Kubernetes

Consider “Confidential Containers” / Kata Containers, or confidential-node pools
(Azure SEV-SNP, GCP TDX, etc.) and schedule:
- secrets broker + wasmCloud control plane on confidential nodes
- data plane (Postgres) on confidential nodes if supported and meets your performance requirements
