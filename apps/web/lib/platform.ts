export type PlatformCreateTenantResponse =
  | {
      tenant_id: string;
      status: string;
      desired_state?: string;
      deployment_mode?: string;
      active_deployment_count?: number;
      api_url?: string;
      gateway_url?: string;
      console_url?: string;
      checkout_url?: string;
    }
  | { error: string; message?: string };

export type PlatformTenantRecord = {
  tenant_id: string;
  email: string;
  plan: string;
  status: string;
  desired_state: string;
  deployment_mode: string;
  active_deployment_count: number;
  api_url: string;
  gateway_url?: string | null;
  console_url?: string | null;
  anon_key?: string | null;
  service_key?: string | null;
  dashboard_username?: string | null;
  dashboard_password?: string | null;
};

function baseUrl() {
  return process.env.PLATFORM_API_URL || "http://localhost:8000";
}

export async function platformCreateTenant(input: {
  email: string;
  plan: string;
}): Promise<PlatformCreateTenantResponse> {
  try {
    const res = await fetch(`${baseUrl()}/v1/tenants`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(input),
      cache: "no-store"
    });

    const text = await res.text();
    try {
      return JSON.parse(text);
    } catch {
      return { error: "bad_response", message: text };
    }
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    return { error: "platform_unreachable", message };
  }
}

export async function platformGetTenant(tenantId: string): Promise<PlatformTenantRecord | null> {
  const res = await fetch(`${baseUrl()}/v1/tenants/${tenantId}`, {
    method: "GET",
    cache: "no-store"
  });
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`platform get tenant failed: ${res.status}`);
  return (await res.json()) as PlatformTenantRecord;
}

export async function platformTenantAction(params: {
  tenantId: string;
  action: "pause" | "resume" | "delete";
}): Promise<{ ok: boolean; status?: string; desired_state?: string; error?: string }> {
  const url =
    params.action === "delete"
      ? `${baseUrl()}/v1/tenants/${params.tenantId}`
      : `${baseUrl()}/v1/tenants/${params.tenantId}/${params.action}`;

  const res = await fetch(url, {
    method: params.action === "delete" ? "DELETE" : "POST",
    cache: "no-store"
  });

  const json = await res.json().catch(() => ({}));
  if (!res.ok) {
    return { ok: false, error: json?.error || `HTTP ${res.status}` };
  }
  return {
    ok: true,
    status: json?.status,
    desired_state: json?.desired_state
  };
}

export async function platformAdminAction(params: {
  tenantId: string;
  action: "pause" | "resume" | "delete" | "reconcile";
}): Promise<{ ok: boolean; status?: string; desired_state?: string; error?: string }> {
  const token = process.env.PLATFORM_ADMIN_TOKEN;
  if (!token) {
    return { ok: false, error: "PLATFORM_ADMIN_TOKEN not set" };
  }

  let url: string;
  let method: "POST" | "DELETE" = "POST";
  if (params.action === "delete") {
    url = `${baseUrl()}/v1/admin/tenants/${params.tenantId}`;
    method = "DELETE";
  } else {
    url = `${baseUrl()}/v1/admin/tenants/${params.tenantId}/${params.action}`;
  }

  const res = await fetch(url, {
    method,
    headers: {
      authorization: `Bearer ${token}`
    },
    cache: "no-store"
  });

  const json = await res.json().catch(() => ({}));
  if (!res.ok) {
    return { ok: false, error: json?.error || `HTTP ${res.status}` };
  }
  return {
    ok: true,
    status: json?.status,
    desired_state: json?.desired_state
  };
}
