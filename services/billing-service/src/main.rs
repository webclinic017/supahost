use anyhow::{anyhow, Context, Result};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use bytes::Bytes;
use futures_util::StreamExt;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{env, net::SocketAddr, sync::Arc};
use time::OffsetDateTime;
use tracing::{error, info};

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
struct AppState {
    nats: async_nats::Client,
    webhook_secret: Option<String>,
    publish_active_subject: String,
    publish_canceled_subject: String,
}

#[derive(Debug, Deserialize)]
struct CheckoutRequest {
    tenant_id: String,
    email: String,
    plan: String,
}

#[derive(Debug, Serialize)]
struct CheckoutResponse {
    checkout_url: String,
    session_id: String,
}

#[derive(Debug, Deserialize)]
struct StripeCheckoutSessionResponse {
    id: String,
    url: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let nats_url = env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_string());
    let billing_subject =
        env::var("BILLING_REQUEST_SUBJECT").unwrap_or_else(|_| "billing.create_checkout".into());

    let publish_active_subject =
        env::var("BILLING_ACTIVE_SUBJECT").unwrap_or_else(|_| "billing.subscription_active".into());
    let publish_canceled_subject = env::var("BILLING_CANCELED_SUBJECT")
        .unwrap_or_else(|_| "billing.subscription_canceled".into());

    let webhook_secret = env::var("STRIPE_WEBHOOK_SECRET").ok();

    let nats = async_nats::connect(nats_url.clone())
        .await
        .context("connect to NATS")?;
    info!("connected to NATS at {}", nats_url);

    let state = Arc::new(AppState {
        nats: nats.clone(),
        webhook_secret,
        publish_active_subject,
        publish_canceled_subject,
    });

    // Task 1: NATS request/reply for checkout creation
    let checkout_state = state.clone();
    let checkout_task = tokio::spawn(async move {
        if let Err(e) = checkout_rpc_loop(nats.clone(), billing_subject, checkout_state).await {
            error!("checkout RPC loop failed: {e:?}");
        }
    });

    // Task 2: HTTP server for Stripe webhooks
    let http_state = state.clone();
    let http_task = tokio::spawn(async move {
        if let Err(e) = run_http(http_state).await {
            error!("http server failed: {e:?}");
        }
    });

    // Wait forever (or until one task fails)
    let _ = tokio::join!(checkout_task, http_task);
    Ok(())
}

async fn run_http(state: Arc<AppState>) -> Result<()> {
    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/stripe/webhook", post(stripe_webhook))
        .with_state(state);

    let addr: SocketAddr = env::var("BILLING_HTTP_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".into())
        .parse()
        .context("BILLING_HTTP_ADDR must be host:port")?;
    info!("billing-service listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn checkout_rpc_loop(
    nats: async_nats::Client,
    subject: String,
    state: Arc<AppState>,
) -> Result<()> {
    let mut sub = nats
        .subscribe(subject.clone())
        .await
        .context("subscribe billing subject")?;
    info!("listening for checkout requests on {}", subject);

    while let Some(msg) = sub.next().await {
        let reply = msg.reply.clone();
        let payload = msg.payload;

        let req: CheckoutRequest = match serde_json::from_slice(&payload) {
            Ok(r) => r,
            Err(e) => {
                error!("invalid checkout request JSON: {e:?}");
                continue;
            }
        };

        let resp = match create_checkout_session(&req).await {
            Ok(r) => r,
            Err(e) => {
                error!("stripe checkout error (tenant={}): {e:?}", req.tenant_id);
                // Reply with error if possible
                if let Some(reply_to) = reply {
                    let _ = nats
                        .publish(reply_to, serde_json::json!({"error": e.to_string()}).to_string().into())
                        .await;
                }
                continue;
            }
        };

        if let Some(reply_to) = reply {
            let bytes = serde_json::to_vec(&resp)?;
            nats.publish(reply_to, bytes.into()).await?;
        } else {
            error!("checkout request had no reply subject");
        }
    }
    Ok(())
}

async fn create_checkout_session(req: &CheckoutRequest) -> Result<CheckoutResponse> {
    let stripe_secret =
        env::var("STRIPE_SECRET_KEY").context("STRIPE_SECRET_KEY is required")?;

    let price_id = price_id_for_plan(&req.plan)?;

    let success_url = env::var("STRIPE_SUCCESS_URL")
        .unwrap_or_else(|_| "http://localhost:8000/success?session_id={CHECKOUT_SESSION_ID}".into());
    let cancel_url = env::var("STRIPE_CANCEL_URL")
        .unwrap_or_else(|_| "http://localhost:8000/cancel".into());

    let client = reqwest::Client::new();
    let url = "https://api.stripe.com/v1/checkout/sessions";

    // Stripe expects x-www-form-urlencoded
    let form = vec![
        ("mode", "subscription"),
        ("success_url", success_url.as_str()),
        ("cancel_url", cancel_url.as_str()),
        ("customer_email", req.email.as_str()),
        ("client_reference_id", req.tenant_id.as_str()),
        ("line_items[0][price]", price_id.as_str()),
        ("line_items[0][quantity]", "1"),
        ("metadata[tenant_id]", req.tenant_id.as_str()),
        ("metadata[plan]", req.plan.as_str()),
    ];

    let resp = client
        .post(url)
        .bearer_auth(stripe_secret)
        .form(&form)
        .send()
        .await
        .context("POST /v1/checkout/sessions")?;

    let status = resp.status();
    let bytes = resp.bytes().await?;

    if !status.is_success() {
        return Err(anyhow!(
            "stripe create session failed: status={} body={}",
            status,
            String::from_utf8_lossy(&bytes)
        ));
    }

    let session: StripeCheckoutSessionResponse =
        serde_json::from_slice(&bytes).context("parse stripe checkout session response")?;

    let checkout_url = session
        .url
        .ok_or_else(|| anyhow!("Stripe response missing checkout URL"))?;

    Ok(CheckoutResponse {
        checkout_url,
        session_id: session.id,
    })
}

fn price_id_for_plan(plan: &str) -> Result<String> {
    let key = format!("STRIPE_PRICE_ID_{}", plan.to_ascii_uppercase());
    env::var(&key).with_context(|| format!("missing env var {key} for plan={plan}"))
}

async fn stripe_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let sig = headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Verify signature if configured
    if let (Some(secret), Some(sig_header)) = (state.webhook_secret.clone(), sig.clone()) {
        if let Err(e) = verify_stripe_signature(&secret, &sig_header, &body, 300) {
            error!("stripe signature verify failed: {e:?}");
            return (StatusCode::BAD_REQUEST, "invalid signature").into_response();
        }
    } else {
        // In dev, you might omit webhook secret (NOT recommended for prod)
        info!("stripe webhook secret not configured; skipping signature verification");
    }

    let event: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            error!("invalid stripe webhook JSON: {e:?}");
            return (StatusCode::BAD_REQUEST, "invalid JSON").into_response();
        }
    };

    let event_type = event
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    // We mainly care about checkout.session.completed for subscription checkouts
    if event_type == "checkout.session.completed" {
        if let Err(e) = handle_checkout_completed(&state, &event).await {
            error!("handle checkout.session.completed error: {e:?}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "error").into_response();
        }
    } else if event_type == "customer.subscription.deleted" {
        if let Err(e) = handle_subscription_deleted(&state, &event).await {
            error!("handle customer.subscription.deleted error: {e:?}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "error").into_response();
        }
    } else {
        // Ignore other events
        info!("ignored stripe event type={}", event_type);
    }

    (StatusCode::OK, "ok").into_response()
}

async fn handle_checkout_completed(state: &Arc<AppState>, event: &serde_json::Value) -> Result<()> {
    let obj = event
        .pointer("/data/object")
        .ok_or_else(|| anyhow!("missing data.object"))?;

    let tenant_id = obj
        .pointer("/metadata/tenant_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing metadata.tenant_id"))?
        .to_string();

    let customer_id = obj.get("customer").and_then(|v| v.as_str()).map(|s| s.to_string());
    let subscription_id = obj
        .get("subscription")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let session_id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();

    #[derive(Serialize)]
    struct ActiveEvent {
        tenant_id: String,
        stripe_customer_id: Option<String>,
        stripe_subscription_id: Option<String>,
        stripe_checkout_session_id: String,
        activated_at: i64,
    }

    let payload = ActiveEvent {
        tenant_id,
        stripe_customer_id: customer_id,
        stripe_subscription_id: subscription_id,
        stripe_checkout_session_id: session_id,
        activated_at: OffsetDateTime::now_utc().unix_timestamp(),
    };

    let bytes = serde_json::to_vec(&payload)?;
    state
        .nats
        .publish(state.publish_active_subject.clone(), bytes.into())
        .await
        .context("publish billing.subscription_active")?;

    Ok(())
}

async fn handle_subscription_deleted(state: &Arc<AppState>, event: &serde_json::Value) -> Result<()> {
    let obj = event
        .pointer("/data/object")
        .ok_or_else(|| anyhow!("missing data.object"))?;

    let tenant_id = obj
        .pointer("/metadata/tenant_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing metadata.tenant_id"))?
        .to_string();

    #[derive(Serialize)]
    struct CanceledEvent {
        tenant_id: String,
        canceled_at: i64,
    }

    let payload = CanceledEvent {
        tenant_id,
        canceled_at: OffsetDateTime::now_utc().unix_timestamp(),
    };

    let bytes = serde_json::to_vec(&payload)?;
    state
        .nats
        .publish(state.publish_canceled_subject.clone(), bytes.into())
        .await
        .context("publish billing.subscription_canceled")?;

    Ok(())
}

/// Verify Stripe webhook signature per Stripe's "t=...,v1=..." scheme.
///
/// Minimal verifier:
/// - Parses `t=<unix>` and `v1=<hexsig>`
/// - Computes HMAC-SHA256(secret, "{t}.{payload}")
/// - Compares to v1
/// - Enforces tolerance in seconds
fn verify_stripe_signature(
    secret: &str,
    signature_header: &str,
    payload: &[u8],
    tolerance_secs: i64,
) -> Result<()> {
    let mut timestamp: Option<i64> = None;
    let mut v1: Option<String> = None;

    for part in signature_header.split(',') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("t=") {
            timestamp = rest.parse::<i64>().ok();
        } else if let Some(rest) = part.strip_prefix("v1=") {
            v1 = Some(rest.to_string());
        }
    }

    let t = timestamp.ok_or_else(|| anyhow!("missing t= in stripe-signature"))?;
    let expected = v1.ok_or_else(|| anyhow!("missing v1= in stripe-signature"))?;

    let now = OffsetDateTime::now_utc().unix_timestamp();
    if (now - t).abs() > tolerance_secs {
        return Err(anyhow!("timestamp outside tolerance"));
    }

    let signed = format!("{}.", t).into_bytes();
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).context("bad webhook secret")?;
    mac.update(&signed);
    mac.update(payload);
    let computed = hex::encode(mac.finalize().into_bytes());

    if !secure_eq(&computed, &expected) {
        return Err(anyhow!("signature mismatch"));
    }

    Ok(())
}

// Constant-time-ish string compare
fn secure_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut out: u8 = 0;
    for (x, y) in a.as_bytes().iter().zip(b.as_bytes().iter()) {
        out |= x ^ y;
    }
    out == 0
}

// Needed for hmac verification
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let b = bytes.as_ref();
        let mut s = String::with_capacity(b.len() * 2);
        for &v in b {
            s.push(HEX[(v >> 4) as usize] as char);
            s.push(HEX[(v & 0x0f) as usize] as char);
        }
        s
    }
}
