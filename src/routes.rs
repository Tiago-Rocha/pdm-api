//! Rotas da API v1.
use axum::extract::{FromRequest, Multipart, Path, Query, Request, State};
use axum::http::{header, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::auth::{self, Admin, Claims, Student, User};
use crate::data::{self, Difficulty, ISLANDS, TRAILS};
use crate::error::{ApiError, ApiResult};
use crate::AppState;

const MAX_TEXT: usize = 500;
const MAX_PHOTO_SIDE: u32 = 1280;

// ---------- autenticação ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenRequest {
    #[serde(alias = "client_id")]
    client_id: String,
    #[serde(alias = "client_secret")]
    client_secret: String,
}

pub async fn token(State(st): State<AppState>, body: Option<Json<TokenRequest>>) -> ApiResult<axum::response::Response> {
    let Json(req) = body.ok_or_else(|| {
        ApiError::BadRequest("Envia JSON com clientId e clientSecret, e o header Content-Type: application/json.".into())
    })?;
    let client_id = req.client_id.trim().to_string();
    let row: Option<(Uuid, String, String)> =
        sqlx::query_as("SELECT id, secret_hash, name FROM students WHERE client_id = $1")
            .bind(&client_id)
            .fetch_optional(&st.db)
            .await?;
    let ok = row.as_ref().is_some_and(|(_, hash, _)| auth::verify_secret(req.client_secret.trim(), hash));
    let name = row.as_ref().map(|(_, _, n)| n.clone()).unwrap_or_default();
    let Some((id, _, _)) = row.filter(|_| ok) else {
        // a tentativa falhada fica registada com o clientId que foi tentado
        let mut res = axum::response::IntoResponse::into_response(ApiError::Unauthorized("clientId ou clientSecret errados.".into()));
        res.extensions_mut().insert(crate::usage::LoggedClient { client_id, name, event: "login_failed" });
        return Ok(res);
    };
    sqlx::query("UPDATE students SET last_seen = now() WHERE id = $1").bind(id).execute(&st.db).await?;
    let token = auth::issue(&st.jwt_secret, id, &client_id, &name)?;
    let mut res = axum::response::IntoResponse::into_response(Json(
        json!({ "accessToken": token, "tokenType": "Bearer", "expiresIn": auth::TOKEN_TTL_SECS }),
    ));
    res.extensions_mut().insert(crate::usage::LoggedClient { client_id, name, event: "login" });
    Ok(res)
}

pub async fn me(State(st): State<AppState>, Student(c): Student) -> ApiResult<Json<Value>> {
    let user = match c.uid {
        Some(uid) => sqlx::query_as::<_, (String, String)>("SELECT email, name FROM users WHERE id = $1")
            .bind(uid)
            .fetch_optional(&st.db)
            .await?
            .map(|(email, name)| json!({ "id": uid, "email": email, "name": name })),
        None => None,
    };
    Ok(Json(json!({ "clientId": c.client_id, "name": c.name, "tokenExpiresAt": c.exp, "user": user })))
}

// ---------- ilhas, trilhos, previsão ----------

fn island_json(st: &AppState, i: &data::Island) -> Value {
    json!({
        "id": i.id,
        "name": i.name,
        "imageUrl": format!("{}/static/islands/{}", st.public_url, i.image),
        "trailCount": TRAILS.iter().filter(|t| t.island_id == i.id).count(),
        "latitude": i.lat,
        "longitude": i.lon,
    })
}

fn trail_json(st: &AppState, t: &data::Trail) -> Value {
    let mut v = trail_json_ok(st, t);
    if bugs_on(st) {
        match t.id {
            "capelinhos" => {
                v.as_object_mut().map(|o| o.remove("islandId"));
            }
            "lagoa-do-fogo" => v["distanceKm"] = json!("10,5 km"),
            _ => {}
        }
    }
    v
}

fn trail_json_ok(st: &AppState, t: &data::Trail) -> Value {
    let island = data::island(t.island_id).expect("trilho com ilha válida");
    json!({
        "id": t.id,
        "name": t.name,
        "islandId": t.island_id,
        "islandName": island.name,
        "imageUrl": format!("{}/static/islands/{}", st.public_url, island.image),
        "distanceKm": t.distance_km,
        "duration": t.duration,
        "difficulty": t.difficulty,
        "difficultyLabel": match t.difficulty { Difficulty::Easy => "Fácil", Difficulty::Medium => "Média", Difficulty::Hard => "Difícil" },
        "description": t.description,
    })
}

fn find_island(id: &str) -> ApiResult<&'static data::Island> {
    data::island(id).ok_or_else(|| ApiError::NotFound(format!("Não há nenhuma ilha com o id \"{id}\".")))
}

fn bugs_on(st: &AppState) -> bool {
    st.bugs.load(std::sync::atomic::Ordering::Relaxed)
}

pub async fn islands(State(st): State<AppState>, _s: Student) -> Json<Value> {
    let bugs = bugs_on(&st);
    Json(Value::Array(
        ISLANDS
            .iter()
            .filter(|i| !(bugs && i.id == "graciosa"))
            .map(|i| island_json(&st, i))
            .collect(),
    ))
}

pub async fn island(State(st): State<AppState>, _s: Student, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    Ok(Json(island_json(&st, find_island(&id)?)))
}

pub async fn forecast(State(st): State<AppState>, _s: Student, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let i = find_island(&id)?;
    let f = st.ipma.forecast(&st.http, i.ipma_id).await?;
    let mut days = json!(f.days);
    if bugs_on(&st) && i.id == "pico" {
        if let Some(day) = days.get_mut(2) {
            day["weatherType"]["category"] = json!("fog");
        }
    }
    Ok(Json(json!({
        "islandId": i.id,
        "islandName": i.name,
        "location": i.ipma_local,
        "source": "IPMA",
        "updatedAt": f.updated_at,
        "days": days,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IslandFilter {
    island_id: Option<String>,
    limit: Option<i64>,
}

pub async fn trails(State(st): State<AppState>, _s: Student, Query(q): Query<IslandFilter>) -> ApiResult<Json<Value>> {
    if let Some(id) = &q.island_id {
        find_island(id)?;
    }
    Ok(Json(Value::Array(
        TRAILS
            .iter()
            .filter(|t| q.island_id.as_deref().is_none_or(|id| t.island_id == id))
            .map(|t| trail_json(&st, t))
            .collect(),
    )))
}

pub async fn trail(State(st): State<AppState>, _s: Student, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    TRAILS
        .iter()
        .find(|t| t.id == id)
        .map(|t| Json(trail_json(&st, t)))
        .ok_or_else(|| ApiError::NotFound(format!("Não há nenhum trilho com o id \"{id}\".")))
}

// ---------- avisos, mar, sismos e estações ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WarningFilter {
    island_id: Option<String>,
}

pub async fn warnings(State(st): State<AppState>, _s: Student, Query(q): Query<WarningFilter>) -> ApiResult<Json<Value>> {
    let island = q.island_id.as_deref().map(find_island).transpose()?;
    let all = st.feeds.warnings(&st.http).await?;
    Ok(Json(json!(
        all.iter()
            .filter(|w| island.is_none_or(|i| w.area == i.warning_area))
            .collect::<Vec<_>>()
    )))
}

pub async fn sea(State(st): State<AppState>, _s: Student, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let i = find_island(&id)?;
    let s = st.feeds.sea(&st.http, i.sea_id).await?;
    Ok(Json(json!({
        "islandId": i.id,
        "islandName": i.name,
        "location": i.sea_local,
        "source": "IPMA",
        "updatedAt": s.updated_at,
        "days": s.days,
    })))
}

pub async fn observations(State(st): State<AppState>, _s: Student, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let i = find_island(&id)?;
    let o = st.feeds.observations(&st.http, i.id).await?;
    Ok(Json(json!({
        "islandId": i.id,
        "islandName": i.name,
        "source": "IPMA",
        "observedAt": o.observed_at,
        "stations": o.stations,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuakeFilter {
    island_id: Option<String>,
    min_magnitude: Option<f64>,
    felt: Option<bool>,
    limit: Option<usize>,
}

pub async fn earthquakes(State(st): State<AppState>, _s: Student, Query(q): Query<QuakeFilter>) -> ApiResult<Json<Value>> {
    if let Some(id) = &q.island_id {
        find_island(id)?;
    }
    let all = st.feeds.earthquakes(&st.http).await?;
    Ok(Json(json!(
        all.iter()
            .filter(|e| q.island_id.as_deref().is_none_or(|id| e.nearest_island_id == id))
            .filter(|e| q.min_magnitude.is_none_or(|m| e.magnitude >= m))
            .filter(|e| q.felt.is_none_or(|f| e.felt == f))
            .take(q.limit.unwrap_or(50).clamp(1, 100))
            .collect::<Vec<_>>()
    )))
}

// ---------- favoritos ----------

/// "islands" ou "trails" no caminho; o id tem de existir.
fn favorite_target(kind: &str, id: &str) -> ApiResult<&'static str> {
    match kind {
        "islands" => find_island(id).map(|_| "island"),
        "trails" => TRAILS
            .iter()
            .any(|t| t.id == id)
            .then_some("trail")
            .ok_or_else(|| ApiError::NotFound(format!("Não há nenhum trilho com o id \"{id}\"."))),
        _ => Err(ApiError::NotFound(format!(
            "Os favoritos são islands ou trails, não \"{kind}\". Exemplo: PUT /v1/me/favorites/islands/pico."
        ))),
    }
}

/// GET /v1/me/favorites: as ilhas e os trilhos favoritos do utilizador, os mais recentes primeiro.
pub async fn favorites(State(st): State<AppState>, u: User) -> ApiResult<Json<Value>> {
    let rows: Vec<(String, String, OffsetDateTime)> =
        sqlx::query_as("SELECT kind, item_id, created_at FROM favorites WHERE user_id = $1 ORDER BY created_at DESC")
            .bind(u.id)
            .fetch_all(&st.db)
            .await?;
    let (mut islands, mut trails) = (vec![], vec![]);
    for (kind, id, at) in rows {
        // um id que deixou de existir nos dados fixos fica de fora
        let item = match kind.as_str() {
            "island" => data::island(&id).map(|i| island_json(&st, i)),
            _ => TRAILS.iter().find(|t| t.id == id).map(|t| trail_json_ok(&st, t)),
        };
        if let Some(mut v) = item {
            v["addedAt"] = json!(ts(at));
            if kind == "island" { islands.push(v) } else { trails.push(v) }
        }
    }
    Ok(Json(json!({ "islands": islands, "trails": trails })))
}

/// PUT /v1/me/favorites/{islands|trails}/{id}: marca como favorito. Repetir não faz mal.
pub async fn add_favorite(
    State(st): State<AppState>,
    u: User,
    Path((kind, id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let kind = favorite_target(&kind, &id)?;
    sqlx::query("INSERT INTO favorites (user_id, kind, item_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
        .bind(u.id)
        .bind(kind)
        .bind(&id)
        .execute(&st.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /v1/me/favorites/{islands|trails}/{id}: tira dos favoritos. Repetir não faz mal.
pub async fn remove_favorite(
    State(st): State<AppState>,
    u: User,
    Path((kind, id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let kind = favorite_target(&kind, &id)?;
    sqlx::query("DELETE FROM favorites WHERE user_id = $1 AND kind = $2 AND item_id = $3")
        .bind(u.id)
        .bind(kind)
        .bind(&id)
        .execute(&st.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- relatos ----------

#[derive(sqlx::FromRow)]
struct ReportRow {
    id: Uuid,
    student_id: Uuid,
    island_id: String,
    text: String,
    photo: Option<String>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    client_id: String,
    name: String,
    user_id: Option<Uuid>,
    user_name: Option<String>,
}

const REPORT_SELECT: &str = "SELECT r.id, r.student_id, r.island_id, r.text, r.photo, r.created_at, r.updated_at, s.client_id, s.name,
            r.user_id, u.name AS user_name
     FROM reports r JOIN students s ON s.id = r.student_id LEFT JOIN users u ON u.id = r.user_id";

fn ts(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

/// Com um utilizador no token, são dele os relatos que escreveu; só com a app, os que a app escreveu sem utilizador.
fn is_mine(r: &ReportRow, c: &Claims) -> bool {
    match c.uid {
        Some(uid) => r.user_id == Some(uid),
        None => r.student_id == c.sub && r.user_id.is_none(),
    }
}

fn report_json(st: &AppState, r: &ReportRow, me: &Claims) -> Value {
    json!({
        "id": r.id,
        "islandId": r.island_id,
        "islandName": data::island(&r.island_id).map(|i| i.name).unwrap_or(""),
        "text": r.text,
        "photoUrl": r.photo.as_ref().map(|p| format!("{}/media/{p}", st.public_url)),
        "author": { "clientId": r.client_id, "name": r.name, "userName": r.user_name },
        "mine": is_mine(r, me),
        "createdAt": ts(r.created_at),
        "updatedAt": ts(r.updated_at),
    })
}

async fn load_report(st: &AppState, id: Uuid) -> ApiResult<ReportRow> {
    sqlx::query_as::<_, ReportRow>(&format!("{REPORT_SELECT} WHERE r.id = $1"))
        .bind(id)
        .fetch_optional(&st.db)
        .await?
        .ok_or_else(|| ApiError::NotFound("Esse relato não existe.".into()))
}

fn parse_id(id: &str) -> ApiResult<Uuid> {
    Uuid::parse_str(id).map_err(|_| ApiError::NotFound("Esse relato não existe.".into()))
}

fn clean_text(text: &str) -> ApiResult<String> {
    let t = text.trim();
    if t.is_empty() {
        return Err(ApiError::BadRequest("O texto do relato não pode estar vazio.".into()));
    }
    if t.chars().count() > MAX_TEXT {
        return Err(ApiError::BadRequest(format!("O texto tem no máximo {MAX_TEXT} caracteres.")));
    }
    Ok(t.to_string())
}

/// Lê, valida e grava uma fotografia: redimensionada a 1280 px no lado maior, em JPEG.
async fn save_photo(st: &AppState, bytes: &[u8]) -> ApiResult<String> {
    let img = image::load_from_memory(bytes)
        .map_err(|_| ApiError::BadRequest("A fotografia tem de ser JPEG, PNG ou WebP.".into()))?;
    let img = if img.width() > MAX_PHOTO_SIDE || img.height() > MAX_PHOTO_SIDE {
        img.resize(MAX_PHOTO_SIDE, MAX_PHOTO_SIDE, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let name = format!("{}.jpg", Uuid::new_v4());
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82)
        .encode_image(&img.to_rgb8())
        .map_err(|e| ApiError::Internal(e.into()))?;
    tokio::fs::write(st.media_dir.join(&name), out).await.map_err(|e| ApiError::Internal(e.into()))?;
    Ok(name)
}

async fn remove_photo(st: &AppState, photo: &Option<String>) {
    if let Some(p) = photo {
        let _ = tokio::fs::remove_file(st.media_dir.join(p)).await;
    }
}

pub async fn list_reports(
    State(st): State<AppState>,
    Student(c): Student,
    Query(q): Query<IslandFilter>,
) -> ApiResult<Json<Value>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 100);
    let rows: Vec<ReportRow> = match &q.island_id {
        Some(id) => {
            find_island(id)?;
            sqlx::query_as(&format!("{REPORT_SELECT} WHERE r.island_id = $1 ORDER BY r.created_at DESC LIMIT $2"))
                .bind(id)
                .bind(limit)
                .fetch_all(&st.db)
                .await?
        }
        None => {
            sqlx::query_as(&format!("{REPORT_SELECT} ORDER BY r.created_at DESC LIMIT $1"))
                .bind(limit)
                .fetch_all(&st.db)
                .await?
        }
    };
    Ok(Json(Value::Array(rows.iter().map(|r| report_json(&st, r, &c)).collect())))
}

pub async fn get_report(State(st): State<AppState>, Student(c): Student, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let r = load_report(&st, parse_id(&id)?).await?;
    Ok(Json(report_json(&st, &r, &c)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewReport {
    #[serde(alias = "island_id")]
    island_id: String,
    text: String,
}

/// POST /v1/reports: JSON {islandId, text}, ou multipart/form-data com islandId, text e photo (opcional).
pub async fn create_report(State(st): State<AppState>, Student(c): Student, req: Request) -> ApiResult<(StatusCode, Json<Value>)> {
    let ctype = req.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let (island_id, text, photo_bytes) = if ctype.starts_with("multipart/form-data") {
        let mut mp = Multipart::from_request(req, &st).await.map_err(|e| ApiError::BadRequest(e.body_text()))?;
        let (mut island, mut text, mut photo) = (None, None, None);
        while let Some(field) = mp.next_field().await.map_err(|e| ApiError::BadRequest(e.body_text()))? {
            match field.name().unwrap_or("") {
                "islandId" | "island_id" => island = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.body_text()))?),
                "text" => text = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.body_text()))?),
                "photo" => photo = Some(field.bytes().await.map_err(|e| ApiError::BadRequest(e.body_text()))?.to_vec()),
                _ => {}
            }
        }
        (
            island.ok_or_else(|| ApiError::BadRequest("Falta o campo islandId.".into()))?,
            text.ok_or_else(|| ApiError::BadRequest("Falta o campo text.".into()))?,
            photo.filter(|p| !p.is_empty()),
        )
    } else if ctype.starts_with("application/json") {
        let Json(r) = Json::<NewReport>::from_request(req, &st)
            .await
            .map_err(|e| ApiError::BadRequest(format!("JSON inválido: {}", e.body_text())))?;
        (r.island_id, r.text, None)
    } else {
        return Err(ApiError::BadRequest(
            "Envia application/json com islandId e text, ou multipart/form-data com islandId, text e photo.".into(),
        ));
    };
    find_island(&island_id)?;
    let text = clean_text(&text)?;
    let photo = match photo_bytes {
        Some(b) => Some(save_photo(&st, &b).await?),
        None => None,
    };
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO reports (id, student_id, island_id, text, photo, user_id) VALUES ($1, $2, $3, $4, $5, $6)")
        .bind(id)
        .bind(c.sub)
        .bind(&island_id)
        .bind(&text)
        .bind(&photo)
        .bind(c.uid)
        .execute(&st.db)
        .await?;
    let r = load_report(&st, id).await?;
    Ok((StatusCode::CREATED, Json(report_json(&st, &r, &c))))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditReport {
    #[serde(alias = "island_id")]
    island_id: Option<String>,
    text: Option<String>,
}

async fn own_report(st: &AppState, c: &auth::Claims, id: &str) -> ApiResult<ReportRow> {
    let r = load_report(st, parse_id(id)?).await?;
    if !is_mine(&r, c) {
        return Err(ApiError::Forbidden("Só podes alterar os teus relatos.".into()));
    }
    Ok(r)
}

/// PUT /v1/reports/{id}: JSON {text?, islandId?}. Só o autor.
pub async fn update_report(
    State(st): State<AppState>,
    Student(c): Student,
    Path(id): Path<String>,
    body: Option<Json<EditReport>>,
) -> ApiResult<Json<Value>> {
    let Json(e) = body.ok_or_else(|| ApiError::BadRequest("Envia JSON com text e/ou islandId.".into()))?;
    let r = own_report(&st, &c, &id).await?;
    if e.text.is_none() && e.island_id.is_none() {
        return Err(ApiError::BadRequest("Nada para alterar: envia text e/ou islandId.".into()));
    }
    let text = match &e.text {
        Some(t) => clean_text(t)?,
        None => r.text.clone(),
    };
    let island_id = match &e.island_id {
        Some(i) => find_island(i)?.id.to_string(),
        None => r.island_id.clone(),
    };
    sqlx::query("UPDATE reports SET text = $1, island_id = $2, updated_at = now() WHERE id = $3")
        .bind(&text)
        .bind(&island_id)
        .bind(r.id)
        .execute(&st.db)
        .await?;
    Ok(Json(report_json(&st, &load_report(&st, r.id).await?, &c)))
}

/// PUT /v1/reports/{id}/photo: multipart/form-data com o campo photo. Substitui a fotografia.
pub async fn put_photo(
    State(st): State<AppState>,
    Student(c): Student,
    Path(id): Path<String>,
    mut mp: Multipart,
) -> ApiResult<Json<Value>> {
    let r = own_report(&st, &c, &id).await?;
    let mut bytes = None;
    while let Some(field) = mp.next_field().await.map_err(|e| ApiError::BadRequest(e.body_text()))? {
        if field.name() == Some("photo") {
            bytes = Some(field.bytes().await.map_err(|e| ApiError::BadRequest(e.body_text()))?.to_vec());
        }
    }
    let bytes = bytes.filter(|b| !b.is_empty()).ok_or_else(|| ApiError::BadRequest("Falta o campo photo.".into()))?;
    let name = save_photo(&st, &bytes).await?;
    sqlx::query("UPDATE reports SET photo = $1, updated_at = now() WHERE id = $2").bind(&name).bind(r.id).execute(&st.db).await?;
    remove_photo(&st, &r.photo).await;
    Ok(Json(report_json(&st, &load_report(&st, r.id).await?, &c)))
}

/// DELETE /v1/reports/{id}. Só o autor.
pub async fn delete_report(State(st): State<AppState>, Student(c): Student, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let r = own_report(&st, &c, &id).await?;
    sqlx::query("DELETE FROM reports WHERE id = $1").bind(r.id).execute(&st.db).await?;
    remove_photo(&st, &r.photo).await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- docente ----------

#[derive(Deserialize)]
pub struct NewStudent {
    name: String,
    github: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Credentials {
    name: String,
    github: Option<String>,
    client_id: String,
    client_secret: String,
}

pub async fn admin_create_student(State(st): State<AppState>, _a: Admin, Json(n): Json<NewStudent>) -> ApiResult<(StatusCode, Json<Credentials>)> {
    let name = n.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::BadRequest("Falta o nome.".into()));
    }
    let client_id = format!("pdm-{}", auth::random_token(6));
    let secret = auth::random_token(24);
    sqlx::query("INSERT INTO students (id, client_id, secret_hash, name, github) VALUES ($1, $2, $3, $4, $5)")
        .bind(Uuid::new_v4())
        .bind(&client_id)
        .bind(auth::hash_secret(&secret)?)
        .bind(&name)
        .bind(&n.github)
        .execute(&st.db)
        .await?;
    Ok((StatusCode::CREATED, Json(Credentials { name, github: n.github, client_id, client_secret: secret })))
}

pub async fn admin_reset_secret(State(st): State<AppState>, _a: Admin, Path(client_id): Path<String>) -> ApiResult<Json<Credentials>> {
    let secret = auth::random_token(24);
    let row: Option<(String, Option<String>)> =
        sqlx::query_as("UPDATE students SET secret_hash = $1 WHERE client_id = $2 RETURNING name, github")
            .bind(auth::hash_secret(&secret)?)
            .bind(&client_id)
            .fetch_optional(&st.db)
            .await?;
    let (name, github) = row.ok_or_else(|| ApiError::NotFound("Esse aluno não existe.".into()))?;
    Ok(Json(Credentials { name, github, client_id, client_secret: secret }))
}

pub async fn admin_students(State(st): State<AppState>, _a: Admin) -> ApiResult<Json<Value>> {
    let rows: Vec<(String, String, Option<String>, Option<OffsetDateTime>, i64, i64)> = sqlx::query_as(
        "SELECT s.client_id, s.name, s.github, s.last_seen, count(r.id),
                (SELECT count(*) FROM users u WHERE u.app_id = s.id)
         FROM students s LEFT JOIN reports r ON r.student_id = s.id
         GROUP BY s.id ORDER BY s.name",
    )
    .fetch_all(&st.db)
    .await?;
    Ok(Json(Value::Array(
        rows.into_iter()
            .map(|(client_id, name, github, last_seen, reports, users)| {
                json!({ "clientId": client_id, "name": name, "github": github, "lastSeen": last_seen.map(ts), "reports": reports, "users": users })
            })
            .collect(),
    )))
}

pub async fn admin_delete_report(State(st): State<AppState>, _a: Admin, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let r = load_report(&st, parse_id(&id)?).await?;
    sqlx::query("DELETE FROM reports WHERE id = $1").bind(r.id).execute(&st.db).await?;
    remove_photo(&st, &r.photo).await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn admin_delete_student(State(st): State<AppState>, _a: Admin, Path(client_id): Path<String>) -> ApiResult<StatusCode> {
    let photos: Vec<(Option<String>,)> = sqlx::query_as(
        "SELECT r.photo FROM reports r JOIN students s ON s.id = r.student_id WHERE s.client_id = $1",
    )
    .bind(&client_id)
    .fetch_all(&st.db)
    .await?;
    let done = sqlx::query("DELETE FROM students WHERE client_id = $1").bind(&client_id).execute(&st.db).await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound("Esse aluno não existe.".into()));
    }
    for (p,) in photos {
        remove_photo(&st, &p).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Resumo de uso por aluno: pedidos, dias ativos, último pedido, relatos e endpoints mais usados.
pub async fn admin_usage(State(st): State<AppState>, _a: Admin) -> ApiResult<Json<Value>> {
    let rows: Vec<(String, String, Option<String>, i64, i64, i64, i64, Option<OffsetDateTime>, Option<OffsetDateTime>, i64)> = sqlx::query_as(
        "SELECT s.client_id, s.name, s.github,
                count(l.id) FILTER (WHERE l.event IS DISTINCT FROM 'login_failed'),
                count(l.id) FILTER (WHERE l.at > now() - interval '24 hours'),
                count(DISTINCT (l.at AT TIME ZONE 'Atlantic/Azores')::date),
                count(l.id) FILTER (WHERE l.event = 'login_failed'),
                min(l.at), max(l.at),
                (SELECT count(*) FROM reports r WHERE r.student_id = s.id)
         FROM students s LEFT JOIN request_log l ON l.client_id = s.client_id
         GROUP BY s.id ORDER BY s.name",
    )
    .fetch_all(&st.db)
    .await?;
    let top: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT client_id, method || ' ' || regexp_replace(split_part(path, '?', 1), '[0-9a-f]{8}-[0-9a-f-]{27}', '{id}'), count(*)
         FROM request_log WHERE client_id IS NOT NULL
         GROUP BY 1, 2 ORDER BY 1, 3 DESC",
    )
    .fetch_all(&st.db)
    .await?;
    Ok(Json(Value::Array(
        rows.into_iter()
            .map(|(client_id, name, github, requests, last24h, days, failed, first, last, reports)| {
                let endpoints: Vec<Value> = top
                    .iter()
                    .filter(|(c, _, _)| *c == client_id)
                    .take(5)
                    .map(|(_, e, n)| json!({ "endpoint": e, "count": n }))
                    .collect();
                json!({
                    "clientId": client_id, "name": name, "github": github,
                    "requests": requests, "last24h": last24h, "activeDays": days, "failedLogins": failed,
                    "firstRequest": first.map(ts), "lastRequest": last.map(ts),
                    "reports": reports, "topEndpoints": endpoints,
                })
            })
            .collect(),
    )))
}

// ---------- exercício de aula: erros de propósito ----------

#[derive(Deserialize)]
pub struct Bugs {
    enabled: bool,
}

pub async fn admin_bugs(State(st): State<AppState>, _a: Admin) -> Json<Value> {
    Json(json!({ "enabled": bugs_on(&st) }))
}

pub async fn admin_set_bugs(State(st): State<AppState>, _a: Admin, Json(b): Json<Bugs>) -> Json<Value> {
    st.bugs.store(b.enabled, std::sync::atomic::Ordering::Relaxed);
    tracing::info!("exercício de erros: {}", if b.enabled { "ligado" } else { "desligado" });
    Json(json!({ "enabled": b.enabled }))
}
