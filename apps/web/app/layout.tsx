export const dynamic = "force-dynamic";

import "./globals.css";
import Link from "next/link";
import { AuthSessionProvider } from "@/components/session-provider";
import { getServerSession } from "next-auth";
import { authOptions } from "@/lib/auth";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { SignOutButton } from "@/components/signout-button";

export const metadata = {
  title: "SupaHost",
  description: "Multi-tenant Supabase hosting platform"
};

export default async function RootLayout({
  children
}: {
  children: React.ReactNode;
}) {
  const session = await getServerSession(authOptions);
  const role = (session?.user as any)?.role;

  return (
    <html lang="en" suppressHydrationWarning>
      <body>
        <AuthSessionProvider>
          <header className="border-b border-slate-200 dark:border-slate-800">
            <div className="container flex h-14 items-center justify-between">
              <div className="flex items-center gap-6">
                <Link href="/" className="font-semibold">
                  SupaHost
                </Link>
                <nav className="hidden md:flex items-center gap-4 text-sm text-slate-600 dark:text-slate-300">
                  <Link href="/pricing" className="hover:underline">
                    Pricing
                  </Link>
                  {session?.user ? (
                    <>
                      <Link href="/dashboard" className="hover:underline">
                        Dashboard
                      </Link>
                      {role === "ADMIN" ? (
                        <Link href="/admin" className="hover:underline">
                          Admin
                        </Link>
                      ) : null}
                    </>
                  ) : null}
                </nav>
              </div>

              <div className="flex items-center gap-2">
                {session?.user ? (
                  <>
                    <span className="hidden sm:inline text-sm text-slate-600 dark:text-slate-300">
                      {session.user.email}
                    </span>
                    <SignOutButton />
                  </>
                ) : (
                  <>
                    <Link href="/login">
                      <Button variant="outline">Sign in</Button>
                    </Link>
                    <Link href="/signup">
                      <Button>Sign up</Button>
                    </Link>
                  </>
                )}
              </div>
            </div>
          </header>
          <Separator />
          <main className="container py-8">{children}</main>
          <footer className="border-t border-slate-200 dark:border-slate-800">
            <div className="container py-6 text-sm text-slate-500 dark:text-slate-400">
              SupaHost • shared multi-tenant UI • Rust control plane • Supabase tenant gateways
            </div>
          </footer>
        </AuthSessionProvider>
      </body>
    </html>
  );
}
