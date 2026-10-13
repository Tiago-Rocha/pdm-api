//! Autenticação: client_id e segredo trocados por um JWT (HS256), enviado em
//! Authorization: Bearer. O segredo só existe em hash argon2 na base de dados.
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

#[derive(Serialize, Deserialize)]
pub struct Claims {
    /// id do aluno na base de dados
    pub sub: Uuid,
    pub client_id: String,
    pub name: String,
    pub iat: i64,
    pub exp: i64,
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
    let claims = Claims { sub, client_id: client_id.into(), name: name.into(), iat: now, exp: now + TOKEN_TTL_SECS };
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
