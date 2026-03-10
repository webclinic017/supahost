# syntax=docker/dockerfile:1

FROM node:20-alpine AS deps
RUN apk add --no-cache libc6-compat openssl
WORKDIR /repo/apps/web

# Copy manifests first for better caching
COPY apps/web/package.json apps/web/package-lock.json* apps/web/pnpm-lock.yaml* apps/web/yarn.lock* ./

# Install deps (use npm by default; works with package-lock, but also works without lockfile)
# If you prefer pnpm/yarn, adjust here and commit the lockfile.
RUN npm install

FROM node:20-alpine AS builder
RUN apk add --no-cache libc6-compat openssl
WORKDIR /repo/apps/web

# Reuse node_modules from deps stage
COPY --from=deps /repo/apps/web/node_modules ./node_modules
COPY --from=deps /repo/apps/web/package.json ./package.json
COPY apps/web/ ./

# Prisma client + Next build (Next is configured for output: 'standalone')
RUN npx prisma generate
ENV NEXT_TELEMETRY_DISABLED=1
RUN npm run build

# A lightweight migrator image for DB migrations + seed (runs once, exits)
FROM builder AS migrator
WORKDIR /repo/apps/web
ENV NEXT_TELEMETRY_DISABLED=1
CMD ["node", "./docker-migrate.mjs"]

FROM node:20-alpine AS runner
RUN apk add --no-cache libc6-compat openssl
WORKDIR /app

ENV NODE_ENV=production
ENV PORT=3000
ENV NEXT_TELEMETRY_DISABLED=1

# Copy the minimal standalone output (reduces file count and avoids copying full repo)
COPY --from=builder /repo/apps/web/.next/standalone ./
COPY --from=builder /repo/apps/web/.next/static ./.next/static
COPY --from=builder /repo/apps/web/public ./public
COPY --from=builder /repo/apps/web/prisma ./prisma

COPY apps/web/docker-entrypoint.sh /app/docker-entrypoint.sh
RUN chmod +x /app/docker-entrypoint.sh

EXPOSE 3000
ENTRYPOINT ["/app/docker-entrypoint.sh"]
CMD ["node", "server.js"]
