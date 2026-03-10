export const dynamic = "force-dynamic";

import Link from "next/link";
import { redirect } from "next/navigation";
import { getServerSession } from "next-auth";
import { authOptions } from "@/lib/auth";
import { prisma } from "@/lib/db";
import { platformGetTenant } from "@/lib/platform";
import { CreateTenantForm } from "@/components/tenants/create-tenant-form";
import { StatusBadge } from "@/components/tenants/status-badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";

export default async function DashboardPage() {
  const session = await getServerSession(authOptions);
  const userId = session?.user && (session.user as any).id;
  if (!userId) {
    redirect("/login");
  }

  const tenants = await prisma.tenant.findMany({
    where: { ownerId: userId },
    orderBy: { createdAt: "desc" }
  });

  const enriched = await Promise.all(
    tenants.map(async (t) => {
      try {
        const rec = await platformGetTenant(t.id);
        return {
          ...t,
          status: rec?.status ?? t.status,
          desiredState: rec?.desired_state ?? t.desiredState,
          deploymentMode: rec?.deployment_mode ?? t.deploymentMode,
          activeDeploymentCount: rec?.active_deployment_count ?? t.activeDeploymentCount,
          apiUrl: rec?.api_url ?? t.apiUrl,
          gatewayUrl: rec?.gateway_url ?? t.gatewayUrl ?? rec?.api_url ?? t.apiUrl,
          consoleUrl: rec?.console_url ?? t.consoleUrl ?? rec?.gateway_url ?? t.gatewayUrl ?? rec?.api_url ?? t.apiUrl ?? `http://${t.id}.supabase.localhost`
        };
      } catch {
        return t;
      }
    })
  );

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold">Dashboard</h1>
        <p className="text-slate-600 dark:text-slate-300">
          One shared platform web console manages every tenant. Each project still gets its own Supabase Studio and one active deployment slot locally.
        </p>
      </div>

      <CreateTenantForm />

      <Card>
        <CardHeader>
          <CardTitle>Your projects</CardTitle>
          <CardDescription>
            Shared control-plane UI + per-tenant Supabase data plane. Root tenant hostnames resolve to each tenant's Supabase Studio through Kong.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {enriched.length === 0 ? (
            <p className="text-sm text-slate-600 dark:text-slate-300">
              No projects yet. Create one above.
            </p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Tenant</TableHead>
                  <TableHead>Plan</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Desired</TableHead>
                  <TableHead>Slot</TableHead>
                  <TableHead>Entry</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {enriched.map((t) => (
                  <TableRow key={t.id}>
                    <TableCell>
                      <Link className="underline" href={`/dashboard/tenants/${t.id}`}>
                        {t.id}
                      </Link>
                    </TableCell>
                    <TableCell className="capitalize">{t.plan}</TableCell>
                    <TableCell>
                      <StatusBadge status={t.status} />
                    </TableCell>
                    <TableCell className="capitalize">{t.desiredState}</TableCell>
                    <TableCell>{t.activeDeploymentCount}/1</TableCell>
                    <TableCell>
                      <a className="underline" href={t.gatewayUrl ?? t.apiUrl ?? `/dashboard/tenants/${t.id}`} target="_blank" rel="noreferrer">
                        Open tenant host
                      </a>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
