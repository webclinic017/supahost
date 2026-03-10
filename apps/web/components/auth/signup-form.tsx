"use client";

import { useState } from "react";
import Link from "next/link";
import { signIn } from "next-auth/react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";

export function SignupForm({ defaultPlan }: { defaultPlan?: string }) {
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    setLoading(true);
    setError(null);

    const res = await fetch("/api/auth/signup", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ email, password, name })
    });

    const json = await res.json().catch(() => ({}));

    if (!res.ok) {
      setLoading(false);
      if (json?.error === "email_in_use") {
        setError("That email is already registered. Try signing in.");
      } else {
        setError("Sign up failed. Please check your details and try again.");
      }
      return;
    }

    const resp = await signIn("credentials", { email, password, redirect: false });
    setLoading(false);

    if (!resp || resp.error) {
      setError("Account created, but sign-in failed. Please sign in.");
      return;
    }

    // Store plan hint for first project creation
    if (defaultPlan) {
      localStorage.setItem("supahost_default_plan", defaultPlan);
    }

    window.location.href = "/dashboard";
  }

  return (
    <form onSubmit={onSubmit} className="space-y-4">
      {error ? (
        <Alert variant="destructive">
          <AlertTitle>Sign up failed</AlertTitle>
          <AlertDescription>
            {error} <Link className="underline" href="/login">Sign in</Link>
          </AlertDescription>
        </Alert>
      ) : null}

      <div className="space-y-2">
        <Label htmlFor="name">Name (optional)</Label>
        <Input id="name" value={name} onChange={(e) => setName(e.target.value)} />
      </div>

      <div className="space-y-2">
        <Label htmlFor="email">Email</Label>
        <Input
          id="email"
          type="email"
          autoComplete="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          required
        />
      </div>

      <div className="space-y-2">
        <Label htmlFor="password">Password</Label>
        <Input
          id="password"
          type="password"
          autoComplete="new-password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          required
        />
        <p className="text-xs text-slate-500 dark:text-slate-400">Minimum 8 characters.</p>
      </div>

      <Button type="submit" className="w-full" disabled={loading}>
        {loading ? "Creating account…" : "Create account"}
      </Button>
    </form>
  );
}
