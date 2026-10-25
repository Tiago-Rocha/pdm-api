//! Dados fixos: as nove ilhas (com a localidade do IPMA de cada uma) e os trilhos.
//! São os mesmos do lib/data.dart dos repositórios das aulas.
use serde::Serialize;

pub struct Island {
    pub id: &'static str,
    pub name: &'static str,
    pub image: &'static str,
    /// globalIdLocal do IPMA para a previsão
    pub ipma_id: u32,
    pub ipma_local: &'static str,
    /// centro aproximado, para o mapa e para o sismo mais próximo
    pub lat: f64,
    pub lon: f64,
    /// caixa (lat mín, lat máx, lon mín, lon máx) onde caem as estações do IPMA da ilha
    pub bbox: (f64, f64, f64, f64),
    /// área dos avisos do IPMA: AOR oriental, ACE central, AOC ocidental
    pub warning_area: &'static str,
    /// ponto da previsão do estado do mar mais próximo (sea-locations do IPMA)
    pub sea_id: u32,
    pub sea_local: &'static str,
}

pub const ISLANDS: &[Island] = &[
    Island { id: "sao-miguel", name: "São Miguel", image: "sao_miguel.jpg", ipma_id: 3420300, ipma_local: "Ponta Delgada", lat: 37.78, lon: -25.50, bbox: (37.69, 37.92, -25.88, -25.12), warning_area: "AOR", sea_id: 3420226, sea_local: "Ponta Delgada" },
    Island { id: "santa-maria", name: "Santa Maria", image: "santa_maria.jpg", ipma_id: 3410100, ipma_local: "Vila do Porto", lat: 36.97, lon: -25.10, bbox: (36.92, 37.02, -25.20, -24.98), warning_area: "AOR", sea_id: 3420226, sea_local: "Ponta Delgada" },
    Island { id: "terceira", name: "Terceira", image: "terceira.jpg", ipma_id: 3430100, ipma_local: "Angra do Heroísmo", lat: 38.72, lon: -27.22, bbox: (38.62, 38.82, -27.40, -27.03), warning_area: "ACE", sea_id: 3470126, sea_local: "Horta" },
    Island { id: "graciosa", name: "Graciosa", image: "graciosa.jpg", ipma_id: 3440100, ipma_local: "Santa Cruz da Graciosa", lat: 39.05, lon: -28.01, bbox: (38.99, 39.11, -28.10, -27.93), warning_area: "ACE", sea_id: 3470126, sea_local: "Horta" },
    Island { id: "sao-jorge", name: "São Jorge", image: "sao_jorge.jpg", ipma_id: 3450200, ipma_local: "Velas", lat: 38.65, lon: -28.08, bbox: (38.54, 38.77, -28.33, -27.74), warning_area: "ACE", sea_id: 3470126, sea_local: "Horta" },
    Island { id: "pico", name: "Pico", image: "pico.jpg", ipma_id: 3460200, ipma_local: "Madalena", lat: 38.47, lon: -28.33, bbox: (38.38, 38.57, -28.56, -28.02), warning_area: "ACE", sea_id: 3470126, sea_local: "Horta" },
    Island { id: "faial", name: "Faial", image: "faial.jpg", ipma_id: 3470100, ipma_local: "Horta", lat: 38.58, lon: -28.70, bbox: (38.51, 38.66, -28.85, -28.58), warning_area: "ACE", sea_id: 3470126, sea_local: "Horta" },
    Island { id: "flores", name: "Flores", image: "flores.jpg", ipma_id: 3480200, ipma_local: "Santa Cruz das Flores", lat: 39.45, lon: -31.20, bbox: (39.36, 39.53, -31.28, -31.12), warning_area: "AOC", sea_id: 3480226, sea_local: "Santa Cruz das Flores" },
    Island { id: "corvo", name: "Corvo", image: "corvo.jpg", ipma_id: 3490100, ipma_local: "Vila do Corvo", lat: 39.70, lon: -31.10, bbox: (39.66, 39.73, -31.14, -31.07), warning_area: "AOC", sea_id: 3480226, sea_local: "Santa Cruz das Flores" },
];

pub fn island(id: &str) -> Option<&'static Island> {
    ISLANDS.iter().find(|i| i.id == id)
}

/// A ilha em cuja caixa cai o ponto (para as estações do IPMA).
pub fn island_at(lat: f64, lon: f64) -> Option<&'static Island> {
    ISLANDS.iter().find(|i| {
        let (a, b, c, d) = i.bbox;
        (a..=b).contains(&lat) && (c..=d).contains(&lon)
    })
}

/// A ilha com o centro mais perto do ponto (para os sismos, que caem no mar).
pub fn nearest_island(lat: f64, lon: f64) -> &'static Island {
    let dist = |i: &Island| {
        let dx = (i.lon - lon) * lat.to_radians().cos();
        let dy = i.lat - lat;
        dx * dx + dy * dy
    };
    ISLANDS.iter().min_by(|a, b| dist(a).total_cmp(&dist(b))).expect("há ilhas")
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

pub struct Trail {
    pub id: &'static str,
    pub name: &'static str,
    pub island_id: &'static str,
    pub distance_km: f64,
    pub duration: &'static str,
    pub difficulty: Difficulty,
    pub description: &'static str,
}

use Difficulty::*;

pub const TRAILS: &[Trail] = &[
    Trail { id: "lagoa-do-fogo", name: "Lagoa do Fogo", island_id: "sao-miguel", distance_km: 10.5, duration: "3 h 30", difficulty: Medium, description: "Desce da serra até à margem da lagoa, numa reserva natural sem estradas." },
    Trail { id: "sete-cidades", name: "Vista do Rei – Sete Cidades", island_id: "sao-miguel", distance_km: 12.0, duration: "4 h", difficulty: Medium, description: "Pelo rebordo da caldeira, com as lagoas Azul e Verde lá em baixo." },
    Trail { id: "salto-do-prego", name: "Salto do Prego", island_id: "sao-miguel", distance_km: 4.5, duration: "2 h", difficulty: Easy, description: "Do Faial da Terra até uma cascata, entre ribeiras e moinhos antigos." },
    Trail { id: "pico-alto", name: "Pico Alto", island_id: "santa-maria", distance_km: 6.0, duration: "2 h", difficulty: Easy, description: "Sobe ao ponto mais alto da ilha, com vista para as baías." },
    Trail { id: "misterios-negros", name: "Mistérios Negros", island_id: "terceira", distance_km: 5.0, duration: "2 h 30", difficulty: Medium, description: "Entre domos de lava e turfeiras, no interior da ilha." },
    Trail { id: "caldeira-graciosa", name: "Volta à Caldeira", island_id: "graciosa", distance_km: 8.0, duration: "3 h", difficulty: Easy, description: "À volta da caldeira, com a Furna do Enxofre lá dentro." },
    Trail { id: "faja-santo-cristo", name: "Serra do Topo – Fajã de Santo Cristo", island_id: "sao-jorge", distance_km: 10.0, duration: "3 h 30", difficulty: Medium, description: "Desce até à fajã da lagoa, onde só se chega a pé ou de moto." },
    Trail { id: "montanha-do-pico", name: "Montanha do Pico", island_id: "pico", distance_km: 7.6, duration: "7 h", difficulty: Hard, description: "Subida ao ponto mais alto de Portugal, 2351 m. Exige registo na Casa da Montanha." },
    Trail { id: "caldeira-faial", name: "Volta à Caldeira do Faial", island_id: "faial", distance_km: 7.0, duration: "2 h 30", difficulty: Easy, description: "À volta da cratera com 2 km de diâmetro no centro da ilha." },
    Trail { id: "capelinhos", name: "Vulcão dos Capelinhos", island_id: "faial", distance_km: 4.0, duration: "1 h 30", difficulty: Easy, description: "Sobre as cinzas da erupção de 1957, ao lado do farol." },
    Trail { id: "faja-lopo-vaz", name: "Fajã de Lopo Vaz", island_id: "flores", distance_km: 3.0, duration: "1 h 30", difficulty: Hard, description: "Uma descida íngreme até uma fajã no sul da ilha." },
    Trail { id: "faja-grande", name: "Lajedo – Fajã Grande", island_id: "flores", distance_km: 12.0, duration: "4 h", difficulty: Medium, description: "Pela costa oeste, entre ribeiras e cascatas." },
    Trail { id: "caldeirao", name: "Caldeirão", island_id: "corvo", distance_km: 5.0, duration: "2 h", difficulty: Medium, description: "Desce ao fundo da cratera, com lagoas e pequenos ilhéus." },
];
