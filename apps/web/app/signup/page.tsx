import Link from "next/link";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { SignupForm } from "@/components/auth/signup-form";

export default function SignupPage({
  searchParams
}: {
  searchParams?: { plan?: string };
}) {
  const plan = searchParams?.plan;
  return (
    <div className="mx-auto max-w-md">
      <Card>
        <CardHeader>
          <CardTitle>Create account</CardTitle>
          <CardDescription>Sign up to create and manage Supabase projects.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <SignupForm defaultPlan={plan} />
          <p className="text-sm text-slate-600 dark:text-slate-300">
            Already have an account? <Link href="/login" className="underline">Sign in</Link>
          </p>
        </CardContent>
      </Card>
    </div>
  );
}
