use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use wasmcloud_component::http;

// WASI preview2 config + keyvalue
use wasmcloud_component::wasi::config;
use wasmcloud_component::wasi::keyvalue;

// wasmCloud messaging
use wasmcloud_component::wasmcloud::messaging;

#[derive(Debug, Deserialize)]
struct CreateTenantRequest {
    email: String,
    plan: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct TenantRecord {
    tenant_id: String,
    email: String,
    plan: String,
    status: String,

    // Stripe-ish fields
    stripe_checkout_session_id: Option<String>,

    // Provisioned outputs (written by provisioner)
    api_url: Option<String>,
    anon_key: Option<String>,
    service_key: Option<String>,

    // Studio basic-auth (written by provisioner; treat as sensitive)
    dashboard_username: Option<String>,
    dashboard_password: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CheckoutRequest {
    tenant_id: String,
    email: String,
    plan: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CheckoutResponse {
    checkout_url: String,
    session_id: String,
}

// ---------- Helpers ----------

fn cfg(key: &str) -> Option<String> {
    // `wasi:config/store.get` returns Option<String>
    config::store::get(key).ok().flatten()
}

fn tenant_key_prefix() -> String {
    cfg("TENANT_KEY_PREFIX").unwrap_or_else(|| "tenant:".to_string())
}

fn tenant_key(tenant_id: &str) -> String {
    format!("{}{}", tenant_key_prefix(), tenant_id)
}

fn kv_bucket() -> Result<keyvalue::store::Bucket> {
    // Bucket name is provider-dependent. For Redis provider, this is a logical namespace.
    // "platform" is fine as a default.
    keyvalue::store::open("platform").map_err(|e| anyhow!("open keyvalue bucket: {e:?}"))
}

fn kv_get_tenant(tenant_id: &str) -> Result<Option<TenantRecord>> {
    let bucket = kv_bucket()?;
    let key = tenant_key(tenant_id);
    let bytes = keyvalue::store::get(&bucket, &key)
        .map_err(|e| anyhow!("kv get failed: {e:?}"))?;
    match bytes {
        None => Ok(None),
        Some(b) => Ok(Some(serde_json::from_slice(&b)?)),
    }
}

fn kv_put_tenant(rec: &TenantRecord) -> Result<()> {
    let bucket = kv_bucket()?;
    let key = tenant_key(&rec.tenant_id);
    let bytes = serde_json::to_vec(rec)?;
    keyvalue::store::set(&bucket, &key, &bytes)
        .map_err(|e| anyhow!("kv set failed: {e:?}"))?;
    Ok(())
}

fn json_response(status: u16, value: serde_json::Value) -> http::Response<impl http::OutgoingBody> {
    let body = value.to_string();
    let mut resp = http::Response::new(body);
    // Best effort: these methods exist in wasmcloud_component::http::Response
    resp.set_status(status);
    resp.headers_mut()
        .insert("content-type", "application/json");
    resp
}

fn text_response(status: u16, body: &str) -> http::Response<impl http::OutgoingBody> {
    let mut resp = http::Response::new(body.to_string());
    resp.set_status(status);
    resp.headers_mut().insert("content-type", "text/plain");
    resp
}

fn read_body(req: &http::IncomingRequest) -> Result<Vec<u8>> {
    // IncomingRequest is a wrapper around WASI http IncomingRequest.
    // wasmcloud_component exposes a blocking body reader via `http::IncomingBody`.
    let mut body = Vec::new();
    if let Some(mut incoming) = req.body() {
        incoming
            .read_to_end(&mut body)
            .map_err(|e| anyhow!("read body: {e:?}"))?;
    }
    Ok(body)
}

fn public_base_domain() -> String {
    cfg("PUBLIC_TENANT_BASE_DOMAIN").unwrap_or_else(|| "supabase.localhost".to_string())
}

fn billing_subject() -> String {
    cfg("BILLING_REQUEST_SUBJECT").unwrap_or_else(|| "billing.create_checkout".to_string())
}

fn billing_mode() -> String {
    // "stripe" or "direct"
    cfg("BILLING_MODE").unwrap_or_else(|| "direct".to_string())
}

fn provision_subject() -> String {
    cfg("PROVISION_REQUEST_SUBJECT").unwrap_or_else(|| "tenant.provision".to_string())
}

fn deprovision_subject() -> String {
    cfg("DEPROVISION_REQUEST_SUBJECT").unwrap_or_else(|| "tenant.deprovision".to_string())
}

fn reconcile_subject() -> String {
    cfg("RECONCILE_REQUEST_SUBJECT").unwrap_or_else(|| "tenant.reconcile".to_string())
}

fn admin_token() -> Option<String> {
    cfg("PLATFORM_ADMIN_TOKEN")
}

fn call_billing_create_checkout(payload: &CheckoutRequest) -> Result<CheckoutResponse> {
    let subject = billing_subject();
    let bytes = serde_json::to_vec(payload)?;
    let msg = messaging::types::BrokerMessage {
        body: bytes,
        reply_to: None,
        subject: None,
    };

    // 5s request timeout
    let resp = messaging::consumer::request(&subject, &msg, Some(5000))
        .map_err(|e| anyhow!("billing request failed: {e:?}"))?;

    serde_json::from_slice(&resp.body).context("parse billing response")
}

fn publish_provision_request(tenant_id: &str, email: &str, plan: &str) -> Result<()> {
    let subject = provision_subject();
    let body = serde_json::to_vec(&serde_json::json!({
        "tenant_id": tenant_id,
        "email": email,
        "plan": plan,
    }))?;

    let msg = messaging::types::BrokerMessage {
        body,
        reply_to: None,
        subject: None,
    };

    messaging::producer::publish(&subject, &msg)
        .map_err(|e| anyhow!("publish provision request failed: {e:?}"))?;
    Ok(())
}

fn publish_deprovision_request(tenant_id: &str) -> Result<()> {
    let subject = deprovision_subject();
    let body = serde_json::to_vec(&serde_json::json!({
        "tenant_id": tenant_id,
    }))?;

    let msg = messaging::types::BrokerMessage {
        body,
        reply_to: None,
        subject: None,
    };

    messaging::producer::publish(&subject, &msg)
        .map_err(|e| anyhow!("publish deprovision request failed: {e:?}"))?;
    Ok(())
}

fn publish_reconcile_request(tenant_id: &str) -> Result<()> {
    let subject = reconcile_subject();
    let body = serde_json::to_vec(&serde_json::json!({
        "tenant_id": tenant_id,
    }))?;

    let msg = messaging::types::BrokerMessage {
        body,
        reply_to: None,
        subject: None,
    };

    messaging::producer::publish(&subject, &msg)
        .map_err(|e| anyhow!("publish reconcile request failed: {e:?}"))?;
    Ok(())
}

fn require_admin(req: &http::IncomingRequest) -> Result<()> {
    let Some(expected) = admin_token() else {
        // If not configured, allow (dev mode).
        return Ok(());
    };

    // Header lookup is case-insensitive, but the wrapper exposes a simple map.
    let auth = req
        .headers()
        .get("authorization")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "".to_string());

    let want = format!("Bearer {}", expected);
    if auth != want {
        return Err(anyhow!("unauthorized"));
    }
    Ok(())
}

// ---------- HTTP Server ----------

struct Component;

impl http::Server for Component {
    fn handle(req: http::IncomingRequest) -> http::Result<http::Response<impl http::OutgoingBody>> {
        match handle_impl(req) {
            Ok(resp) => Ok(resp),
            Err(err) => {
                // Avoid leaking secrets; return generic error
                Ok(json_response(
                    500,
                    serde_json::json!({"error":"internal_error","message": err.to_string()}),
                ))
            }
        }
    }
}

fn handle_impl(req: http::IncomingRequest) -> Result<http::Response<impl http::OutgoingBody>> {
    let method = req.method().to_string();
    let path = req.path().unwrap_or_else(|| "/".to_string());

    // ---- health ----
    if method == "GET" && path == "/healthz" {
        return Ok(text_response(200, "ok"));
    }

    // ---- create tenant ----
    if method == "POST" && path == "/v1/tenants" {
        let body = read_body(&req)?;
        let create: CreateTenantRequest = serde_json::from_slice(&body)
            .context("invalid JSON body (expected {email, plan})")?;

        let tenant_id = Uuid::new_v4().to_string();

        let mut rec = TenantRecord {
            tenant_id: tenant_id.clone(),
            email: create.email.clone(),
            plan: create.plan.clone(),
            status: "provisioning".to_string(),
            stripe_checkout_session_id: None,
            api_url: None,
            anon_key: None,
            service_key: None,
            dashboard_username: None,
            dashboard_password: None,
        };

        kv_put_tenant(&rec)?;

        if billing_mode() == "stripe" {
            rec.status = "pending_checkout".to_string();
            kv_put_tenant(&rec)?;

            // Ask billing-service to create a Stripe checkout session
            let checkout_req = CheckoutRequest {
                tenant_id: tenant_id.clone(),
                email: create.email,
                plan: create.plan,
            };

            let checkout = call_billing_create_checkout(&checkout_req)?;

            rec.status = "checkout_created".to_string();
            rec.stripe_checkout_session_id = Some(checkout.session_id.clone());
            kv_put_tenant(&rec)?;

            return Ok(json_response(
                200,
                serde_json::json!({
                  "tenant_id": tenant_id,
                  "checkout_url": checkout.checkout_url,
                  "status": rec.status,
                }),
            ));
        } else {
            // Direct provisioning (no Stripe)
            publish_provision_request(&tenant_id, &create.email, &create.plan)?;

            rec.status = "provisioning".to_string();
            kv_put_tenant(&rec)?;

            return Ok(json_response(
                200,
                serde_json::json!({
                  "tenant_id": tenant_id,
                  "status": rec.status,
                  "api_url": format!("http://{}.{}", tenant_id, public_base_domain()),
                }),
            ));
        }
    }

    // ---- get tenant ----
    if method == "GET" && path.starts_with("/v1/tenants/") {
        let tenant_id = path.trim_start_matches("/v1/tenants/").to_string();
        if tenant_id.is_empty() {
            return Ok(json_response(400, serde_json::json!({"error":"bad_request"})));
        }

        let rec = kv_get_tenant(&tenant_id)?;
        if let Some(rec) = rec {
            // Derive default api_url for convenience if provisioner hasn't written it yet
            let api_url = rec.api_url.clone().unwrap_or_else(|| {
                format!("http://{}.{}", tenant_id, public_base_domain())
            });

            return Ok(json_response(
                200,
                serde_json::json!({
                    "tenant_id": rec.tenant_id,
                    "email": rec.email,
                    "plan": rec.plan,
                    "status": rec.status,
                    "api_url": api_url,
                    "anon_key": rec.anon_key,
                    "service_key": rec.service_key,
                    "dashboard_username": rec.dashboard_username,
                    "dashboard_password": rec.dashboard_password,
                }),
            ));
        } else {
            return Ok(json_response(404, serde_json::json!({"error":"not_found"})));
        }
    }

    // ---- admin actions (token-protected) ----
    if path.starts_with("/v1/admin/tenants/") {
        if let Err(_) = require_admin(&req) {
            return Ok(json_response(401, serde_json::json!({"error":"unauthorized"})));
        }

        // /v1/admin/tenants/{id}/reconcile
        if method == "POST" && path.ends_with("/reconcile") {
            let tenant_id = path
                .trim_start_matches("/v1/admin/tenants/")
                .trim_end_matches("/reconcile")
                .trim_end_matches('/')
                .to_string();

            if tenant_id.is_empty() {
                return Ok(json_response(400, serde_json::json!({"error":"bad_request"})));
            }

            let Some(mut rec) = kv_get_tenant(&tenant_id)? else {
                return Ok(json_response(404, serde_json::json!({"error":"not_found"})));
            };
            rec.status = "reconciling".into();
            kv_put_tenant(&rec)?;

            publish_reconcile_request(&tenant_id)?;

            return Ok(json_response(
                200,
                serde_json::json!({"tenant_id": tenant_id, "status": rec.status}),
            ));
        }

        // /v1/admin/tenants/{id}/deprovision
        if method == "POST" && path.ends_with("/deprovision") {
            let tenant_id = path
                .trim_start_matches("/v1/admin/tenants/")
                .trim_end_matches("/deprovision")
                .trim_end_matches('/')
                .to_string();

            if tenant_id.is_empty() {
                return Ok(json_response(400, serde_json::json!({"error":"bad_request"})));
            }

            let Some(mut rec) = kv_get_tenant(&tenant_id)? else {
                return Ok(json_response(404, serde_json::json!({"error":"not_found"})));
            };
            rec.status = "deprovisioning".into();
            kv_put_tenant(&rec)?;

            publish_deprovision_request(&tenant_id)?;

            return Ok(json_response(
                200,
                serde_json::json!({"tenant_id": tenant_id, "status": rec.status}),
            ));
        }

        return Ok(json_response(404, serde_json::json!({"error":"not_found"})));
    }

    Ok(json_response(404, serde_json::json!({"error":"not_found"})))
}

http::export!(Component);
