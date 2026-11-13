//! Logs das apps: a app manda linhas de log (erros, avisos, o que fez) e a API
//! passa-as ao OpenObserve, na stream `apps`, com quem as mandou.
//! A app nunca fala com o OpenObserve: as credenciais ficam no servidor.
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Map, Value};

use crate::auth::Student;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

/// Linhas por pedido.
const MAX_LINES: usize = 50;
/// Texto por campo (uma stack trace cabe).
const MAX_TEXT: usize = 8_000;
/// Campos extra por linha.
const MAX_FIELDS: usize = 30;
const LEVELS: [&str; 5] = ["debug", "info", "warn", "error", "fatal"];
/// Campos que a API escreve; a app não os pode trocar.
const RESERVED: [&str; 5] = ["_timestamp", "client_id", "name", "user_id", "user_name"];

/// POST /v1/logs: uma linha (objeto) ou várias (lista), cada uma com `level` e `message`.
pub async fn post_logs(
    State(st): State<AppState>,
    Student(c): Student,
    Json(body): Json<Value>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let lines = match body {
        Value::Array(v) => v,
        v @ Value::Object(_) => vec![v],
        _ => return Err(ApiError::BadRequest("envia um objeto ou uma lista de objetos".into())),
    };
    if lines.is_empty() || lines.len() > MAX_LINES {
        return Err(ApiError::BadRequest(format!("entre 1 e {MAX_LINES} linhas por pedido")));
    }
    let now = time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000;
    let mut records = Vec::with_capacity(lines.len());
    for (i, line) in lines.into_iter().enumerate() {
        let Value::Object(fields) = line else {
            return Err(ApiError::BadRequest(format!("linha {i}: tem de ser um objeto")));
        };
        records.push(record(i, fields, now, &c)?);
    }
    let accepted = records.len();
    if let Some(o2) = st.o2_apps.clone() {
        let http = st.http.clone();
        tokio::spawn(async move {
            match http.post(&o2.url).basic_auth(&o2.user, Some(&o2.password)).json(&records).send().await {
                Ok(r) if r.status().is_success() => {}
                Ok(r) => tracing::warn!("OpenObserve (apps) respondeu {}", r.status()),
                Err(err) => tracing::warn!("OpenObserve (apps) falhou: {err}"),
            }
        });
    }
    Ok((StatusCode::ACCEPTED, Json(json!({ "accepted": accepted }))))
}

fn record(i: usize, fields: Map<String, Value>, now: i128, c: &crate::auth::Claims) -> ApiResult<Value> {
    let level = fields.get("level").and_then(Value::as_str).unwrap_or("info").to_lowercase();
    if !LEVELS.contains(&level.as_str()) {
        return Err(ApiError::BadRequest(format!("linha {i}: level tem de ser um de {}", LEVELS.join(", "))));
    }
    match fields.get("message").and_then(Value::as_str) {
        Some(m) if !m.trim().is_empty() => {}
        _ => return Err(ApiError::BadRequest(format!("linha {i}: falta message"))),
    }
    if fields.len() > MAX_FIELDS {
        return Err(ApiError::BadRequest(format!("linha {i}: no máximo {MAX_FIELDS} campos")));
    }
    let mut out = Map::new();
    for (k, v) in fields {
        if RESERVED.contains(&k.as_str()) || k == "level" {
            continue;
        }
        let key: String = k.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(60).collect();
        if key.is_empty() {
            continue;
        }
        out.insert(key.to_lowercase(), short(v));
    }
    out.insert("level".into(), json!(level));
    out.insert("_timestamp".into(), json!(now));
    out.insert("client_id".into(), json!(c.client_id));
    out.insert("name".into(), json!(c.name));
    if let Some(uid) = c.uid {
        out.insert("user_id".into(), json!(uid));
    }
    if let Some(u) = &c.user_name {
        out.insert("user_name".into(), json!(u));
    }
    Ok(Value::Object(out))
}

/// Texto comprido é cortado; objetos e listas vão como texto JSON.
fn short(v: Value) -> Value {
    let text = match v {
        Value::String(s) => s,
        Value::Object(_) | Value::Array(_) => v.to_string(),
        other => return other,
    };
    if text.len() <= MAX_TEXT {
        return json!(text);
    }
    let mut end = MAX_TEXT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    json!(format!("{}…", &text[..end]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn claims() -> crate::auth::Claims {
        crate::auth::Claims {
            sub: Uuid::nil(),
            client_id: "pdm-ab12cd".into(),
            name: "Grupo 1".into(),
            iat: 0,
            exp: 0,
            uid: None,
            user_name: None,
        }
    }

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn a_api_escreve_quem_mandou() {
        let r = record(0, obj(json!({"level": "ERROR", "message": "x", "client_id": "outro"})), 1, &claims()).unwrap();
        assert_eq!(r["client_id"], "pdm-ab12cd");
        assert_eq!(r["level"], "error");
    }

    #[test]
    fn sem_message_falha() {
        assert!(record(0, obj(json!({"level": "info"})), 1, &claims()).is_err());
        assert!(record(0, obj(json!({"level": "grave", "message": "x"})), 1, &claims()).is_err());
    }

    #[test]
    fn texto_comprido_e_cortado() {
        let r = record(0, obj(json!({"message": "x", "stack": "é".repeat(9_000)})), 1, &claims()).unwrap();
        assert!(r["stack"].as_str().unwrap().len() <= MAX_TEXT + 3);
        assert_eq!(r["level"], "info");
    }
}
