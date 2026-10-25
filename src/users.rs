//! Contas de utilizador dentro da app de cada grupo. A app identifica-se com o seu
//! clientId e clientSecret; cada pessoa tem a sua conta (email e password) nessa app.
//! O login devolve um access token curto e um refresh token de 30 dias.
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::auth::{self, User};
use crate::error::{ApiError, ApiResult};
use crate::usage::LoggedClient;
use crate::AppState;

const REFRESH_DAYS: i64 = 30;
const MIN_PASSWORD: usize = 8;

/// A app de um grupo, já com o clientSecret verificado.
struct App {
    id: Uuid,
    client_id: String,
    name: String,
}

async fn app(st: &AppState, client_id: &str, secret: &str) -> ApiResult<App> {
    let client_id = client_id.trim();
    let row: Option<(Uuid, String, String)> = sqlx::query_as("SELECT id, secret_hash, name FROM students WHERE client_id = $1")
        .bind(client_id)
        .fetch_optional(&st.db)
        .await?;
    match row {
        Some((id, hash, name)) if auth::verify_secret(secret.trim(), &hash) => Ok(App { id, client_id: client_id.into(), name }),
        _ => Err(ApiError::Unauthorized("clientId ou clientSecret errados.".into())),
    }
}

fn clean_email(email: &str) -> ApiResult<String> {
    let e = email.trim().to_lowercase();
    let ok = e.chars().count() <= 200
        && !e.contains(char::is_whitespace)
        && e.split_once('@').is_some_and(|(a, b)| !a.is_empty() && b.contains('.') && !b.starts_with('.') && !b.ends_with('.'));
    if !ok {
        return Err(ApiError::BadRequest("O email não é válido.".into()));
    }
    Ok(e)
}

fn clean_name(name: &str) -> ApiResult<String> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 60 {
        return Err(ApiError::BadRequest("O nome tem de ter entre 1 e 60 caracteres.".into()));
    }
    Ok(n.to_string())
}

fn check_password(p: &str) -> ApiResult<()> {
    let n = p.chars().count();
    if !(MIN_PASSWORD..=200).contains(&n) {
        return Err(ApiError::BadRequest(format!("A password tem de ter pelo menos {MIN_PASSWORD} caracteres.")));
    }
    Ok(())
}

/// Cria um refresh token e devolve a resposta de login: tokens e o utilizador.
async fn session(st: &AppState, app: &App, uid: Uuid, email: &str, name: &str) -> ApiResult<Value> {
    let rid = Uuid::new_v4();
    let secret = auth::random_token(32);
    sqlx::query("INSERT INTO refresh_tokens (id, user_id, token_hash, expires_at) VALUES ($1, $2, $3, $4)")
        .bind(rid)
        .bind(uid)
        .bind(auth::hash_secret(&secret)?)
        .bind(OffsetDateTime::now_utc() + time::Duration::days(REFRESH_DAYS))
        .execute(&st.db)
        .await?;
    let access = auth::issue_user(&st.jwt_secret, app.id, &app.client_id, &app.name, uid, name)?;
    Ok(json!({
        "accessToken": access,
        "tokenType": "Bearer",
        "expiresIn": auth::USER_TOKEN_TTL_SECS,
        "refreshToken": format!("{rid}.{secret}"),
        "refreshExpiresIn": REFRESH_DAYS * 24 * 60 * 60,
        "user": { "id": uid, "email": email, "name": name },
    }))
}

fn logged(res: impl IntoResponse, app: &str, name: &str, event: &'static str) -> Response {
    let mut res = res.into_response();
    res.extensions_mut().insert(LoggedClient { client_id: app.into(), name: name.into(), event });
    res
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Register {
    #[serde(alias = "client_id")]
    client_id: String,
    #[serde(alias = "client_secret")]
    client_secret: String,
    email: String,
    password: String,
    name: String,
}

/// POST /v1/auth/register: cria uma conta na app e já devolve a sessão.
pub async fn register(State(st): State<AppState>, body: Option<Json<Register>>) -> ApiResult<Response> {
    let Json(r) = body.ok_or_else(|| {
        ApiError::BadRequest("Envia JSON com clientId, clientSecret, email, password e name, e o header Content-Type: application/json.".into())
    })?;
    let app = app(&st, &r.client_id, &r.client_secret).await?;
    let email = clean_email(&r.email)?;
    let name = clean_name(&r.name)?;
    check_password(&r.password)?;
    let uid = Uuid::new_v4();
    let done = sqlx::query("INSERT INTO users (id, app_id, email, name, password_hash, last_seen) VALUES ($1, $2, $3, $4, $5, now())")
        .bind(uid)
        .bind(app.id)
        .bind(&email)
        .bind(&name)
        .bind(auth::hash_secret(&r.password)?)
        .execute(&st.db)
        .await;
    if let Err(e) = done {
        if e.as_database_error().and_then(|d| d.code()).as_deref() == Some("23505") {
            return Err(ApiError::Conflict("Já há uma conta com esse email nesta app. Faz login.".into()));
        }
        return Err(e.into());
    }
    let s = session(&st, &app, uid, &email, &name).await?;
    Ok(logged((StatusCode::CREATED, Json(s)), &app.client_id, &app.name, "user_register"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Login {
    #[serde(alias = "client_id")]
    client_id: String,
    #[serde(alias = "client_secret")]
    client_secret: String,
    email: String,
    password: String,
}

/// POST /v1/auth/login: email e password de uma conta da app.
pub async fn login(State(st): State<AppState>, body: Option<Json<Login>>) -> ApiResult<Response> {
    let Json(l) = body.ok_or_else(|| {
        ApiError::BadRequest("Envia JSON com clientId, clientSecret, email e password, e o header Content-Type: application/json.".into())
    })?;
    let app = app(&st, &l.client_id, &l.client_secret).await?;
    let email = l.email.trim().to_lowercase();
    let row: Option<(Uuid, String, String)> =
        sqlx::query_as("SELECT id, name, password_hash FROM users WHERE app_id = $1 AND email = $2")
            .bind(app.id)
            .bind(&email)
            .fetch_optional(&st.db)
            .await?;
    let Some((uid, name, _)) = row.filter(|(_, _, hash)| auth::verify_secret(&l.password, hash)) else {
        let err = ApiError::Unauthorized("Email ou password errados.".into());
        return Ok(logged(err, &app.client_id, &app.name, "user_login_failed"));
    };
    sqlx::query("UPDATE users SET last_seen = now() WHERE id = $1").bind(uid).execute(&st.db).await?;
    let s = session(&st, &app, uid, &email, &name).await?;
    Ok(logged(Json(s), &app.client_id, &app.name, "user_login"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshRequest {
    #[serde(alias = "refresh_token")]
    refresh_token: String,
}

/// Lê "id.segredo" e devolve o id do refresh token, se o segredo bater certo e o token ainda valer.
async fn check_refresh(st: &AppState, token: &str) -> ApiResult<(Uuid, Uuid)> {
    let ended = || ApiError::Unauthorized("Esta sessão terminou. Faz login outra vez.".into());
    let (rid, secret) = token.trim().split_once('.').ok_or_else(ended)?;
    let rid = Uuid::parse_str(rid).map_err(|_| ended())?;
    let row: Option<(Uuid, String, OffsetDateTime, Option<OffsetDateTime>)> =
        sqlx::query_as("SELECT user_id, token_hash, expires_at, revoked_at FROM refresh_tokens WHERE id = $1")
            .bind(rid)
            .fetch_optional(&st.db)
            .await?;
    match row {
        Some((uid, hash, exp, None)) if exp > OffsetDateTime::now_utc() && auth::verify_secret(secret, &hash) => Ok((rid, uid)),
        _ => Err(ended()),
    }
}

/// POST /v1/auth/refresh: um access token novo. O refresh token fica o mesmo até ao logout ou aos 30 dias.
pub async fn refresh(State(st): State<AppState>, body: Option<Json<RefreshRequest>>) -> ApiResult<Json<Value>> {
    let Json(r) = body.ok_or_else(|| ApiError::BadRequest("Envia JSON com refreshToken.".into()))?;
    let (_, uid) = check_refresh(&st, &r.refresh_token).await?;
    let (email, name, app_id, client_id, app_name): (String, String, Uuid, String, String) = sqlx::query_as(
        "SELECT u.email, u.name, s.id, s.client_id, s.name FROM users u JOIN students s ON s.id = u.app_id WHERE u.id = $1",
    )
    .bind(uid)
    .fetch_one(&st.db)
    .await?;
    sqlx::query("UPDATE users SET last_seen = now() WHERE id = $1").bind(uid).execute(&st.db).await?;
    let access = auth::issue_user(&st.jwt_secret, app_id, &client_id, &app_name, uid, &name)?;
    Ok(Json(json!({
        "accessToken": access,
        "tokenType": "Bearer",
        "expiresIn": auth::USER_TOKEN_TTL_SECS,
        "refreshToken": r.refresh_token.trim(),
        "user": { "id": uid, "email": email, "name": name },
    })))
}

/// POST /v1/auth/logout: o refresh token deixa de valer. Repetir não faz mal.
pub async fn logout(State(st): State<AppState>, body: Option<Json<RefreshRequest>>) -> ApiResult<StatusCode> {
    let Json(r) = body.ok_or_else(|| ApiError::BadRequest("Envia JSON com refreshToken.".into()))?;
    if let Ok((rid, _)) = check_refresh(&st, &r.refresh_token).await {
        sqlx::query("UPDATE refresh_tokens SET revoked_at = now() WHERE id = $1").bind(rid).execute(&st.db).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /v1/me: apaga a conta do utilizador, os favoritos, as sessões e os relatos dele.
pub async fn delete_account(State(st): State<AppState>, u: User) -> ApiResult<StatusCode> {
    let photos: Vec<(Option<String>,)> = sqlx::query_as("SELECT photo FROM reports WHERE user_id = $1").bind(u.id).fetch_all(&st.db).await?;
    sqlx::query("DELETE FROM users WHERE id = $1").bind(u.id).execute(&st.db).await?;
    for (p,) in photos {
        if let Some(p) = p {
            let _ = tokio::fs::remove_file(st.media_dir.join(p)).await;
        }
    }
    Ok(StatusCode::NO_CONTENT)
}
