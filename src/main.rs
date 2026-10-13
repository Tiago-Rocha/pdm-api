//! API Tempo Açores da UC de PDM (CTeSP DAW, ESTA, Universidade dos Açores, 2026/27).
//!
//! Ilhas, trilhos e a previsão real do IPMA para a app dos alunos, e um feed de
//! relatos (texto e fotografia) para exercitarem POST, PUT e DELETE.
mod auth;
mod data;
mod error;
mod ipma;
mod routes;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::extract::DefaultBodyLimit;
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

/// Fotografias até 8 MB (são reduzidas a 1280 px ao gravar).
const MAX_UPLOAD: usize = 8 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub http: reqwest::Client,
    pub ipma: Arc<ipma::Cache>,
    pub jwt_secret: Arc<Vec<u8>>,
    pub admin_key: Arc<String>,
    pub media_dir: Arc<PathBuf>,
    /// endereço público, sem barra no fim, para os URLs das imagens
    pub public_url: Arc<String>,
}

const OPENAPI: &str = include_str!("openapi.json");
const DOCS: &str = include_str!("docs.html");

async fn openapi() -> impl IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "application/json")], OPENAPI)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "pdm_api=info,tower_http=info".into()))
        .init();

    let var = |k: &str| std::env::var(k).with_context(|| format!("falta {k}"));
    let db = PgPoolOptions::new().max_connections(8).connect(&var("DATABASE_URL")?).await.context("ligar ao Postgres")?;
    sqlx::migrate!().run(&db).await.context("migrações")?;

    let media_dir = PathBuf::from(std::env::var("MEDIA_DIR").unwrap_or_else(|_| "media".into()));
    tokio::fs::create_dir_all(&media_dir).await.context("criar MEDIA_DIR")?;
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "static".into());

    let state = AppState {
        db,
        http: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .user_agent("pdm-api (UC de PDM, Universidade dos Açores)")
            .build()?,
        ipma: Arc::new(ipma::Cache::default()),
        jwt_secret: Arc::new(var("JWT_SECRET")?.into_bytes()),
        admin_key: Arc::new(var("ADMIN_KEY")?),
        media_dir: Arc::new(media_dir.clone()),
        public_url: Arc::new(var("PUBLIC_URL")?.trim_end_matches('/').to_string()),
    };

    let v1 = Router::new()
        .route("/auth/token", post(routes::token))
        .route("/me", get(routes::me))
        .route("/islands", get(routes::islands))
        .route("/islands/{id}", get(routes::island))
        .route("/islands/{id}/forecast", get(routes::forecast))
        .route("/trails", get(routes::trails))
        .route("/trails/{id}", get(routes::trail))
        .route("/reports", get(routes::list_reports).post(routes::create_report))
        .route("/reports/{id}", get(routes::get_report).put(routes::update_report).delete(routes::delete_report))
        .route("/reports/{id}/photo", put(routes::put_photo));

    let admin = Router::new()
        .route("/students", get(routes::admin_students).post(routes::admin_create_student))
        .route("/students/{client_id}/reset-secret", post(routes::admin_reset_secret))
        .route("/reports/{id}", delete(routes::admin_delete_report));

    let app = Router::new()
        .route("/", get(|| async { Redirect::temporary("/docs") }))
        .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
        .route("/openapi.json", get(openapi))
        .route("/docs", get(|| async { Html(DOCS) }))
        .nest("/v1", v1)
        .nest("/admin", admin)
        .nest_service("/static", ServeDir::new(static_dir))
        .nest_service("/media", ServeDir::new(media_dir))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD))
        // a app Flutter no Chrome corre noutra origem (localhost): é preciso CORS
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = std::env::var("BIND").unwrap_or_else(|_| "127.0.0.1:3002".into());
    let listener = tokio::net::TcpListener::bind(&addr).await.with_context(|| format!("bind {addr}"))?;
    tracing::info!("pdm-api a ouvir em {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
