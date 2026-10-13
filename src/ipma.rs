//! Previsão a 5 dias do IPMA (dados abertos), normalizada e com cache de 30 minutos.
use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::error::{ApiError, ApiResult};

const TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Default)]
pub struct Cache(Mutex<HashMap<u32, (Instant, Forecast)>>);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Forecast {
    pub updated_at: String,
    pub days: Vec<Day>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Day {
    pub date: String,
    pub weather_type: WeatherType,
    pub t_min: f64,
    pub t_max: f64,
    /// probabilidade de precipitação, 0 a 100
    pub rain_chance: u32,
    pub wind: Wind,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WeatherType {
    pub id: i32,
    pub description: String,
    /// sunny, partlyCloudy, cloudy, rain ou storm: o enum WeatherType das aulas
    pub category: &'static str,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Wind {
    pub direction: String,
    /// 1 fraco, 2 moderado, 3 forte, 4 muito forte
    pub class: i32,
    pub description: &'static str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Raw {
    data_update: String,
    data: Vec<RawDay>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDay {
    precipita_prob: String,
    t_min: String,
    t_max: String,
    pred_wind_dir: String,
    id_weather_type: i32,
    class_wind_speed: i32,
    forecast_date: String,
}

fn weather(id: i32) -> WeatherType {
    let description = match id {
        1 => "Céu limpo",
        2 => "Céu pouco nublado",
        3 => "Céu parcialmente nublado",
        4 => "Céu muito nublado ou encoberto",
        5 => "Céu nublado por nuvens altas",
        6 => "Aguaceiros/chuva",
        7 => "Aguaceiros/chuva fracos",
        8 => "Aguaceiros/chuva fortes",
        9 => "Chuva/aguaceiros",
        10 => "Chuva fraca ou chuvisco",
        11 => "Chuva/aguaceiros forte",
        12 => "Períodos de chuva",
        13 => "Períodos de chuva fraca",
        14 => "Períodos de chuva forte",
        15 => "Chuvisco",
        16 => "Neblina",
        17 => "Nevoeiro ou nuvens baixas",
        18 => "Neve",
        19 => "Trovoada",
        20 => "Aguaceiros e possibilidade de trovoada",
        21 => "Granizo",
        22 => "Geada",
        23 => "Chuva e possibilidade de trovoada",
        24 => "Nebulosidade convectiva",
        25 => "Céu com períodos de muito nublado",
        26 => "Nevoeiro",
        27 => "Céu nublado",
        28 => "Aguaceiros de neve",
        29 | 30 => "Chuva e neve",
        _ => "Sem informação",
    };
    let category = match id {
        1 => "sunny",
        2 | 3 | 25 => "partlyCloudy",
        6..=15 | 18 | 28..=30 => "rain",
        19..=21 | 23 => "storm",
        _ => "cloudy",
    };
    WeatherType { id, description: description.into(), category }
}

fn wind(direction: String, class: i32) -> Wind {
    let description = match class {
        1 => "Fraco",
        2 => "Moderado",
        3 => "Forte",
        4 => "Muito forte",
        _ => "Sem informação",
    };
    Wind { direction, class, description }
}

fn num(s: &str) -> f64 {
    s.trim().parse().unwrap_or(0.0)
}

impl Cache {
    pub async fn forecast(&self, http: &reqwest::Client, ipma_id: u32) -> ApiResult<Forecast> {
        if let Some((at, f)) = self.0.lock().await.get(&ipma_id) {
            if at.elapsed() < TTL {
                return Ok(f.clone());
            }
        }
        match fetch(http, ipma_id).await {
            Ok(f) => {
                self.0.lock().await.insert(ipma_id, (Instant::now(), f.clone()));
                Ok(f)
            }
            Err(e) => {
                tracing::warn!("IPMA falhou para {ipma_id}: {e:#}");
                // sem IPMA, serve a última previsão que houver, mesmo antiga
                match self.0.lock().await.get(&ipma_id) {
                    Some((_, f)) => Ok(f.clone()),
                    None => Err(ApiError::Upstream("O IPMA não respondeu. Tenta daqui a pouco.".into())),
                }
            }
        }
    }
}

async fn fetch(http: &reqwest::Client, ipma_id: u32) -> anyhow::Result<Forecast> {
    let url = format!("https://api.ipma.pt/open-data/forecast/meteorology/cities/daily/{ipma_id}.json");
    let raw: Raw = http.get(url).send().await?.error_for_status()?.json().await?;
    Ok(Forecast {
        updated_at: raw.data_update,
        days: raw
            .data
            .into_iter()
            .map(|d| Day {
                date: d.forecast_date,
                weather_type: weather(d.id_weather_type),
                t_min: num(&d.t_min),
                t_max: num(&d.t_max),
                rain_chance: num(&d.precipita_prob).round() as u32,
                wind: wind(d.pred_wind_dir, d.class_wind_speed),
            })
            .collect(),
    })
}
