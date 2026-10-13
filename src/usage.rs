//! Registo de uso: um evento por pedido, com o aluno que o fez.
//! Vai para a tabela request_log e, se houver DD_API_KEY, para o Datadog (logs, região UE).
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::http::header;
use axum::middleware::Next;
use axum::response::Response;
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::mpsc;

use crate::auth::Claims;
use crate::AppState;

/// Quem fez o pedido, quando o handler sabe e o token não diz (POST /v1/auth/token).
#[derive(Clone)]
pub struct LoggedClient {
    pub client_id: String,
    pub name: String,
    pub event: &'static str,
}

pub struct Event {
    pub at: time::OffsetDateTime,
    pub client_id: Option<String>,
    pub name: Option<String>,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub duration: Duration,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub event: Option<&'static str>,
}

pub type Sender = mpsc::UnboundedSender<Event>;

fn header_str(req: &Request, name: &str) -> Option<String> {
    req.headers().get(name).and_then(|v| v.to_str().ok()).map(|s| s.chars().take(300).collect())
}

/// Middleware: mede cada pedido e envia um evento para o registo.
pub async fn track(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    // /health e as imagens (carregadas sem token) ficam fora do registo
    if path == "/health" || path.starts_with("/static/") || path.starts_with("/media/") {
        return next.run(req).await;
    }
    let started = Instant::now();
    let method = req.method().to_string();
    let path_q = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or(path);
    // o Caddy põe o IP verdadeiro em X-Forwarded-For
    let ip = header_str(&req, "x-forwarded-for").map(|s| s.split(',').next().unwrap_or("").trim().to_string());
    let user_agent = header_str(&req, header::USER_AGENT.as_str());
    let claims = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .and_then(|t| decode::<Claims>(t, &DecodingKey::from_secret(&st.jwt_secret), &Validation::default()).ok())
        .map(|d| d.claims);

    let res = next.run(req).await;

    let logged = res.extensions().get::<LoggedClient>().cloned();
    let (client_id, name, event) = match (logged, claims) {
        (Some(l), _) => (Some(l.client_id), Some(l.name), Some(l.event)),
        (None, Some(c)) => (Some(c.client_id), Some(c.name), None),
        (None, None) => (None, None, None),
    };
    let _ = st.usage.send(Event {
        at: time::OffsetDateTime::now_utc(),
        client_id,
        name,
        method,
        path: path_q.chars().take(500).collect(),
        status: res.status().as_u16(),
        duration: started.elapsed(),
        ip,
        user_agent,
        event,
    });
    res
}

/// Tarefa de fundo: junta os eventos em lotes e grava-os na base de dados e no Datadog.
pub fn spawn(db: PgPool, http: reqwest::Client, dd_api_key: Option<String>, dd_site: String) -> Sender {
    let (tx, mut rx) = mpsc::unbounded_channel::<Event>();
    tokio::spawn(async move {
        let host = std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()).unwrap_or_else(|_| "pdm-api".into());
        loop {
            let Some(first) = rx.recv().await else { break };
            let mut batch = vec![first];
            // espera um pouco para juntar mais eventos no mesmo lote
            let deadline = tokio::time::sleep(Duration::from_secs(3));
            tokio::pin!(deadline);
            while batch.len() < 200 {
                tokio::select! {
                    ev = rx.recv() => match ev { Some(e) => batch.push(e), None => break },
                    _ = &mut deadline => break,
                }
            }
            save(&db, &batch).await;
            if let Some(key) = &dd_api_key {
                send_datadog(&http, key, &dd_site, &host, &batch).await;
            }
        }
    });
    tx
}

async fn save(db: &PgPool, batch: &[Event]) {
    for e in batch {
        let r = sqlx::query(
            "INSERT INTO request_log (at, client_id, name, method, path, status, duration_ms, ip, user_agent, event)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(e.at)
        .bind(&e.client_id)
        .bind(&e.name)
        .bind(&e.method)
        .bind(&e.path)
        .bind(e.status as i32)
        .bind(e.duration.as_millis() as i32)
        .bind(&e.ip)
        .bind(&e.user_agent)
        .bind(e.event)
        .execute(db)
        .await;
        if let Err(err) = r {
            tracing::warn!("request_log: {err}");
        }
    }
}

async fn send_datadog(http: &reqwest::Client, key: &str, site: &str, host: &str, batch: &[Event]) {
    let logs: Vec<_> = batch
        .iter()
        .map(|e| {
            let who = e.client_id.as_deref().unwrap_or("anonimo");
            let mut tags = vec!["env:prod".to_string(), format!("client_id:{who}")];
            if let Some(ev) = e.event {
                tags.push(format!("event:{ev}"));
            }
            json!({
                "ddsource": "rust",
                "service": "pdm-api",
                "hostname": host,
                "ddtags": tags.join(","),
                "status": if e.status >= 500 { "error" } else if e.status >= 400 { "warn" } else { "info" },
                "message": format!("{} {} {} {}ms {}", e.method, e.path, e.status, e.duration.as_millis(),
                                   e.name.as_deref().unwrap_or("(sem token)")),
                "date": e.at.unix_timestamp() * 1000,
                "duration": e.duration.as_nanos() as u64,
                "event": e.event,
                "usr": { "id": e.client_id, "name": e.name },
                "http": { "method": e.method, "url": e.path, "status_code": e.status, "useragent": e.user_agent },
                "network": { "client": { "ip": e.ip } },
            })
        })
        .collect();
    let url = format!("https://http-intake.logs.{site}/api/v2/logs");
    match http.post(url).header("DD-API-KEY", key).json(&logs).send().await {
        Ok(r) if r.status().is_success() => {}
        Ok(r) => tracing::warn!("Datadog respondeu {}", r.status()),
        Err(err) => tracing::warn!("Datadog falhou: {err}"),
    }
}
