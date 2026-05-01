//! Liveness and readiness probes.
//!
//! `/health` is the cheap "process is alive" check Docker uses for
//! its container healthcheck. It does no I/O, so a transient DB or
//! Redis blip never restarts the container.
//!
//! `/readiness` actively verifies the dependencies an instance needs
//! to serve traffic: a usable DB connection and a reachable Redis.
//! Orchestrators that route traffic conditionally (Kubernetes
//! readiness, load balancer health probes) should hit this endpoint.
//! Returns 200 when both checks pass, 503 with a body listing the
//! failing dependencies otherwise.

use actix_web::{web, HttpResponse, Responder};
use serde_json::json;
use std::time::Duration;
use tracing::warn;

use crate::db::Pool;
use crate::utils::rate_limit::get_redis_url;

const REDIS_PING_TIMEOUT: Duration = Duration::from_secs(2);

pub async fn liveness() -> impl Responder {
    HttpResponse::Ok().body("Helpdesk API is running!")
}

pub async fn readiness(pool: web::Data<Pool>) -> HttpResponse {
    let db_ok = check_db(&pool);
    let redis_ok = check_redis().await;

    if db_ok && redis_ok {
        return HttpResponse::Ok().json(json!({
            "status": "ready",
            "checks": { "db": "ok", "redis": "ok" }
        }));
    }

    HttpResponse::ServiceUnavailable()
        .insert_header(("Retry-After", "5"))
        .json(json!({
            "status": "not_ready",
            "checks": {
                "db": if db_ok { "ok" } else { "fail" },
                "redis": if redis_ok { "ok" } else { "fail" },
            }
        }))
}

fn check_db(pool: &Pool) -> bool {
    match pool.get() {
        Ok(_conn) => true,
        Err(e) => {
            warn!(error = %e, "readiness: DB pool acquire failed");
            false
        }
    }
}

async fn check_redis() -> bool {
    let url = get_redis_url();
    let client = match redis::Client::open(url) {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "readiness: Redis client init failed");
            return false;
        }
    };

    let ping = async {
        let mut con = client.get_multiplexed_async_connection().await?;
        let _: String = redis::cmd("PING").query_async(&mut con).await?;
        Ok::<(), redis::RedisError>(())
    };

    match tokio::time::timeout(REDIS_PING_TIMEOUT, ping).await {
        Ok(Ok(())) => true,
        Ok(Err(e)) => {
            warn!(error = %e, "readiness: Redis ping failed");
            false
        }
        Err(_) => {
            warn!("readiness: Redis ping timed out");
            false
        }
    }
}
