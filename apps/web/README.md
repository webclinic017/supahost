# SupaHost Web Console (Next.js + TypeScript)

This is the **customer + admin** web console for the SupaHost control plane.

## Features

- TypeScript + Next.js App Router
- shadcn-style component primitives (Tailwind)
- Email/password signup + login (Credentials provider)
- Customer dashboard:
  - Create Supabase projects (tenants)
  - Redirect to Stripe Checkout if the control plane is configured with `BILLING_MODE=stripe`
  - View tenant status and keys
- Admin console:
  - List tenants
  - Reconcile (restart) tenant
  - Deprovision (suspend) tenant

## Local dev (run on host)

1) Start infrastructure:

```bash
./scripts/dev-up.sh
```

2) Configure DB + auth env:

```bash
cp apps/web/.env.example apps/web/.env
```

3) Initialize Prisma (first time only):

```bash
cd apps/web
npm install
npm run prisma:generate
npm run prisma:deploy   # uses included migrations
npm run prisma:seed     # creates ADMIN user if env vars set
```

4) Run Next.js:

```bash
npm run dev
```

Open: http://localhost:3000

## Running in Docker

`./scripts/dev-up.sh` starts the web console in Docker by default (see `infra/local/web/docker-compose.web.yaml`).

