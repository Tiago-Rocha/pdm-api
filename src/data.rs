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
}

pub const ISLANDS: &[Island] = &[
    Island { id: "sao-miguel", name: "São Miguel", image: "sao_miguel.jpg", ipma_id: 3420300, ipma_local: "Ponta Delgada" },
    Island { id: "santa-maria", name: "Santa Maria", image: "santa_maria.jpg", ipma_id: 3410100, ipma_local: "Vila do Porto" },
    Island { id: "terceira", name: "Terceira", image: "terceira.jpg", ipma_id: 3430100, ipma_local: "Angra do Heroísmo" },
    Island { id: "graciosa", name: "Graciosa", image: "graciosa.jpg", ipma_id: 3440100, ipma_local: "Santa Cruz da Graciosa" },
    Island { id: "sao-jorge", name: "São Jorge", image: "sao_jorge.jpg", ipma_id: 3450200, ipma_local: "Velas" },
    Island { id: "pico", name: "Pico", image: "pico.jpg", ipma_id: 3460200, ipma_local: "Madalena" },
    Island { id: "faial", name: "Faial", image: "faial.jpg", ipma_id: 3470100, ipma_local: "Horta" },
    Island { id: "flores", name: "Flores", image: "flores.jpg", ipma_id: 3480200, ipma_local: "Santa Cruz das Flores" },
    Island { id: "corvo", name: "Corvo", image: "corvo.jpg", ipma_id: 3490100, ipma_local: "Vila do Corvo" },
];

pub fn island(id: &str) -> Option<&'static Island> {
    ISLANDS.iter().find(|i| i.id == id)
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
