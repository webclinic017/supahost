import Link from "next/link";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { LoginForm } from "@/components/auth/login-form";

export default function LoginPage() {
  return (
    <div className="mx-auto max-w-md">
      <Card>
        <CardHeader>
          <CardTitle>Sign in</CardTitle>
          <CardDescription>Access your SupaHost dashboard.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <LoginForm redirectTo="/dashboard" />
          <p className="text-sm text-slate-600 dark:text-slate-300">
            No account? <Link href="/signup" className="underline">Sign up</Link>
          </p>
          <p className="text-xs text-slate-500 dark:text-slate-400">
            Platform admins can use <Link href="/admin" className="underline">Admin</Link> (requires ADMIN role).
          </p>
        </CardContent>
      </Card>
    </div>
  );
}
