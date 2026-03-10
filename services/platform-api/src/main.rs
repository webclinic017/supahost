use anyhow::{anyhow, Context, Result};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::{env, net::SocketAddr, sync::Arc};
use tracing::info;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    nats: async_nats::Client,
    redis: redis::Client,
    tenant_key_prefix: String,
    billing_mode: String,
    billing_request_subject: String,
    provision_request_subject: String,
    pause_request_subject: String,
    resume_request_subject: String,
    delete_request_subject: String,
    deprovision_request_subject: String,
    reconcile_request_subject: String,
    public_tenant_base_domain: String,
    admin_token: Option<String>,
}

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
    desired_state: String,
    deployment_mode: String,
    active_deployment_count: u8,
    stripe_checkout_session_id: Option<String>,
    stripe_customer_id: Option<String>,
    stripe_subscription_id: Option<String>,
    api_url: Option<String>,
    gateway_url: Option<String>,
    console_url: Option<String>,
    anon_key: Option<String>,
    service_key: Option<String>,
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

#[derive(Debug, Deserialize)]
struct TenantPath {
    tenant_id: String,
}

#[derive(Debug, Serialize)]
struct AckResponse {
    tenant_id: String,
    status: String,
    desired_state: String,
    active_deployment_count: u8,
}

#[derive(Debug, Clone, Copy)]
enum LifecycleAction {
    Pause,
    Resume,
    Delete,
    Reconcile,
}

impl LifecycleAction {
    fn subject<'a>(&self, state: &'a AppState) -> &'a str {
        match self {
            Self::Pause => &state.pause_request_subject,
            Self::Resume => &state.resume_request_subject,
            Self::Delete => &state.delete_request_subject,
            Self::Reconcile => &state.reconcile_request_subject,
        }
    }

    fn desired_state(&self) -> &'static str {
        match self {
            Self::Pause => "paused",
            Self::Resume | Self::Reconcile => "active",
            Self::Delete => "deleted",
        }
    }

    fn next_status(&self, rec: &TenantRecord) -> String {
        match self {
            Self::Pause => {
                if rec.active_deployment_count == 0
                    && matches!(
                        rec.status.as_str(),
                        "pending_checkout" | "checkout_created" | "paused" | "pause_requested"
                    )
                {
                    "paused".into()
                } else {
                    "pausing".into()
                }
            }
            Self::Resume => {
                if rec.status == "active" && rec.active_deployment_count == 1 {
                    "active".into()
                } else {
                    "provisioning".into()
                }
            }
            Self::Reconcile => "reconciling".into(),
            Self::Delete => {
                if rec.status == "deleted" {
                    "deleted".into()
                } else {
                    "deleting".into()
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let nats_url = env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".into());
    let redis_url = env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());

    let bind_addr: SocketAddr = env::var("PLATFORM_API_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8000".into())
        .parse()
        .context("PLATFORM_API_BIND must be host:port")?;

    let state = Arc::new(AppState {
        nats: async_nats::connect(nats_url.clone()).await.context("connect NATS")?,
        redis: redis::Client::open(redis_url.clone()).context("parse REDIS_URL")?,
        tenant_key_prefix: env::var("TENANT_KEY_PREFIX").unwrap_or_else(|_| "tenant:".into()),
        billing_mode: env::var("BILLING_MODE").unwrap_or_else(|_| "direct".into()),
        billing_request_subject: env::var("BILLING_REQUEST_SUBJECT")
            .unwrap_or_else(|_| "billing.create_checkout".into()),
        provision_request_subject: env::var("PROVISION_REQUEST_SUBJECT")
            .unwrap_or_else(|_| "tenant.provision".into()),
        pause_request_subject: env::var("PAUSE_REQUEST_SUBJECT")
            .unwrap_or_else(|_| "tenant.pause".into()),
        resume_request_subject: env::var("RESUME_REQUEST_SUBJECT")
            .unwrap_or_else(|_| "tenant.resume".into()),
        delete_request_subject: env::var("DELETE_REQUEST_SUBJECT")
            .unwrap_or_else(|_| "tenant.delete".into()),
        deprovision_request_subject: env::var("DEPROVISION_REQUEST_SUBJECT")
            .unwrap_or_else(|_| "tenant.deprovision".into()),
        reconcile_request_subject: env::var("RECONCILE_REQUEST_SUBJECT")
            .unwrap_or_else(|_| "tenant.reconcile".into()),
        public_tenant_base_domain: env::var("PUBLIC_TENANT_BASE_DOMAIN")
            .unwrap_or_else(|_| "supabase.localhost".into()),
        admin_token: env::var("PLATFORM_ADMIN_TOKEN").ok(),
    });

    info!("platform-api listening on http://{}", bind_addr);
    info!("connected to NATS at {}", nats_url);
    info!("connected to Redis at {}", redis_url);

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/tenants", post(create_tenant))
        .route("/v1/tenants/:tenant_id", get(get_tenant).delete(delete_tenant))
        .route("/v1/tenants/:tenant_id/pause", post(pause_tenant))
        .route("/v1/tenants/:tenant_id/resume", post(resume_tenant))
        .route("/v1/admin/tenants/:tenant_id/reconcile", post(admin_reconcile))
        .route("/v1/admin/tenants/:tenant_id/deprovision", post(admin_deprovision))
        .route("/v1/admin/tenants/:tenant_id/pause", post(admin_pause))
        .route("/v1/admin/tenants/:tenant_id/resume", post(admin_resume))
        .route("/v1/admin/tenants/:tenant_id", delete(admin_delete))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;

    Ok(())
}

async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({"ok": true}))
}

async fn create_tenant(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateTenantRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let tenant_id = Uuid::new_v4().to_string();
    let gateway_url = tenant_gateway_url(&state, &tenant_id);
    let console_url = tenant_console_url(&state, &tenant_id);

    let mut rec = TenantRecord {
        tenant_id: tenant_id.clone(),
        email: payload.email.clone(),
        plan: payload.plan.clone(),
        status: "provisioning".into(),
        desired_state: "active".into(),
        deployment_mode: "single".into(),
        active_deployment_count: 0,
        stripe_checkout_session_id: None,
        stripe_customer_id: None,
        stripe_subscription_id: None,
        api_url: Some(gateway_url.clone()),
        gateway_url: Some(gateway_url.clone()),
        console_url: Some(console_url.clone()),
        anon_key: None,
        service_key: None,
        dashboard_username: None,
        dashboard_password: None,
    };

    put_tenant(&state, &rec).await?;

    if state.billing_mode == "stripe" {
        rec.status = "pending_checkout".into();
        put_tenant(&state, &rec).await?;

        let checkout = call_billing_create_checkout(
            &state,
            &CheckoutRequest {
                tenant_id: tenant_id.clone(),
                email: payload.email,
                plan: payload.plan,
            },
        )
        .await?;

        rec.status = "checkout_created".into();
        rec.stripe_checkout_session_id = Some(checkout.session_id.clone());
        put_tenant(&state, &rec).await?;

        Ok((
            StatusCode::OK,
            Json(serde_json::json!({
                "tenant_id": tenant_id,
                "status": rec.status,
                "desired_state": rec.desired_state,
                "deployment_mode": rec.deployment_mode,
                "active_deployment_count": rec.active_deployment_count,
                "checkout_url": checkout.checkout_url,
                "api_url": rec.api_url,
                "gateway_url": rec.gateway_url,
                "console_url": rec.console_url,
            })),
        ))
    } else {
        publish_json(
            &state.nats,
            &state.provision_request_subject,
            &serde_json::json!({
                "tenant_id": tenant_id,
                "email": payload.email,
                "plan": payload.plan,
            }),
        )
        .await?;

        Ok((
            StatusCode::OK,
            Json(serde_json::json!({
                "tenant_id": tenant_id,
                "status": rec.status,
                "desired_state": rec.desired_state,
                "deployment_mode": rec.deployment_mode,
                "active_deployment_count": rec.active_deployment_count,
                "api_url": rec.api_url,
                "gateway_url": rec.gateway_url,
                "console_url": rec.console_url,
            })),
        ))
    }
}

async fn get_tenant(
    State(state): State<Arc<AppState>>,
    Path(TenantPath { tenant_id }): Path<TenantPath>,
) -> Result<impl IntoResponse, ApiError> {
    match get_tenant_record(&state, &tenant_id).await? {
        Some(rec) => Ok((StatusCode::OK, Json(serde_json::to_value(rec)?))),
        None => Ok((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not_found"})))),
    }
}

async fn pause_tenant(
    State(state): State<Arc<AppState>>,
    Path(TenantPath { tenant_id }): Path<TenantPath>,
) -> Result<impl IntoResponse, ApiError> {
    lifecycle_action(&state, &tenant_id, LifecycleAction::Pause, false, None).await
}

async fn resume_tenant(
    State(state): State<Arc<AppState>>,
    Path(TenantPath { tenant_id }): Path<TenantPath>,
) -> Result<impl IntoResponse, ApiError> {
    lifecycle_action(&state, &tenant_id, LifecycleAction::Resume, false, None).await
}

async fn delete_tenant(
    State(state): State<Arc<AppState>>,
    Path(TenantPath { tenant_id }): Path<TenantPath>,
) -> Result<impl IntoResponse, ApiError> {
    lifecycle_action(&state, &tenant_id, LifecycleAction::Delete, false, None).await
}

async fn admin_pause(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(TenantPath { tenant_id }): Path<TenantPath>,
) -> Result<impl IntoResponse, ApiError> {
    lifecycle_action(&state, &tenant_id, LifecycleAction::Pause, true, Some(&headers)).await
}

async fn admin_resume(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(TenantPath { tenant_id }): Path<TenantPath>,
) -> Result<impl IntoResponse, ApiError> {
    lifecycle_action(&state, &tenant_id, LifecycleAction::Resume, true, Some(&headers)).await
}

async fn admin_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(TenantPath { tenant_id }): Path<TenantPath>,
) -> Result<impl IntoResponse, ApiError> {
    lifecycle_action(&state, &tenant_id, LifecycleAction::Delete, true, Some(&headers)).await
}

async fn admin_reconcile(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(TenantPath { tenant_id }): Path<TenantPath>,
) -> Result<impl IntoResponse, ApiError> {
    lifecycle_action(&state, &tenant_id, LifecycleAction::Reconcile, true, Some(&headers)).await
}

async fn admin_deprovision(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(TenantPath { tenant_id }): Path<TenantPath>,
) -> Result<impl IntoResponse, ApiError> {
    lifecycle_action(&state, &tenant_id, LifecycleAction::Pause, true, Some(&headers)).await
}

async fn lifecycle_action(
    state: &Arc<AppState>,
    tenant_id: &str,
    action: LifecycleAction,
    require_admin_auth: bool,
    headers: Option<&HeaderMap>,
) -> Result<impl IntoResponse, ApiError> {
    if require_admin_auth {
        require_admin(state, headers.ok_or_else(|| anyhow!("missing headers"))?)?;
    }

    let mut rec = get_tenant_record(state, tenant_id)
        .await?
        .ok_or_else(|| ApiError::not_found("not_found"))?;

    if rec.status == "deleted" && !matches!(action, LifecycleAction::Delete) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "deleted deployments cannot be resumed or paused",
        ));
    }

    rec.desired_state = action.desired_state().into();
    rec.status = action.next_status(&rec);
    if matches!(action, LifecycleAction::Pause | LifecycleAction::Delete) && rec.active_deployment_count == 0 {
        rec.status = action.desired_state().into();
    }
    if matches!(action, LifecycleAction::Delete) {
        rec.active_deployment_count = 0;
    }
    put_tenant(state, &rec).await?;

    let payload = match action {
        LifecycleAction::Resume => serde_json::json!({
            "tenant_id": rec.tenant_id,
            "email": rec.email,
            "plan": rec.plan,
        }),
        _ => serde_json::json!({ "tenant_id": rec.tenant_id }),
    };

    publish_json(&state.nats, action.subject(state), &payload).await?;

    Ok((
        StatusCode::OK,
        Json(serde_json::to_value(AckResponse {
            tenant_id: rec.tenant_id,
            status: rec.status,
            desired_state: rec.desired_state,
            active_deployment_count: rec.active_deployment_count,
        })?),
    ))
}

fn tenant_gateway_url(state: &AppState, tenant_id: &str) -> String {
    format!("http://{}.{}", tenant_id, state.public_tenant_base_domain)
}

fn tenant_console_url(state: &AppState, tenant_id: &str) -> String {
    tenant_gateway_url(state, tenant_id)
}

async fn publish_json(nats: &async_nats::Client, subject: &str, value: &serde_json::Value) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    nats.publish(subject.to_string(), bytes.into()).await?;
    Ok(())
}

async fn call_billing_create_checkout(
    state: &AppState,
    payload: &CheckoutRequest,
) -> Result<CheckoutResponse> {
    let bytes = serde_json::to_vec(payload)?;
    let msg = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        state
            .nats
            .request(state.billing_request_subject.clone(), bytes.into()),
    )
    .await
    .context("billing request timed out")??;

    let value: serde_json::Value = serde_json::from_slice(&msg.payload)?;
    if let Some(err) = value.get("error").and_then(|v| v.as_str()) {
        return Err(anyhow!("billing service error: {err}"));
    }
    Ok(serde_json::from_value(value)?)
}

fn require_admin(state: &AppState, headers: &HeaderMap) -> Result<()> {
    let Some(expected) = &state.admin_token else {
        return Ok(());
    };
    let actual = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let want = format!("Bearer {}", expected);
    if actual != want {
        return Err(anyhow!("unauthorized"));
    }
    Ok(())
}

async fn get_tenant_record(state: &AppState, tenant_id: &str) -> Result<Option<TenantRecord>> {
    let mut conn = state.redis.get_multiplexed_async_connection().await?;
    let key = format!("{}{}", state.tenant_key_prefix, tenant_id);
    let bytes: Option<Vec<u8>> = conn.get(key).await?;
    match bytes {
        Some(value) => Ok(Some(serde_json::from_slice(&value)?)),
        None => Ok(None),
    }
}

async fn put_tenant(state: &AppState, rec: &TenantRecord) -> Result<()> {
    let mut conn = state.redis.get_multiplexed_async_connection().await?;
    let key = format!("{}{}", state.tenant_key_prefix, rec.tenant_id);
    let value = serde_json::to_vec(rec)?;
    conn.set::<_, _, ()>(key, value).await?;
    Ok(())
}

struct ApiError {
    status: StatusCode,
    body: serde_json::Value,
}

impl ApiError {
    fn new(status: StatusCode, message: impl ToString) -> Self {
        Self {
            status,
            body: serde_json::json!({
                "error": status.canonical_reason().unwrap_or("error").to_lowercase().replace(' ', "_"),
                "message": message.to_string(),
            }),
        }
    }

    fn not_found(message: impl ToString) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            body: serde_json::json!({ "error": message.to_string() }),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(self.body)).into_response()
    }
}

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(value: E) -> Self {
        let err: anyhow::Error = value.into();
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, err)
    }
}
