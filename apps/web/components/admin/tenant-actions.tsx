"use client";

import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";

export function TenantActions({ tenantId }: { tenantId: string }) {
  const [loading, setLoading] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [ok, setOk] = useState<string | null>(null);

  async function run(action: "reconcile" | "pause" | "resume" | "delete") {
    setLoading(action);
    setError(null);
    setOk(null);

    const res = await fetch(`/api/admin/tenants/${tenantId}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ action })
    });

    const json = await res.json().catch(() => ({}));
    setLoading(null);

    if (!res.ok) {
      setError(json?.error || "Action failed");
      return;
    }

    setOk(`Action '${action}' accepted (${json?.status ?? "ok"}).`);
    setTimeout(() => window.location.reload(), 700);
  }

  return (
    <div className="space-y-3">
      {error ? (
        <Alert variant="destructive">
          <AlertTitle>Action failed</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      ) : null}
      {ok ? (
        <Alert variant="success">
          <AlertTitle>Queued</AlertTitle>
          <AlertDescription>{ok}</AlertDescription>
        </Alert>
      ) : null}

      <div className="flex flex-wrap gap-2">
        <Button variant="outline" onClick={() => run("reconcile")} disabled={!!loading}>
          {loading === "reconcile" ? "Reconciling…" : "Reconcile"}
        </Button>
        <Button variant="outline" onClick={() => run("pause")} disabled={!!loading}>
          {loading === "pause" ? "Pausing…" : "Pause"}
        </Button>
        <Button onClick={() => run("resume")} disabled={!!loading}>
          {loading === "resume" ? "Resuming…" : "Resume"}
        </Button>
        <Button variant="destructive" onClick={() => run("delete")} disabled={!!loading}>
          {loading === "delete" ? "Deleting…" : "Delete"}
        </Button>
      </div>

      <p className="text-xs text-slate-500 dark:text-slate-400">
        Operators can force the single 0/1 deployment slot into reconcile, pause, resume, or delete from any lifecycle state.
      </p>
    </div>
  );
}
