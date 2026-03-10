export const dynamic = "force-dynamic";

import Link from "next/link";
import { redirect } from "next/navigation";
import { getServerSession } from "next-auth";
import { authOptions } from "@/lib/auth";
import { prisma } from "@/lib/db";
import { platformGetTenant } from "@/lib/platform";
import { StatusBadge } from "@/components/tenants/status-badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";

export default async function AdminPage() {
  const session = await getServerSession(authOptions);
  const role = (session?.user as any)?.role;
  if (role !== "ADMIN") redirect("/login");

  const tenants = await prisma.tenant.findMany({
    orderBy: { createdAt: "desc" },
    include: { owner: true }
  });

  const enriched = await Promise.all(
    tenants.map(async (t) => {
      try {
        const rec = await platformGetTenant(t.id);
        return {
          ...t,
          status: rec?.status ?? t.status,
          desiredState: rec?.desired_state ?? t.desiredState,
          activeDeploymentCount: rec?.active_deployment_count ?? t.activeDeploymentCount,
          apiUrl: rec?.api_url ?? t.apiUrl,
          gatewayUrl: rec?.gateway_url ?? t.gatewayUrl ?? rec?.api_url ?? t.apiUrl
        };
      } catch {
        return t;
      }
    })
  );

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold">Admin</h1>
        <p className="text-slate-600 dark:text-slate-300">
          Platform operators manage one active local deployment slot per tenant and one shared UI deployment for the whole platform.
        </p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Tenants</CardTitle>
          <CardDescription>Backed by the platform database; status is refreshed from the control plane.</CardDescription>
        </CardHeader>
        <CardContent>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Tenant</TableHead>
                <TableHead>Owner</TableHead>
                <TableHead>Plan</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Desired</TableHead>
                <TableHead>Slot</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {enriched.map((t) => (
                <TableRow key={t.id}>
                  <TableCell>
                    <Link className="underline" href={`/admin/tenants/${t.id}`}>
                      {t.id}
                    </Link>
                  </TableCell>
                  <TableCell>{t.owner.email}</TableCell>
                  <TableCell className="capitalize">{t.plan}</TableCell>
                  <TableCell>
                    <StatusBadge status={t.status} />
                  </TableCell>
                  <TableCell className="capitalize">{t.desiredState}</TableCell>
                  <TableCell>{t.activeDeploymentCount}/1</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </div>
  );
}
