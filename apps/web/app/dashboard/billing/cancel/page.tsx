import Link from "next/link";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";

export default function BillingCancelPage() {
  return (
    <div className="mx-auto max-w-xl space-y-6">
      <Card>
        <CardHeader>
          <CardTitle>Checkout canceled</CardTitle>
          <CardDescription>No charges were made.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <p className="text-sm text-slate-600 dark:text-slate-300">
            You can try again from your dashboard.
          </p>
          <Link href="/dashboard">
            <Button>Back to dashboard</Button>
          </Link>
        </CardContent>
      </Card>
    </div>
  );
}
