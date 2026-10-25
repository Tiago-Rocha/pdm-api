//! Autenticação: client_id e segredo trocados por um JWT (HS256), enviado em
//! Authorization: Bearer. O segredo só existe em hash argon2 na base de dados.
//! Com login de um utilizador da app, o JWT leva também o id e o nome dele.
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::Rng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ApiError;
use crate::AppState;

pub const TOKEN_TTL_SECS: i64 = 60 * 60; // 1 hora
/// token de um utilizador: curto, renova-se com o refresh token
pub const USER_TOKEN_TTL_SECS: i64 = 15 * 60;

#[derive(Serialize, Deserialize)]
pub struct Claims {
    /// id do aluno na base de dados
    pub sub: Uuid,
    pub client_id: String,
    pub name: String,
    pub iat: i64,
    pub exp: i64,
    /// utilizador com sessão iniciada na app (só nos tokens de POST /v1/auth/login)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_name: Option<String>,
}

pub fn hash_secret(secret: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(secret.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("argon2: {e}"))?
        .to_string())
}

pub fn verify_secret(secret: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|h| Argon2::default().verify_password(secret.as_bytes(), &h).is_ok())
        .unwrap_or(false)
}

/// Texto aleatório legível: letras minúsculas e algarismos, sem 0/o/1/l.
pub fn random_token(len: usize) -> String {
    const CHARS: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..len).map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char).collect()
}

pub fn issue(secret: &[u8], sub: Uuid, client_id: &str, name: &str) -> anyhow::Result<String> {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let claims = Claims { sub, client_id: client_id.into(), name: name.into(), iat: now, exp: now + TOKEN_TTL_SECS, uid: None, user_name: None };
    Ok(encode(&Header::default(), &claims, &EncodingKey::from_secret(secret))?)
}

pub fn issue_user(secret: &[u8], sub: Uuid, client_id: &str, name: &str, uid: Uuid, user_name: &str) -> anyhow::Result<String> {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let claims = Claims {
        sub,
        client_id: client_id.into(),
        name: name.into(),
        iat: now,
        exp: now + USER_TOKEN_TTL_SECS,
        uid: Some(uid),
        user_name: Some(user_name.into()),
    };
    Ok(encode(&Header::default(), &claims, &EncodingKey::from_secret(secret))?)
}

/// Extractor: o aluno autenticado, a partir do header Authorization: Bearer <token>.
pub struct Student(pub Claims);

impl FromRequestParts<AppState> for Student {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ApiError::Unauthorized("Falta o header Authorization: Bearer <token>. Obtém o token em POST /v1/auth/token.".into()))?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or_else(|| ApiError::Unauthorized("O header Authorization tem de começar por \"Bearer \".".into()))?;
        let data = decode::<Claims>(token, &DecodingKey::from_secret(&state.jwt_secret), &Validation::default())
            .map_err(|e| match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => {
                    ApiError::Unauthorized("O token expirou. Pede um novo em POST /v1/auth/token.".into())
                }
                _ => ApiError::Unauthorized("Token inválido.".into()),
            })?;
        Ok(Student(data.claims))
    }
}

/// Extractor: um utilizador com sessão iniciada (token de POST /v1/auth/login) cuja conta ainda existe.
pub struct User {
    pub id: Uuid,
}

impl FromRequestParts<AppState> for User {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let Student(claims) = Student::from_request_parts(parts, state).await?;
        let id = claims.uid.ok_or_else(|| {
            ApiError::Forbidden(
                "Este pedido é de um utilizador: faz login em POST /v1/auth/login e usa o token que vem na resposta.".into(),
            )
        })?;
        let exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE id = $1").bind(id).fetch_optional(&state.db).await?;
        if exists.is_none() {
            return Err(ApiError::Unauthorized("Esta conta já não existe. Faz login com outra conta.".into()));
        }
        Ok(User { id })
    }
}

/// Extractor: o docente, pelo header X-Admin-Key.
pub struct Admin;

impl FromRequestParts<AppState> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        match parts.headers.get("x-admin-key").and_then(|v| v.to_str().ok()) {
            Some(k) if !state.admin_key.is_empty() && k == state.admin_key.as_str() => Ok(Admin),
            _ => Err(ApiError::Forbidden("Reservado ao docente.".into())),
        }
    }
}
