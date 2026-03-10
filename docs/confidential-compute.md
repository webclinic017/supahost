# Confidential compute notes (Nitro Enclaves, AMD SEV-SNP, Intel TDX)

This starter is designed so you can move sensitive parts into a TEE without rewriting the whole platform.

## What to put inside a TEE

Recommended to run inside your TEE boundary:

- tenant secret generation (JWT secrets, DB passwords, API keys)
- signing/encryption operations
- policy decisions (e.g., “allow provisioning only if subscription is active”)

Less ideal to run inside a TEE (higher operational friction):

- large multi-process Supabase stacks (Postgres, Realtime, etc.)
- anything that needs direct inbound networking (especially in Nitro Enclaves)

## AWS Nitro Enclaves

Nitro Enclaves have strict constraints (no direct network access, use vsock to parent instance, etc.).
A common approach is:

1) Run the main platform + Supabase stacks on the parent EC2 instance.
2) Run a small “secrets broker” in the enclave.
3) The parent sends vsock requests (e.g., “generate tenant secrets”).
4) The enclave returns encrypted material or performs signing on-demand.
5) Optionally integrate AWS KMS via the Nitro Enclaves KMS proxy on the parent.

## AMD SEV-SNP (Confidential EC2) and Intel TDX (Confidential VMs)

SEV-SNP and TDX are typically “confidential VM” models: you can run your whole stack inside
the protected VM with standard networking.

Practical approach:
- Run Kubernetes (or plain systemd) inside confidential VMs
- Deploy wasmCloud + billing + provisioner + tenant stacks normally
- Add attestation-based secret release (e.g., Vault/Key Broker only releases secrets to attested VMs)

## Important compatibility note

On AWS, SEV-SNP and Nitro Enclaves are not always compatible on the same instance configuration.
Design for a “choose one” posture per environment.
