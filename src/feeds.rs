//! Outros dados abertos do IPMA para os Açores: avisos, estado do mar, sismos e
//! observações das estações. Normalizados (sem -99, sem números em texto) e com cache.
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::data::{self, ISLANDS};
use crate::error::{ApiError, ApiResult};

const BASE: &str = "https://api.ipma.pt/open-data";

/// Um ficheiro do IPMA em cache. O lock fica preso durante o pedido ao IPMA, para
/// que uma turma inteira a pedir ao mesmo tempo faça um só pedido.
pub struct Feed<T> {
    ttl: Duration,
    slot: Mutex<Option<(Instant, Arc<T>)>>,
}

impl<T> Feed<T> {
    const fn new(ttl: Duration) -> Self {
        Self { ttl, slot: Mutex::const_new(None) }
    }

    async fn get<F, Fut>(&self, what: &str, fetch: F) -> ApiResult<Arc<T>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = anyhow::Result<T>>,
    {
        let mut slot = self.slot.lock().await;
        if let Some((at, v)) = slot.as_ref() {
            if at.elapsed() < self.ttl {
                return Ok(v.clone());
            }
        }
        match fetch().await {
            Ok(v) => {
                let v = Arc::new(v);
                *slot = Some((Instant::now(), v.clone()));
                Ok(v)
            }
            Err(e) => {
                tracing::warn!("IPMA falhou ({what}): {e:#}");
                // sem IPMA, serve o último que houver, mesmo antigo
                slot.as_ref()
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| ApiError::Upstream("O IPMA não respondeu. Tenta daqui a pouco.".into()))
            }
        }
    }
}

pub struct Feeds {
    warnings: Feed<Vec<Warning>>,
    sea: Feed<HashMap<u32, Sea>>,
    earthquakes: Feed<Vec<Earthquake>>,
    observations: Feed<HashMap<&'static str, Observations>>,
}

impl Default for Feeds {
    fn default() -> Self {
        Self {
            warnings: Feed::new(Duration::from_secs(10 * 60)),
            sea: Feed::new(Duration::from_secs(30 * 60)),
            earthquakes: Feed::new(Duration::from_secs(10 * 60)),
            observations: Feed::new(Duration::from_secs(15 * 60)),
        }
    }
}

async fn get_json<T: serde::de::DeserializeOwned>(http: &reqwest::Client, path: &str) -> anyhow::Result<T> {
    Ok(http.get(format!("{BASE}/{path}")).send().await?.error_for_status()?.json().await?)
}

/// Número que o IPMA tanto manda como texto como como número; -99 quer dizer "sem leitura".
fn num(v: &Value) -> Option<f64> {
    let n = match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }?;
    (n > -90.0).then_some(n)
}

// ---------- avisos ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Warning {
    pub area: &'static str,
    pub area_name: &'static str,
    pub island_ids: Vec<&'static str>,
    /// wind, rain, seaState, thunderstorm, fog, snow, hot ou cold
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub type_label: String,
    /// yellow, orange ou red (os verdes, que querem dizer "sem aviso", não aparecem)
    pub level: &'static str,
    pub level_label: &'static str,
    pub text: String,
    pub start_time: String,
    pub end_time: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawWarning {
    text: String,
    awareness_type_name: String,
    id_area_aviso: String,
    start_time: String,
    end_time: String,
    #[serde(rename = "awarenessLevelID")]
    awareness_level_id: String,
}

fn area_name(area: &str) -> Option<&'static str> {
    match area {
        "AOR" => Some("Grupo Oriental"),
        "ACE" => Some("Grupo Central"),
        "AOC" => Some("Grupo Ocidental"),
        _ => None,
    }
}

fn warning_type(name: &str) -> &'static str {
    match name {
        "Vento" => "wind",
        "Precipitação" => "rain",
        "Agitação Marítima" => "seaState",
        "Trovoada" => "thunderstorm",
        "Nevoeiro" => "fog",
        "Neve" => "snow",
        "Tempo Quente" => "hot",
        "Tempo Frio" => "cold",
        _ => "other",
    }
}

fn level(id: &str) -> Option<(&'static str, &'static str, u8)> {
    match id {
        "yellow" => Some(("yellow", "Amarelo", 1)),
        "orange" => Some(("orange", "Laranja", 2)),
        "red" => Some(("red", "Vermelho", 3)),
        _ => None,
    }
}

fn normalize_warnings(raw: Vec<RawWarning>) -> Vec<Warning> {
    let mut out: Vec<(u8, Warning)> = raw
        .into_iter()
        .filter_map(|w| {
            let area_name = area_name(&w.id_area_aviso)?;
            let (level, level_label, rank) = level(&w.awareness_level_id)?;
            let area = ISLANDS.iter().find(|i| i.warning_area == w.id_area_aviso)?.warning_area;
            Some((
                rank,
                Warning {
                    area,
                    area_name,
                    island_ids: ISLANDS.iter().filter(|i| i.warning_area == area).map(|i| i.id).collect(),
                    kind: warning_type(&w.awareness_type_name),
                    type_label: w.awareness_type_name,
                    level,
                    level_label,
                    text: w.text.trim().to_string(),
                    start_time: w.start_time,
                    end_time: w.end_time,
                },
            ))
        })
        .collect();
    // os mais graves primeiro, e dentro do mesmo nível os que começam mais cedo
    out.sort_by(|(ra, a), (rb, b)| rb.cmp(ra).then_with(|| a.start_time.cmp(&b.start_time)));
    out.into_iter().map(|(_, w)| w).collect()
}

impl Feeds {
    pub async fn warnings(&self, http: &reqwest::Client) -> ApiResult<Arc<Vec<Warning>>> {
        self.warnings
            .get("avisos", || async {
                let raw: Vec<RawWarning> = get_json(http, "forecast/warnings/warnings_www.json").await?;
                Ok(normalize_warnings(raw))
            })
            .await
    }
}

// ---------- estado do mar ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Sea {
    pub updated_at: String,
    pub days: Vec<SeaDay>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SeaDay {
    pub date: String,
    /// altura significativa da ondulação, em metros
    pub wave_height_min: Option<f64>,
    pub wave_height_max: Option<f64>,
    /// período da ondulação, em segundos
    pub wave_period_min: Option<f64>,
    pub wave_period_max: Option<f64>,
    pub wave_direction: Option<String>,
    /// altura total do mar (ondulação e vaga), em metros
    pub total_sea_min: Option<f64>,
    pub total_sea_max: Option<f64>,
    /// temperatura da água, em °C
    pub sea_temp_min: Option<f64>,
    pub sea_temp_max: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSeaFile {
    forecast_date: String,
    data_update: String,
    data: Vec<HashMap<String, Value>>,
}

impl Feeds {
    pub async fn sea(&self, http: &reqwest::Client, sea_id: u32) -> ApiResult<Sea> {
        let all = self
            .sea
            .get("mar", || async {
                let mut by_place: HashMap<u32, Sea> = HashMap::new();
                for day in 0..3 {
                    let f: RawSeaFile =
                        get_json(http, &format!("forecast/oceanography/daily/hp-daily-sea-forecast-day{day}.json")).await?;
                    for p in &f.data {
                        let Some(id) = p.get("globalIdLocal").and_then(num).map(|n| n as u32) else { continue };
                        if !ISLANDS.iter().any(|i| i.sea_id == id) {
                            continue;
                        }
                        let n = |k: &str| p.get(k).and_then(num);
                        let entry = by_place.entry(id).or_insert_with(|| Sea { updated_at: f.data_update.clone(), days: vec![] });
                        entry.days.push(SeaDay {
                            date: f.forecast_date.clone(),
                            wave_height_min: n("waveHighMin"),
                            wave_height_max: n("waveHighMax"),
                            wave_period_min: n("wavePeriodMin"),
                            wave_period_max: n("wavePeriodMax"),
                            wave_direction: p.get("predWaveDir").and_then(Value::as_str).map(str::to_string),
                            total_sea_min: n("totalSeaMin"),
                            total_sea_max: n("totalSeaMax"),
                            sea_temp_min: n("sstMin"),
                            sea_temp_max: n("sstMax"),
                        });
                    }
                }
                Ok(by_place)
            })
            .await?;
        all.get(&sea_id)
            .cloned()
            .ok_or_else(|| ApiError::Upstream("O IPMA não tem previsão do mar para esta ilha agora. Tenta daqui a pouco.".into()))
    }
}

// ---------- sismos ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Earthquake {
    pub id: String,
    pub time: String,
    pub magnitude: f64,
    /// L (local) ou W (momento)
    pub magnitude_type: String,
    pub depth_km: f64,
    pub latitude: f64,
    pub longitude: f64,
    /// zona do epicentro, como o IPMA a escreve
    pub region: String,
    pub nearest_island_id: &'static str,
    /// se foi sentido pela população
    pub felt: bool,
    /// intensidade máxima sentida (escala de Mercalli), por exemplo "IV/V"
    pub intensity: Option<String>,
    /// onde foi mais sentido
    pub felt_at: Option<String>,
}

#[derive(Deserialize)]
struct RawQuakes {
    data: Vec<HashMap<String, Value>>,
}

impl Feeds {
    /// Sismos dos últimos 30 dias na região dos Açores, do mais recente para o mais antigo.
    pub async fn earthquakes(&self, http: &reqwest::Client) -> ApiResult<Arc<Vec<Earthquake>>> {
        self.earthquakes
            .get("sismos", || async {
                let raw: RawQuakes = get_json(http, "observation/seismic/3.json").await?;
                let text = |q: &HashMap<String, Value>, k: &str| {
                    q.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
                };
                let mut out: Vec<Earthquake> = raw
                    .data
                    .iter()
                    .filter_map(|q| {
                        let time = text(q, "time")?;
                        let (lat, lon) = (q.get("lat").and_then(num)?, q.get("lon").and_then(num)?);
                        Some(Earthquake {
                            id: text(q, "sismoId").unwrap_or_else(|| time.replace(['-', ':', 'T'], "")),
                            magnitude: q.get("magnitud").and_then(num)?,
                            magnitude_type: text(q, "magType").unwrap_or_default(),
                            depth_km: q.get("depth").and_then(num).unwrap_or(0.0),
                            latitude: lat,
                            longitude: lon,
                            region: text(q, "obsRegion").unwrap_or_default(),
                            nearest_island_id: data::nearest_island(lat, lon).id,
                            felt: q.get("sensed").and_then(Value::as_bool).unwrap_or(false),
                            intensity: text(q, "degree"),
                            felt_at: text(q, "local"),
                            time,
                        })
                    })
                    .collect();
                out.sort_by(|a, b| b.time.cmp(&a.time));
                Ok(out)
            })
            .await
    }
}

// ---------- observações das estações ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Observations {
    /// a hora (UTC) da leitura mais recente das estações da ilha
    pub observed_at: Option<String>,
    pub stations: Vec<Station>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Station {
    pub id: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    /// hora (UTC) da última leitura desta estação
    pub observed_at: String,
    /// °C
    pub temperature: Option<f64>,
    /// %
    pub humidity: Option<f64>,
    /// km/h
    pub wind_kmh: Option<f64>,
    /// N, NE, E, SE, S, SW, W ou NW
    pub wind_direction: Option<&'static str>,
    /// hPa
    pub pressure: Option<f64>,
    /// mm na última hora
    pub precipitation: Option<f64>,
    /// kJ/m²
    pub radiation: Option<f64>,
}

#[derive(Deserialize)]
struct RawStation {
    geometry: RawPoint,
    properties: RawStationProps,
}

#[derive(Deserialize)]
struct RawPoint {
    coordinates: (f64, f64),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawStationProps {
    id_estacao: u64,
    local_estacao: String,
}

fn wind_dir(id: Option<f64>) -> Option<&'static str> {
    match id? as i64 {
        1 | 9 => Some("N"),
        2 => Some("NE"),
        3 => Some("E"),
        4 => Some("SE"),
        5 => Some("S"),
        6 => Some("SW"),
        7 => Some("W"),
        8 => Some("NW"),
        _ => None,
    }
}

impl Feeds {
    pub async fn observations(&self, http: &reqwest::Client, island_id: &str) -> ApiResult<Observations> {
        let all = self
            .observations
            .get("observações", || async {
                let stations: Vec<RawStation> = get_json(http, "observation/meteorology/stations/stations.json").await?;
                // { "2026-10-25T15:00": { "1200512": { ... } | null, ... }, ... }
                let obs: HashMap<String, HashMap<String, Option<HashMap<String, Value>>>> =
                    get_json(http, "observation/meteorology/stations/observations.json").await?;
                let mut hours: Vec<&String> = obs.keys().collect();
                hours.sort_by(|a, b| b.cmp(a));
                let mut by_island: HashMap<&'static str, Observations> =
                    ISLANDS.iter().map(|i| (i.id, Observations { observed_at: None, stations: vec![] })).collect();
                for s in &stations {
                    let (lon, lat) = s.geometry.coordinates;
                    let Some(island) = data::island_at(lat, lon) else { continue };
                    let id = s.properties.id_estacao.to_string();
                    // a leitura mais recente desta estação nas últimas 24 horas
                    let Some((hour, o)) = hours.iter().find_map(|h| obs[*h].get(&id).and_then(|o| o.as_ref()).map(|o| (*h, o)))
                    else {
                        continue;
                    };
                    let n = |k: &str| o.get(k).and_then(num);
                    let entry = by_island.get_mut(island.id).expect("todas as ilhas");
                    if entry.observed_at.as_deref().is_none_or(|t| t < hour.as_str()) {
                        entry.observed_at = Some(hour.clone());
                    }
                    entry.stations.push(Station {
                        id,
                        name: s.properties.local_estacao.trim().to_string(),
                        latitude: lat,
                        longitude: lon,
                        observed_at: hour.clone(),
                        temperature: n("temperatura"),
                        humidity: n("humidade"),
                        wind_kmh: n("intensidadeVentoKM"),
                        wind_direction: wind_dir(n("idDireccVento")).filter(|_| n("intensidadeVentoKM").is_some()),
                        pressure: n("pressao"),
                        precipitation: n("precAcumulada"),
                        radiation: n("radiacao"),
                    });
                }
                for o in by_island.values_mut() {
                    o.stations.sort_by(|a, b| a.name.cmp(&b.name));
                }
                Ok(by_island)
            })
            .await?;
        Ok(all.get(island_id).cloned().unwrap_or(Observations { observed_at: None, stations: vec![] }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(area: &str, kind: &str, level: &str, start: &str) -> RawWarning {
        RawWarning {
            text: " Rajadas até 90 km/h. ".into(),
            awareness_type_name: kind.into(),
            id_area_aviso: area.into(),
            start_time: start.into(),
            end_time: "2026-10-26T18:00:00".into(),
            awareness_level_id: level.into(),
        }
    }

    #[test]
    fn avisos_sem_verdes_nem_continente_e_os_mais_graves_primeiro() {
        let w = normalize_warnings(vec![
            raw("ACE", "Vento", "green", "2026-10-26T06:00:00"),
            raw("BGC", "Vento", "red", "2026-10-26T06:00:00"),
            raw("AOR", "Precipitação", "yellow", "2026-10-26T03:00:00"),
            raw("AOC", "Agitação Marítima", "orange", "2026-10-26T09:00:00"),
        ]);
        assert_eq!(w.len(), 2);
        assert_eq!((w[0].area, w[0].kind, w[0].level), ("AOC", "seaState", "orange"));
        assert_eq!(w[0].island_ids, vec!["flores", "corvo"]);
        assert_eq!((w[1].area, w[1].kind, w[1].level_label), ("AOR", "rain", "Amarelo"));
        assert_eq!(w[1].text, "Rajadas até 90 km/h.");
    }

    #[test]
    fn menos_99_quer_dizer_sem_leitura() {
        assert_eq!(num(&serde_json::json!(-99.0)), None);
        assert_eq!(num(&serde_json::json!("4.2")), Some(4.2));
        assert_eq!(num(&serde_json::json!(-3.5)), Some(-3.5));
    }

    #[test]
    fn estacoes_e_sismos_caem_na_ilha_certa() {
        assert_eq!(data::island_at(38.552, -28.441).map(|i| i.id), Some("pico"));
        assert_eq!(data::island_at(38.644, -28.059).map(|i| i.id), Some("sao-jorge"));
        assert_eq!(data::island_at(37.0, -9.0).map(|i| i.id), None);
        assert_eq!(data::nearest_island(37.856, -26.274).id, "sao-miguel");
    }
}
