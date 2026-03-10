import Link from "next/link";
import { headers } from "next/headers";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";

function tenantFromHost(host: string | null) {
  if (!host) return null;
  const clean = host.split(":")[0].toLowerCase();
  const suffix = ".supabase.localhost";
  if (!clean.endsWith(suffix)) return null;
  const candidate = clean.slice(0, -suffix.length);
  return candidate || null;
}

export default function HomePage() {
  const host = headers().get("host");
  const tenant = tenantFromHost(host);
  if (tenant) {
    return (
      <div className="space-y-6">
        <Card>
          <CardHeader>
            <CardTitle>Tenant hostname detected</CardTitle>
            <CardDescription>
              This shared web console should normally be reached on <code>localhost:3000</code>. Tenant hostnames are expected to resolve to Supabase Studio through Traefik and Kong.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4 text-sm text-slate-600 dark:text-slate-300">
            <div className="font-mono break-all">http://{tenant}.supabase.localhost/</div>
            <p>If you are seeing this page on a tenant host, rebuild the local stack and recreate or reconcile the tenant so Traefik picks up the latest labels.</p>
            <div className="flex gap-3">
              <a href={`http://${tenant}.supabase.localhost/`}>
                <Button>Open tenant Studio root</Button>
              </a>
              <Link href="/dashboard">
                <Button variant="outline">Open platform dashboard</Button>
              </Link>
            </div>
          </CardContent>
        </Card>
      </div>
    );
  }

  return (
    <div className="space-y-10">
      <section className="space-y-4">
        <h1 className="text-3xl font-bold tracking-tight">
          Host real Supabase projects — multi-tenant, automated, billable.
        </h1>
        <p className="text-slate-600 dark:text-slate-300 max-w-2xl">
          SupaHost provisions a full self-hosted Supabase stack per tenant while keeping one shared web console container for every customer. Tenant subdomains open each tenant's Supabase Studio on their own hostname, while localhost:3000 stays reserved for the shared platform console.
        </p>
        <div className="flex gap-3">
          <Link href="/signup">
            <Button size="lg">Get started</Button>
          </Link>
          <Link href="/dashboard">
            <Button size="lg" variant="outline">
              Go to dashboard
            </Button>
          </Link>
        </div>
      </section>

      <section className="grid md:grid-cols-3 gap-6">
        <Card>
          <CardHeader>
            <CardTitle>Actual Supabase</CardTitle>
            <CardDescription>Not a mock — provisions the upstream self-host data plane.</CardDescription>
          </CardHeader>
          <CardContent className="text-sm text-slate-600 dark:text-slate-300">
            Each tenant gets its own Supabase services and gateway, and its Studio stays behind tenant-local routing with no host-port collision.
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Single shared UI</CardTitle>
            <CardDescription>One Next.js container serves the shared platform console.</CardDescription>
          </CardHeader>
          <CardContent className="text-sm text-slate-600 dark:text-slate-300">
            The shared control console is multi-tenant aware, while tenant subdomains route through Traefik to the right tenant Kong and Studio.
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Lifecycle-safe</CardTitle>
            <CardDescription>Pause or delete from provisioning, active, or suspended states.</CardDescription>
          </CardHeader>
          <CardContent className="text-sm text-slate-600 dark:text-slate-300">
            Local mode enforces exactly one active deployment slot per tenant, making hacks and demos cheaper and easier to reason about.
          </CardContent>
        </Card>
      </section>
    </div>
  );
}
