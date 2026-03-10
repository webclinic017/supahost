import Link from "next/link";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";

export default function BillingSuccessPage({
  searchParams
}: {
  searchParams?: { session_id?: string };
}) {
  return (
    <div className="mx-auto max-w-xl space-y-6">
      <Card>
        <CardHeader>
          <CardTitle>Payment received</CardTitle>
          <CardDescription>
            Stripe returned successfully. Provisioning continues in the background via webhook.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <p className="text-sm text-slate-600 dark:text-slate-300">
            Checkout session: <span className="font-mono text-xs">{searchParams?.session_id ?? "(unknown)"}</span>
          </p>
          <p className="text-sm text-slate-600 dark:text-slate-300">
            Go back to your dashboard and open your project once it becomes <b>active</b>.
          </p>
          <Link href="/dashboard">
            <Button>Back to dashboard</Button>
          </Link>
        </CardContent>
      </Card>
    </div>
  );
}
