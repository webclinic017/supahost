use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use jsonwebtoken::{encode, EncodingKey, Header};
use rand::{distributions::Alphanumeric, Rng};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use serde_yaml::Value as Yaml;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::process::Command;
use tracing::{error, info};

#[derive(Debug, Deserialize)]
struct BillingActiveEvent {
    tenant_id: String,
    stripe_customer_id: Option<String>,
    stripe_subscription_id: Option<String>,
    stripe_checkout_session_id: Option<String>,
    activated_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct BillingCanceledEvent {
    tenant_id: String,
    canceled_at: Option<i64>,
}

#[derive(Debug, Deserialize, Clone)]
struct ProvisionRequest {
    tenant_id: String,
    email: Option<String>,
    plan: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PauseRequest {
    tenant_id: String,
}

#[derive(Debug, Deserialize, Clone)]
struct ResumeRequest {
    tenant_id: String,
    email: Option<String>,
    plan: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeleteRequest {
    tenant_id: String,
}

#[derive(Debug, Deserialize)]
struct DeprovisionRequest {
    tenant_id: String,
}

#[derive(Debug, Deserialize)]
struct ReconcileRequest {
    tenant_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct TenantRecord {
    tenant_id: String,
    email: Option<String>,
    plan: Option<String>,
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

#[derive(Debug, Serialize)]
struct JwtClaims<'a> {
    role: &'a str,
    iss: &'a str,
    iat: i64,
    exp: i64,
}

#[derive(Debug, Serialize)]
struct ProvisionAck {
    tenant_id: String,
    status: String,
    api_url: Option<String>,
    gateway_url: Option<String>,
    console_url: Option<String>,
    active_deployment_count: u8,
}

#[derive(Debug, Serialize)]
struct ActionAck {
    tenant_id: String,
    status: String,
    desired_state: String,
    active_deployment_count: u8,
}

#[derive(Debug, Clone)]
struct TenantSecrets {
    postgres_password: String,
    jwt_secret: String,
    anon_key: String,
    service_key: String,
    dashboard_username: String,
    dashboard_password: String,
    secret_key_base: String,
    vault_enc_key: String,
    pg_meta_crypto_key: String,
    logflare_public: String,
    logflare_private: String,
    s3_protocol_access_key_id: String,
    s3_protocol_access_key_secret: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let nats_url = env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_string());
    let active_subject =
        env::var("BILLING_ACTIVE_SUBJECT").unwrap_or_else(|_| "billing.subscription_active".into());
    let canceled_subject = env::var("BILLING_CANCELED_SUBJECT")
        .unwrap_or_else(|_| "billing.subscription_canceled".into());

    let provision_subject =
        env::var("PROVISION_REQUEST_SUBJECT").unwrap_or_else(|_| "tenant.provision".into());
    let pause_subject = env::var("PAUSE_REQUEST_SUBJECT").unwrap_or_else(|_| "tenant.pause".into());
    let resume_subject = env::var("RESUME_REQUEST_SUBJECT").unwrap_or_else(|_| "tenant.resume".into());
    let delete_subject = env::var("DELETE_REQUEST_SUBJECT").unwrap_or_else(|_| "tenant.delete".into());
    let deprovision_subject =
        env::var("DEPROVISION_REQUEST_SUBJECT").unwrap_or_else(|_| "tenant.deprovision".into());
    let reconcile_subject =
        env::var("RECONCILE_REQUEST_SUBJECT").unwrap_or_else(|_| "tenant.reconcile".into());

    let redis_url = env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let tenant_key_prefix = env::var("TENANT_KEY_PREFIX").unwrap_or_else(|_| "tenant:".into());

    let tenants_dir = env::var("TENANTS_DIR").unwrap_or_else(|_| "./tenants".into());
    let base_domain =
        env::var("PUBLIC_TENANT_BASE_DOMAIN").unwrap_or_else(|_| "supabase.localhost".into());
    let public_web_base_url = env::var("PUBLIC_WEB_BASE_URL")
        .unwrap_or_else(|_| "http://localhost:3000".into());
    let supabase_vendor_dir =
        env::var("SUPABASE_VENDOR_DIR").unwrap_or_else(|_| "./vendor/supabase-docker".into());

    let backend = env::var("PROVISION_BACKEND").unwrap_or_else(|_| "docker".into());
    let docker_network = env::var("DOCKER_NETWORK").unwrap_or_else(|_| "supahost".into());

    info!("connecting to NATS at {}", nats_url);
    let nats = async_nats::connect(nats_url).await.context("connect NATS")?;

    info!("connecting to Redis at {}", redis_url);
    let redis = redis::Client::open(redis_url).context("parse Redis URL")?;

    if backend == "docker" {
        ensure_docker_network(&docker_network).await?;
    } else {
        info!("PROVISION_BACKEND={} (no containers will be started)", backend);
    }

    let mut sub_active = nats.subscribe(active_subject.clone()).await?;
    let mut sub_canceled = nats.subscribe(canceled_subject.clone()).await?;
    let mut sub_provision = nats.subscribe(provision_subject.clone()).await?;
    let mut sub_pause = nats.subscribe(pause_subject.clone()).await?;
    let mut sub_resume = nats.subscribe(resume_subject.clone()).await?;
    let mut sub_delete = nats.subscribe(delete_subject.clone()).await?;
    let mut sub_deprovision = nats.subscribe(deprovision_subject.clone()).await?;
    let mut sub_reconcile = nats.subscribe(reconcile_subject.clone()).await?;

    info!("subscribed to billing subjects: {} and {}", active_subject, canceled_subject);
    info!(
        "subscribed to lifecycle subjects: {}, {}, {}, {}, {}",
        provision_subject, pause_subject, resume_subject, delete_subject, reconcile_subject
    );

    loop {
        tokio::select! {
            maybe = sub_active.next() => {
                if let Some(msg) = maybe {
                    if let Err(e) = handle_active(&nats, &redis, &tenant_key_prefix, &tenants_dir, &base_domain, &public_web_base_url, &supabase_vendor_dir, &backend, &docker_network, &msg).await {
                        error!("handle_active error: {e:?}");
                    }
                }
            }
            maybe = sub_canceled.next() => {
                if let Some(msg) = maybe {
                    if let Err(e) = handle_canceled(&nats, &redis, &tenant_key_prefix, &tenants_dir, &base_domain, &public_web_base_url, &backend, &msg).await {
                        error!("handle_canceled error: {e:?}");
                    }
                }
            }
            maybe = sub_provision.next() => {
                if let Some(msg) = maybe {
                    if let Err(e) = handle_provision_request(&nats, &redis, &tenant_key_prefix, &tenants_dir, &base_domain, &public_web_base_url, &supabase_vendor_dir, &backend, &docker_network, &msg).await {
                        error!("handle_provision_request error: {e:?}");
                    }
                }
            }
            maybe = sub_pause.next() => {
                if let Some(msg) = maybe {
                    if let Err(e) = handle_pause_request(&nats, &redis, &tenant_key_prefix, &tenants_dir, &base_domain, &public_web_base_url, &backend, &msg).await {
                        error!("handle_pause_request error: {e:?}");
                    }
                }
            }
            maybe = sub_resume.next() => {
                if let Some(msg) = maybe {
                    if let Err(e) = handle_resume_request(&nats, &redis, &tenant_key_prefix, &tenants_dir, &base_domain, &public_web_base_url, &supabase_vendor_dir, &backend, &docker_network, &msg).await {
                        error!("handle_resume_request error: {e:?}");
                    }
                }
            }
            maybe = sub_delete.next() => {
                if let Some(msg) = maybe {
                    if let Err(e) = handle_delete_request(&nats, &redis, &tenant_key_prefix, &tenants_dir, &base_domain, &public_web_base_url, &backend, &msg).await {
                        error!("handle_delete_request error: {e:?}");
                    }
                }
            }
            maybe = sub_deprovision.next() => {
                if let Some(msg) = maybe {
                    if let Err(e) = handle_deprovision_request(&nats, &redis, &tenant_key_prefix, &tenants_dir, &base_domain, &public_web_base_url, &backend, &msg).await {
                        error!("handle_deprovision_request error: {e:?}");
                    }
                }
            }
            maybe = sub_reconcile.next() => {
                if let Some(msg) = maybe {
                    if let Err(e) = handle_reconcile_request(&nats, &redis, &tenant_key_prefix, &tenants_dir, &base_domain, &public_web_base_url, &supabase_vendor_dir, &backend, &docker_network, &msg).await {
                        error!("handle_reconcile_request error: {e:?}");
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                info!("ctrl-c received, exiting");
                break;
            }
        }
    }

    Ok(())
}

async fn handle_active(
    nats: &async_nats::Client,
    redis: &redis::Client,
    tenant_key_prefix: &str,
    tenants_dir: &str,
    base_domain: &str,
    public_web_base_url: &str,
    supabase_vendor_dir: &str,
    backend: &str,
    docker_network: &str,
    msg: &async_nats::Message,
) -> Result<()> {
    let evt: BillingActiveEvent =
        serde_json::from_slice(&msg.payload).context("parse billing.subscription_active")?;
    info!("provision requested (billing active) for tenant={}", evt.tenant_id);
    let _ = evt.activated_at;

    let out = provision_and_persist(
        redis,
        tenant_key_prefix,
        tenants_dir,
        base_domain,
        public_web_base_url,
        supabase_vendor_dir,
        backend,
        docker_network,
        &evt.tenant_id,
        None,
        None,
        evt.stripe_checkout_session_id,
        evt.stripe_customer_id,
        evt.stripe_subscription_id,
    )
    .await?;

    if let Some(reply) = &msg.reply {
        let _ = nats.publish(reply.clone(), serde_json::to_vec(&out)?.into()).await;
    }

    Ok(())
}

async fn handle_canceled(
    nats: &async_nats::Client,
    redis: &redis::Client,
    tenant_key_prefix: &str,
    tenants_dir: &str,
    base_domain: &str,
    public_web_base_url: &str,
    backend: &str,
    msg: &async_nats::Message,
) -> Result<()> {
    let evt: BillingCanceledEvent =
        serde_json::from_slice(&msg.payload).context("parse billing.subscription_canceled")?;
    info!("billing canceled for tenant={}, marking suspended", evt.tenant_id);
    let _ = evt.canceled_at;

    let out = pause_and_persist(
        redis,
        tenant_key_prefix,
        tenants_dir,
        base_domain,
        public_web_base_url,
        backend,
        &evt.tenant_id,
        "suspended",
    )
    .await?;

    if let Some(reply) = &msg.reply {
        let _ = nats.publish(reply.clone(), serde_json::to_vec(&out)?.into()).await;
    }
    Ok(())
}

async fn handle_provision_request(
    nats: &async_nats::Client,
    redis: &redis::Client,
    tenant_key_prefix: &str,
    tenants_dir: &str,
    base_domain: &str,
    public_web_base_url: &str,
    supabase_vendor_dir: &str,
    backend: &str,
    docker_network: &str,
    msg: &async_nats::Message,
) -> Result<()> {
    let req: ProvisionRequest = serde_json::from_slice(&msg.payload).context("parse tenant.provision")?;
    info!("provision requested (manual) for tenant={}", req.tenant_id);

    let out = provision_and_persist(
        redis,
        tenant_key_prefix,
        tenants_dir,
        base_domain,
        public_web_base_url,
        supabase_vendor_dir,
        backend,
        docker_network,
        &req.tenant_id,
        req.email,
        req.plan,
        None,
        None,
        None,
    )
    .await?;

    if let Some(reply) = &msg.reply {
        let _ = nats.publish(reply.clone(), serde_json::to_vec(&out)?.into()).await;
    }
    Ok(())
}

async fn handle_pause_request(
    nats: &async_nats::Client,
    redis: &redis::Client,
    tenant_key_prefix: &str,
    tenants_dir: &str,
    base_domain: &str,
    public_web_base_url: &str,
    backend: &str,
    msg: &async_nats::Message,
) -> Result<()> {
    let req: PauseRequest = serde_json::from_slice(&msg.payload).context("parse tenant.pause")?;
    info!("pause requested for tenant={}", req.tenant_id);

    let out = pause_and_persist(
        redis,
        tenant_key_prefix,
        tenants_dir,
        base_domain,
        public_web_base_url,
        backend,
        &req.tenant_id,
        "paused",
    )
    .await?;

    if let Some(reply) = &msg.reply {
        let _ = nats.publish(reply.clone(), serde_json::to_vec(&out)?.into()).await;
    }
    Ok(())
}

async fn handle_resume_request(
    nats: &async_nats::Client,
    redis: &redis::Client,
    tenant_key_prefix: &str,
    tenants_dir: &str,
    base_domain: &str,
    public_web_base_url: &str,
    supabase_vendor_dir: &str,
    backend: &str,
    docker_network: &str,
    msg: &async_nats::Message,
) -> Result<()> {
    let req: ResumeRequest = serde_json::from_slice(&msg.payload).context("parse tenant.resume")?;
    info!("resume requested for tenant={}", req.tenant_id);

    let existing = fetch_tenant_record(redis, tenant_key_prefix, &req.tenant_id).await?;
    let email = req
        .email
        .or_else(|| existing.as_ref().and_then(|r| r.email.clone()));
    let plan = req
        .plan
        .or_else(|| existing.as_ref().and_then(|r| r.plan.clone()));

    let out = provision_and_persist(
        redis,
        tenant_key_prefix,
        tenants_dir,
        base_domain,
        public_web_base_url,
        supabase_vendor_dir,
        backend,
        docker_network,
        &req.tenant_id,
        email,
        plan,
        existing.as_ref().and_then(|r| r.stripe_checkout_session_id.clone()),
        existing.as_ref().and_then(|r| r.stripe_customer_id.clone()),
        existing.as_ref().and_then(|r| r.stripe_subscription_id.clone()),
    )
    .await?;

    if let Some(reply) = &msg.reply {
        let _ = nats.publish(reply.clone(), serde_json::to_vec(&out)?.into()).await;
    }
    Ok(())
}

async fn handle_delete_request(
    nats: &async_nats::Client,
    redis: &redis::Client,
    tenant_key_prefix: &str,
    tenants_dir: &str,
    base_domain: &str,
    public_web_base_url: &str,
    backend: &str,
    msg: &async_nats::Message,
) -> Result<()> {
    let req: DeleteRequest = serde_json::from_slice(&msg.payload).context("parse tenant.delete")?;
    info!("delete requested for tenant={}", req.tenant_id);

    let out = delete_and_persist(
        redis,
        tenant_key_prefix,
        tenants_dir,
        base_domain,
        public_web_base_url,
        backend,
        &req.tenant_id,
    )
    .await?;

    if let Some(reply) = &msg.reply {
        let _ = nats.publish(reply.clone(), serde_json::to_vec(&out)?.into()).await;
    }
    Ok(())
}

async fn handle_deprovision_request(
    nats: &async_nats::Client,
    redis: &redis::Client,
    tenant_key_prefix: &str,
    tenants_dir: &str,
    base_domain: &str,
    public_web_base_url: &str,
    backend: &str,
    msg: &async_nats::Message,
) -> Result<()> {
    let req: DeprovisionRequest = serde_json::from_slice(&msg.payload).context("parse tenant.deprovision")?;
    info!("deprovision requested (alias pause) for tenant={}", req.tenant_id);

    let out = pause_and_persist(
        redis,
        tenant_key_prefix,
        tenants_dir,
        base_domain,
        public_web_base_url,
        backend,
        &req.tenant_id,
        "paused",
    )
    .await?;

    if let Some(reply) = &msg.reply {
        let _ = nats.publish(reply.clone(), serde_json::to_vec(&out)?.into()).await;
    }
    Ok(())
}

async fn handle_reconcile_request(
    nats: &async_nats::Client,
    redis: &redis::Client,
    tenant_key_prefix: &str,
    tenants_dir: &str,
    base_domain: &str,
    public_web_base_url: &str,
    supabase_vendor_dir: &str,
    backend: &str,
    docker_network: &str,
    msg: &async_nats::Message,
) -> Result<()> {
    let req: ReconcileRequest = serde_json::from_slice(&msg.payload).context("parse tenant.reconcile")?;
    info!("reconcile requested for tenant={}", req.tenant_id);

    let existing = fetch_tenant_record(redis, tenant_key_prefix, &req.tenant_id).await?;
    let out = provision_and_persist(
        redis,
        tenant_key_prefix,
        tenants_dir,
        base_domain,
        public_web_base_url,
        supabase_vendor_dir,
        backend,
        docker_network,
        &req.tenant_id,
        existing.as_ref().and_then(|r| r.email.clone()),
        existing.as_ref().and_then(|r| r.plan.clone()),
        existing.as_ref().and_then(|r| r.stripe_checkout_session_id.clone()),
        existing.as_ref().and_then(|r| r.stripe_customer_id.clone()),
        existing.as_ref().and_then(|r| r.stripe_subscription_id.clone()),
    )
    .await?;

    if let Some(reply) = &msg.reply {
        let _ = nats.publish(reply.clone(), serde_json::to_vec(&out)?.into()).await;
    }
    Ok(())
}

async fn provision_and_persist(
    redis: &redis::Client,
    tenant_key_prefix: &str,
    tenants_dir: &str,
    base_domain: &str,
    public_web_base_url: &str,
    supabase_vendor_dir: &str,
    backend: &str,
    docker_network: &str,
    tenant_id: &str,
    email: Option<String>,
    plan: Option<String>,
    stripe_checkout_session_id: Option<String>,
    stripe_customer_id: Option<String>,
    stripe_subscription_id: Option<String>,
) -> Result<ProvisionAck> {
    let existing = fetch_tenant_record(redis, tenant_key_prefix, tenant_id).await?;
    if let Some(rec) = &existing {
        if rec.status == "deleted" || rec.desired_state == "deleted" {
            return Ok(ProvisionAck {
                tenant_id: tenant_id.to_string(),
                status: "deleted".into(),
                api_url: rec.api_url.clone(),
                gateway_url: rec.gateway_url.clone(),
                console_url: rec.console_url.clone(),
                active_deployment_count: 0,
            });
        }
        if rec.status == "active" && rec.desired_state == "active" && rec.active_deployment_count == 1 {
            return Ok(ProvisionAck {
                tenant_id: tenant_id.to_string(),
                status: rec.status.clone(),
                api_url: rec.api_url.clone(),
                gateway_url: rec.gateway_url.clone(),
                console_url: rec.console_url.clone(),
                active_deployment_count: rec.active_deployment_count,
            });
        }
    }

    let gateway_url = tenant_gateway_url(base_domain, tenant_id);
    let console_url = tenant_console_url(base_domain, tenant_id);

    let mut rec = existing.unwrap_or_else(|| empty_tenant_record(tenant_id, base_domain, public_web_base_url));
    if rec.email.is_none() {
        rec.email = email;
    }
    if rec.plan.is_none() {
        rec.plan = plan;
    }
    rec.desired_state = "active".into();
    rec.deployment_mode = "single".into();
    rec.status = if rec.active_deployment_count == 1 { "active".into() } else { "provisioning".into() };
    rec.api_url = Some(gateway_url.clone());
    rec.gateway_url = Some(gateway_url.clone());
    rec.console_url = Some(console_url.clone());
    if stripe_checkout_session_id.is_some() {
        rec.stripe_checkout_session_id = stripe_checkout_session_id.clone();
    }
    if stripe_customer_id.is_some() {
        rec.stripe_customer_id = stripe_customer_id.clone();
    }
    if stripe_subscription_id.is_some() {
        rec.stripe_subscription_id = stripe_subscription_id.clone();
    }
    persist_tenant_record(redis, tenant_key_prefix, &rec).await?;

    let tenant_supabase_dir = tenant_supabase_path(tenants_dir, tenant_id);
    let is_fresh = !tenant_supabase_dir.exists();

    if backend == "docker" {
        if is_fresh {
            let postgres_password = rand_string(40);
            let jwt_secret = rand_string(64);
            let anon_key = sign_jwt(&jwt_secret, "anon")?;
            let service_key = sign_jwt(&jwt_secret, "service_role")?;
            let dashboard_username = env::var("TENANT_DASHBOARD_USERNAME").unwrap_or_else(|_| "supabase".into());
            let dashboard_password = rand_string(24);

            let secrets = TenantSecrets {
                postgres_password,
                jwt_secret,
                anon_key: anon_key.clone(),
                service_key: service_key.clone(),
                dashboard_username: dashboard_username.clone(),
                dashboard_password: dashboard_password.clone(),
                secret_key_base: rand_string(64),
                vault_enc_key: rand_string(32),
                pg_meta_crypto_key: rand_string(32),
                logflare_public: rand_string(48),
                logflare_private: rand_string(48),
                s3_protocol_access_key_id: rand_hex(32),
                s3_protocol_access_key_secret: rand_hex(64),
            };

            provision_docker_official_supabase(
                tenant_id,
                tenants_dir,
                base_domain,
                docker_network,
                supabase_vendor_dir,
                &secrets,
            )
            .await?;

            rec.anon_key = Some(anon_key);
            rec.service_key = Some(service_key);
            rec.dashboard_username = Some(dashboard_username);
            rec.dashboard_password = Some(dashboard_password);
        } else {
            reconcile_docker(tenant_id, tenants_dir, base_domain, docker_network, supabase_vendor_dir).await?;
        }
    } else {
        info!("PROVISION_BACKEND={} (skipping containers)", backend);
    }

    let latest = fetch_tenant_record(redis, tenant_key_prefix, tenant_id).await?;
    let final_desired = latest
        .as_ref()
        .map(|r| r.desired_state.clone())
        .unwrap_or_else(|| "active".into());

    rec.status = match final_desired.as_str() {
        "paused" => {
            if backend == "docker" {
                deprovision_docker(tenant_id, tenants_dir).await?;
            }
            rec.active_deployment_count = 0;
            "paused".into()
        }
        "deleted" => {
            if backend == "docker" {
                delete_docker(tenant_id, tenants_dir).await?;
            }
            rec.active_deployment_count = 0;
            "deleted".into()
        }
        _ => {
            rec.active_deployment_count = 1;
            "active".into()
        }
    };
    rec.desired_state = final_desired;
    rec.api_url = Some(gateway_url.clone());
    rec.gateway_url = Some(gateway_url);
    rec.console_url = Some(console_url.clone());
    persist_tenant_record(redis, tenant_key_prefix, &rec).await?;

    Ok(ProvisionAck {
        tenant_id: tenant_id.to_string(),
        status: rec.status,
        api_url: rec.api_url,
        gateway_url: rec.gateway_url,
        console_url: rec.console_url,
        active_deployment_count: rec.active_deployment_count,
    })
}

async fn pause_and_persist(
    redis: &redis::Client,
    tenant_key_prefix: &str,
    tenants_dir: &str,
    base_domain: &str,
    public_web_base_url: &str,
    backend: &str,
    tenant_id: &str,
    final_status: &str,
) -> Result<ActionAck> {
    if backend == "docker" {
        deprovision_docker(tenant_id, tenants_dir).await?;
    }

    let mut rec = fetch_tenant_record(redis, tenant_key_prefix, tenant_id)
        .await?
        .unwrap_or_else(|| empty_tenant_record(tenant_id, base_domain, public_web_base_url));
    rec.status = final_status.into();
    rec.desired_state = "paused".into();
    rec.deployment_mode = "single".into();
    rec.active_deployment_count = 0;
    rec.api_url = Some(tenant_gateway_url(base_domain, tenant_id));
    rec.gateway_url = Some(tenant_gateway_url(base_domain, tenant_id));
    rec.console_url = Some(tenant_console_url(base_domain, tenant_id));
    persist_tenant_record(redis, tenant_key_prefix, &rec).await?;

    Ok(ActionAck {
        tenant_id: tenant_id.to_string(),
        status: rec.status,
        desired_state: rec.desired_state,
        active_deployment_count: rec.active_deployment_count,
    })
}

async fn delete_and_persist(
    redis: &redis::Client,
    tenant_key_prefix: &str,
    tenants_dir: &str,
    base_domain: &str,
    public_web_base_url: &str,
    backend: &str,
    tenant_id: &str,
) -> Result<ActionAck> {
    if backend == "docker" {
        delete_docker(tenant_id, tenants_dir).await?;
    }

    let mut rec = fetch_tenant_record(redis, tenant_key_prefix, tenant_id)
        .await?
        .unwrap_or_else(|| empty_tenant_record(tenant_id, base_domain, public_web_base_url));
    rec.status = "deleted".into();
    rec.desired_state = "deleted".into();
    rec.deployment_mode = "single".into();
    rec.active_deployment_count = 0;
    rec.api_url = Some(tenant_gateway_url(base_domain, tenant_id));
    rec.gateway_url = Some(tenant_gateway_url(base_domain, tenant_id));
    rec.console_url = Some(tenant_console_url(base_domain, tenant_id));
    persist_tenant_record(redis, tenant_key_prefix, &rec).await?;

    Ok(ActionAck {
        tenant_id: tenant_id.to_string(),
        status: rec.status,
        desired_state: rec.desired_state,
        active_deployment_count: rec.active_deployment_count,
    })
}

async fn fetch_tenant_record(
    redis: &redis::Client,
    tenant_key_prefix: &str,
    tenant_id: &str,
) -> Result<Option<TenantRecord>> {
    let mut conn = redis.get_multiplexed_async_connection().await?;
    let key = format!("{}{}", tenant_key_prefix, tenant_id);
    let bytes: Option<Vec<u8>> = conn.get(key).await?;
    Ok(match bytes {
        Some(value) => Some(serde_json::from_slice(&value)?),
        None => None,
    })
}

async fn persist_tenant_record(
    redis: &redis::Client,
    tenant_key_prefix: &str,
    rec: &TenantRecord,
) -> Result<()> {
    let mut conn = redis.get_multiplexed_async_connection().await?;
    let key = format!("{}{}", tenant_key_prefix, rec.tenant_id);
    conn.set::<_, _, ()>(key, serde_json::to_vec(rec)?).await?;
    Ok(())
}

fn empty_tenant_record(tenant_id: &str, base_domain: &str, public_web_base_url: &str) -> TenantRecord {
    let gateway_url = tenant_gateway_url(base_domain, tenant_id);
    let console_url = tenant_console_url(base_domain, tenant_id);
    TenantRecord {
        tenant_id: tenant_id.to_string(),
        email: None,
        plan: None,
        status: "unknown".into(),
        desired_state: "active".into(),
        deployment_mode: "single".into(),
        active_deployment_count: 0,
        stripe_checkout_session_id: None,
        stripe_customer_id: None,
        stripe_subscription_id: None,
        api_url: Some(gateway_url.clone()),
        gateway_url: Some(gateway_url),
        console_url: Some(console_url),
        anon_key: None,
        service_key: None,
        dashboard_username: None,
        dashboard_password: None,
    }
}

fn tenant_gateway_url(base_domain: &str, tenant_id: &str) -> String {
    format!("http://{}.{}", tenant_id, base_domain)
}

fn tenant_console_url(base_domain: &str, tenant_id: &str) -> String {
    tenant_gateway_url(
        base_domain.trim_start_matches('.').trim_end_matches('/'),
        tenant_id,
    )
}

fn tenant_supabase_path(tenants_dir: &str, tenant_id: &str) -> PathBuf {
    PathBuf::from(tenants_dir).join(tenant_id).join("supabase")
}

fn rand_string(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

fn rand_hex(len: usize) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| {
            let idx = rng.gen_range(0..HEX.len());
            HEX[idx] as char
        })
        .collect()
}

fn sign_jwt(jwt_secret: &str, role: &str) -> Result<String> {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let exp = now + 60 * 60 * 24 * 365 * 10;
    let claims = JwtClaims {
        role,
        iss: "supabase",
        iat: now,
        exp,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_bytes()),
    )?;
    Ok(token)
}

fn tenant_slug(tenant_id: &str) -> String {
    tenant_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

fn tenant_prefix(tenant_id: &str) -> String {
    let slug = tenant_slug(tenant_id);
    let short: String = slug.chars().take(10).collect();
    format!("t{}", short)
}

fn set_env(content: &str, key: &str, value: &str) -> String {
    let mut out = String::new();
    let mut replaced = false;
    for line in content.lines() {
        if line.starts_with(&format!("{key}=")) {
            out.push_str(&format!("{key}={value}\n"));
            replaced = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !replaced {
        out.push_str(&format!("{key}={value}\n"));
    }
    out
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let target = dst.join(entry.file_name());

        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else if ty.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &target)?;
        }
    }

    Ok(())
}

async fn ensure_docker_network(name: &str) -> Result<()> {
    let status = Command::new("docker")
        .args(["network", "inspect", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .context("docker network inspect")?;

    if status.success() {
        info!("docker network '{}' exists", name);
        return Ok(());
    }

    info!("creating docker network '{}'", name);
    let status = Command::new("docker")
        .args(["network", "create", name])
        .status()
        .await
        .context("docker network create")?;

    if !status.success() {
        return Err(anyhow!("failed to create docker network {name}"));
    }
    Ok(())
}

async fn provision_docker_official_supabase(
    tenant_id: &str,
    tenants_dir: &str,
    base_domain: &str,
    docker_edge_network: &str,
    supabase_vendor_dir: &str,
    secrets: &TenantSecrets,
) -> Result<()> {
    let tenant_root = PathBuf::from(tenants_dir).join(tenant_id);
    let tenant_supabase_dir = tenant_root.join("supabase");
    tokio::fs::create_dir_all(&tenant_supabase_dir).await?;

    let vendor_volumes = PathBuf::from(supabase_vendor_dir).join("volumes");
    if !vendor_volumes.exists() {
        return Err(anyhow!(
            "SUPABASE_VENDOR_DIR missing volumes/: {}",
            vendor_volumes.display()
        ));
    }

    let tenant_volumes = tenant_supabase_dir.join("volumes");
    tokio::fs::create_dir_all(&tenant_volumes).await?;
    copy_dir_recursive(&vendor_volumes, &tenant_volumes)
        .with_context(|| format!("copy {} -> {}", vendor_volumes.display(), tenant_volumes.display()))?;

    tokio::fs::create_dir_all(tenant_volumes.join("db/data")).await?;
    tokio::fs::create_dir_all(tenant_volumes.join("storage")).await?;
    tokio::fs::create_dir_all(tenant_volumes.join("snippets")).await?;
    tokio::fs::create_dir_all(tenant_volumes.join("functions/main")).await?;

    let main_fn = tenant_volumes.join("functions/main/index.ts");
    if !main_fn.exists() {
        tokio::fs::write(
            &main_fn,
            "// SupaHost tenant default Edge Function\nDeno.serve(async (_req: Request) => new Response(JSON.stringify({ ok: true, tenant: Deno.env.get('SUPAHOST_TENANT') }), { headers: { 'content-type': 'application/json' } }));\n",
        )
        .await?;
    }

    let env_example_path = PathBuf::from(supabase_vendor_dir).join(".env.example");
    let env_example = tokio::fs::read_to_string(&env_example_path)
        .await
        .with_context(|| format!("read {}", env_example_path.display()))?;

    let public_url = format!("http://{}.{}", tenant_id, base_domain);

    let mut env_rendered = env_example;
    env_rendered = set_env(&env_rendered, "POSTGRES_PASSWORD", &secrets.postgres_password);
    env_rendered = set_env(&env_rendered, "JWT_SECRET", &secrets.jwt_secret);
    env_rendered = set_env(&env_rendered, "ANON_KEY", &secrets.anon_key);
    env_rendered = set_env(&env_rendered, "SERVICE_ROLE_KEY", &secrets.service_key);
    env_rendered = set_env(&env_rendered, "DASHBOARD_USERNAME", &secrets.dashboard_username);
    env_rendered = set_env(&env_rendered, "DASHBOARD_PASSWORD", &secrets.dashboard_password);
    env_rendered = set_env(&env_rendered, "SECRET_KEY_BASE", &secrets.secret_key_base);
    env_rendered = set_env(&env_rendered, "VAULT_ENC_KEY", &secrets.vault_enc_key);
    env_rendered = set_env(&env_rendered, "PG_META_CRYPTO_KEY", &secrets.pg_meta_crypto_key);
    env_rendered = set_env(&env_rendered, "LOGFLARE_PUBLIC_ACCESS_TOKEN", &secrets.logflare_public);
    env_rendered = set_env(&env_rendered, "LOGFLARE_PRIVATE_ACCESS_TOKEN", &secrets.logflare_private);
    env_rendered = set_env(&env_rendered, "SITE_URL", &public_url);
    env_rendered = set_env(&env_rendered, "API_EXTERNAL_URL", &public_url);
    env_rendered = set_env(&env_rendered, "SUPABASE_PUBLIC_URL", &public_url);
    env_rendered = set_env(&env_rendered, "POOLER_TENANT_ID", &tenant_slug(tenant_id));
    env_rendered = set_env(&env_rendered, "STORAGE_TENANT_ID", &tenant_slug(tenant_id));
    env_rendered = set_env(&env_rendered, "GLOBAL_S3_BUCKET", &tenant_slug(tenant_id));
    env_rendered = set_env(&env_rendered, "REGION", "local");
    env_rendered = set_env(&env_rendered, "S3_PROTOCOL_ACCESS_KEY_ID", &secrets.s3_protocol_access_key_id);
    env_rendered = set_env(&env_rendered, "S3_PROTOCOL_ACCESS_KEY_SECRET", &secrets.s3_protocol_access_key_secret);
    env_rendered.push_str(&format!("\nSUPAHOST_TENANT={}\n", tenant_id));

    tokio::fs::write(tenant_supabase_dir.join(".env"), env_rendered).await?;

    let rtid = format!("realtime-{}", tenant_prefix(tenant_id));
    let kong_path = tenant_volumes.join("api/kong.yml");
    let kong = tokio::fs::read_to_string(&kong_path)
        .await
        .with_context(|| format!("read {}", kong_path.display()))?;
    let kong = kong.replace("realtime-dev", &rtid);
    tokio::fs::write(&kong_path, kong).await?;

    let compose_src_path = PathBuf::from(supabase_vendor_dir).join("docker-compose.yml");
    let compose_src = tokio::fs::read_to_string(&compose_src_path)
        .await
        .with_context(|| format!("read {}", compose_src_path.display()))?;

    let compose_patched = patch_supabase_compose_for_tenant(
        &compose_src,
        tenant_id,
        base_domain,
        docker_edge_network,
        &rtid,
    )?;
    tokio::fs::write(tenant_supabase_dir.join("docker-compose.yml"), compose_patched).await?;

    info!("starting OFFICIAL Supabase stack via docker compose (tenant={})", tenant_id);
    let project = format!("tenant_{}", tenant_slug(tenant_id));
    let status = Command::new("docker")
        .current_dir(&tenant_supabase_dir)
        .args(["compose", "-f", "docker-compose.yml", "-p", &project, "up", "-d"])
        .status()
        .await
        .context("docker compose up")?;

    if !status.success() {
        return Err(anyhow!("docker compose up failed (tenant={tenant_id})"));
    }

    write_tenant_traefik_route(tenant_id, base_domain).await?;
    Ok(())
}

fn patch_supabase_compose_for_tenant(
    compose_yaml: &str,
    tenant_id: &str,
    base_domain: &str,
    docker_edge_network: &str,
    realtime_tenant_id: &str,
) -> Result<String> {
    let mut doc: Yaml = serde_yaml::from_str(compose_yaml).context("parse vendor docker-compose.yml")?;
    let tprefix = tenant_prefix(tenant_id);
    let tslug = tenant_slug(tenant_id);

    set_yaml_string(&mut doc, "name", &format!("tenant-{}", tslug));
    ensure_edge_network(&mut doc, docker_edge_network)?;

    let services = doc
        .as_mapping_mut()
        .and_then(|top| top.get_mut(&Yaml::String("services".into())))
        .and_then(|v| v.as_mapping_mut())
        .ok_or_else(|| anyhow!("compose missing services map"))?;


    for (svc_name, svc_val) in services.iter_mut() {
        let svc_map = svc_val
            .as_mapping_mut()
            .ok_or_else(|| anyhow!("service entry not a map"))?;
        let svc_name_str = svc_name.as_str().unwrap_or("");

        if svc_name_str == "realtime" {
            svc_map.insert(
                Yaml::String("container_name".into()),
                Yaml::String(format!("{}.supabase-realtime", realtime_tenant_id)),
            );
        } else if let Some(existing) = svc_map
            .get(&Yaml::String("container_name".into()))
            .and_then(|v| v.as_str())
        {
            svc_map.insert(
                Yaml::String("container_name".into()),
                Yaml::String(format!("{}-{}", tprefix, existing)),
            );
        } else {
            svc_map.insert(
                Yaml::String("container_name".into()),
                Yaml::String(format!("{}-{}", tprefix, svc_name_str)),
            );
        }

        if svc_name_str == "kong" || svc_name_str == "supavisor" {
            svc_map.remove(&Yaml::String("ports".into()));
        }

        if let Some(volumes) = svc_map.get_mut(&Yaml::String("volumes".into())) {
            if let Some(seq) = volumes.as_sequence_mut() {
                for item in seq.iter_mut() {
                    if let Some(s) = item.as_str() {
                        *item = Yaml::String(s.replace(":z", "").replace(":Z", ""));
                    }
                }
            }
        }

        if svc_name_str == "realtime" {
            if let Some(hc) = svc_map.get_mut(&Yaml::String("healthcheck".into())) {
                patch_realtime_healthcheck(hc, realtime_tenant_id);
            }
        }

if svc_name_str == "kong" {
    svc_map.insert(
        Yaml::String("networks".into()),
        Yaml::Sequence(vec![Yaml::String("default".into()), Yaml::String("edge".into())]),
    );
}
    }

    Ok(serde_yaml::to_string(&doc).context("serialize patched compose")?)
}

fn set_yaml_string(doc: &mut Yaml, key: &str, value: &str) {
    if let Some(map) = doc.as_mapping_mut() {
        map.insert(Yaml::String(key.to_string()), Yaml::String(value.to_string()));
    }
}

fn ensure_edge_network(doc: &mut Yaml, docker_edge_network: &str) -> Result<()> {
    let top = doc
        .as_mapping_mut()
        .ok_or_else(|| anyhow!("compose doc not a map"))?;

    if !top.contains_key(&Yaml::String("networks".into())) {
        top.insert(
            Yaml::String("networks".into()),
            Yaml::Mapping(Default::default()),
        );
    }

    let networks_map = top
        .get_mut(&Yaml::String("networks".into()))
        .and_then(|v| v.as_mapping_mut())
        .ok_or_else(|| anyhow!("networks not a map"))?;

    let mut edge_map = serde_yaml::Mapping::new();
    edge_map.insert(Yaml::String("external".into()), Yaml::Bool(true));
    edge_map.insert(
        Yaml::String("name".into()),
        Yaml::String(docker_edge_network.to_string()),
    );
    networks_map.insert(Yaml::String("edge".into()), Yaml::Mapping(edge_map));

    Ok(())
}

fn patch_realtime_healthcheck(healthcheck: &mut Yaml, realtime_tenant_id: &str) {
    if let Some(map) = healthcheck.as_mapping_mut() {
        if let Some(test) = map.get_mut(&Yaml::String("test".into())) {
            if let Some(seq) = test.as_sequence_mut() {
                for item in seq.iter_mut() {
                    if let Some(s) = item.as_str() {
                        if s.contains("realtime-dev") {
                            *item = Yaml::String(s.replace("realtime-dev", realtime_tenant_id));
                        }
                    }
                }
            }
        }
    }
}

fn traefik_dynamic_dir() -> PathBuf {
    env::var("TRAEFIK_DYNAMIC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./infra/local/traefik/dynamic"))
}

fn tenant_kong_container_name(tenant_id: &str) -> String {
    format!("{}-supabase-kong", tenant_prefix(tenant_id))
}

async fn write_tenant_traefik_route(tenant_id: &str, base_domain: &str) -> Result<()> {
    let dir = traefik_dynamic_dir();
    tokio::fs::create_dir_all(&dir).await?;
    let host = format!("{}.{}", tenant_id, base_domain);
    let router = format!("tenant-{}", tenant_slug(tenant_id));
    let service_url = format!("http://{}:8000", tenant_kong_container_name(tenant_id));
    let yaml = format!(
        "http:\n  routers:\n    {router}:\n      rule: Host(`{host}`)\n      entryPoints:\n        - web\n      service: {router}\n  services:\n    {router}:\n      loadBalancer:\n        servers:\n          - url: {service_url}\n"
    );
    tokio::fs::write(dir.join(format!("tenant-{}.yaml", tenant_slug(tenant_id))), yaml).await?;
    Ok(())
}

async fn remove_tenant_traefik_route(tenant_id: &str) -> Result<()> {
    let path = traefik_dynamic_dir().join(format!("tenant-{}.yaml", tenant_slug(tenant_id)));
    if path.exists() {
        tokio::fs::remove_file(path).await?;
    }
    Ok(())
}

async fn deprovision_docker(tenant_id: &str, tenants_dir: &str) -> Result<()> {
    let tenant_supabase_dir = tenant_supabase_path(tenants_dir, tenant_id);
    let compose_path = tenant_supabase_dir.join("docker-compose.yml");
    if !compose_path.exists() {
        return Ok(());
    }

    let project = format!("tenant_{}", tenant_slug(tenant_id));
    info!("stopping tenant stack (tenant={})", tenant_id);
    let status = Command::new("docker")
        .current_dir(&tenant_supabase_dir)
        .args([
            "compose",
            "-f",
            "docker-compose.yml",
            "-p",
            project.as_str(),
            "down",
            "--remove-orphans",
        ])
        .status()
        .await
        .context("docker compose down")?;

    if !status.success() {
        return Err(anyhow!("docker compose down failed (tenant={tenant_id})"));
    }

    remove_tenant_traefik_route(tenant_id).await?;
    Ok(())
}

async fn delete_docker(tenant_id: &str, tenants_dir: &str) -> Result<()> {
    deprovision_docker(tenant_id, tenants_dir).await?;
    let tenant_root = PathBuf::from(tenants_dir).join(tenant_id);
    if tenant_root.exists() {
        tokio::fs::remove_dir_all(&tenant_root).await?;
    }
    remove_tenant_traefik_route(tenant_id).await?;
    Ok(())
}

async fn repair_docker_official_supabase_layout(
    tenant_id: &str,
    tenants_dir: &str,
    base_domain: &str,
    docker_edge_network: &str,
    supabase_vendor_dir: &str,
) -> Result<()> {
    let tenant_supabase_dir = tenant_supabase_path(tenants_dir, tenant_id);
    if !tenant_supabase_dir.exists() {
        return Err(anyhow!(
            "tenant directory does not exist (cannot repair): {}",
            tenant_supabase_dir.display()
        ));
    }

    let vendor_volumes = PathBuf::from(supabase_vendor_dir).join("volumes");
    if !vendor_volumes.exists() {
        return Err(anyhow!(
            "SUPABASE_VENDOR_DIR missing volumes/: {}",
            vendor_volumes.display()
        ));
    }

    let tenant_volumes = tenant_supabase_dir.join("volumes");
    tokio::fs::create_dir_all(&tenant_volumes).await?;
    copy_dir_recursive(&vendor_volumes, &tenant_volumes)
        .with_context(|| format!("copy {} -> {}", vendor_volumes.display(), tenant_volumes.display()))?;

    tokio::fs::create_dir_all(tenant_volumes.join("db/data")).await?;
    tokio::fs::create_dir_all(tenant_volumes.join("storage")).await?;
    tokio::fs::create_dir_all(tenant_volumes.join("snippets")).await?;
    tokio::fs::create_dir_all(tenant_volumes.join("functions/main")).await?;

    let main_fn = tenant_volumes.join("functions/main/index.ts");
    if !main_fn.exists() {
        tokio::fs::write(
            &main_fn,
            "// SupaHost tenant default Edge Function
Deno.serve(async (_req: Request) => new Response(JSON.stringify({ ok: true, tenant: Deno.env.get('SUPAHOST_TENANT') }), { headers: { 'content-type': 'application/json' } }));
",
        )
        .await?;
    }

    let rtid = format!("realtime-{}", tenant_prefix(tenant_id));
    let kong_path = tenant_volumes.join("api/kong.yml");
    let kong = tokio::fs::read_to_string(&kong_path)
        .await
        .with_context(|| format!("read {}", kong_path.display()))?;
    let kong = if kong.contains("realtime-dev") {
        kong.replace("realtime-dev", &rtid)
    } else {
        kong
    };
    tokio::fs::write(&kong_path, kong).await?;

    let compose_src_path = PathBuf::from(supabase_vendor_dir).join("docker-compose.yml");
    let compose_src = tokio::fs::read_to_string(&compose_src_path)
        .await
        .with_context(|| format!("read {}", compose_src_path.display()))?;
    let compose_patched = patch_supabase_compose_for_tenant(
        &compose_src,
        tenant_id,
        base_domain,
        docker_edge_network,
        &rtid,
    )?;
    tokio::fs::write(tenant_supabase_dir.join("docker-compose.yml"), compose_patched).await?;
    write_tenant_traefik_route(tenant_id, base_domain).await?;

    Ok(())
}

async fn reconcile_docker(
    tenant_id: &str,
    tenants_dir: &str,
    base_domain: &str,
    docker_edge_network: &str,
    supabase_vendor_dir: &str,
) -> Result<()> {
    let tenant_supabase_dir = tenant_supabase_path(tenants_dir, tenant_id);

    if !tenant_supabase_dir.exists() {
        return Err(anyhow!(
            "tenant directory does not exist (cannot reconcile): {}",
            tenant_supabase_dir.display()
        ));
    }

    repair_docker_official_supabase_layout(
        tenant_id,
        tenants_dir,
        base_domain,
        docker_edge_network,
        supabase_vendor_dir,
    )
    .await?;

    info!("reconciling tenant via docker compose up -d (tenant={})", tenant_id);
    let project = format!("tenant_{}", tenant_slug(tenant_id));

    let status = Command::new("docker")
        .current_dir(&tenant_supabase_dir)
        .args(["compose", "-f", "docker-compose.yml", "-p", &project, "up", "-d"])
        .status()
        .await
        .context("docker compose up (reconcile)")?;

    if !status.success() {
        return Err(anyhow!("docker compose up failed (reconcile tenant={tenant_id})"));
    }
    write_tenant_traefik_route(tenant_id, base_domain).await?;
    Ok(())
}
