//! Placar ao vivo dos exercícios de ordenar código dos slides.
//!
//! Cada aluno envia o progresso (`POST /v1/play/{room}/{puzzle}`) e o quadro da sala
//! lê o placar (`GET`). Só nome e equipa, em memória: nada vai para a base de dados,
//! e uma sala desaparece seis horas depois da última jogada.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::AppState;

const TTL: Duration = Duration::from_secs(6 * 3600);
const MAX_KEYS: usize = 500;
const MAX_PLAYERS: usize = 120;

#[derive(Default)]
pub struct Store {
    boards: Mutex<HashMap<String, Board>>,
}

pub type Shared = Arc<Store>;

struct Board {
    touched: Instant,
    players: HashMap<String, Player>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    name: String,
    team: String,
    right: u32,
    total: u32,
    tries: u32,
    ms: u64,
    solved: bool,
    /// no jogo de construir a app em conjunto, a parte que calhou a este aluno
    part: String,
    /// pergunta de escolha múltipla depois do exercício: só conta a primeira resposta
    #[serde(skip_serializing_if = "Option::is_none")]
    quiz: Option<bool>,
    /// o aluno carregou em "Pedir ajuda" (fica até voltar a carregar)
    help: bool,
    /// segundos desde a última jogada deste aluno
    #[serde(skip)]
    seen: Option<Instant>,
    idle: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Move {
    player: String,
    name: String,
    team: String,
    #[serde(default)]
    right: u32,
    #[serde(default)]
    total: u32,
    #[serde(default)]
    tries: u32,
    #[serde(default)]
    ms: u64,
    #[serde(default)]
    solved: bool,
    /// partes possíveis: a primeira jogada recebe a que tem menos alunos
    #[serde(default)]
    parts: Vec<String>,
    #[serde(default)]
    quiz: Option<bool>,
    #[serde(default)]
    help: Option<bool>,
}

#[derive(Serialize)]
pub struct Scoreboard {
    players: Vec<Player>,
    #[serde(skip_serializing_if = "Option::is_none")]
    you: Option<Player>,
}

fn key(room: &str, puzzle: &str) -> Result<String, ApiError> {
    let ok = |s: &str| !s.is_empty() && s.len() <= 40 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok(room) || !ok(puzzle) {
        return Err(ApiError::BadRequest("Sala ou exercício inválido.".into()));
    }
    Ok(format!("{}/{}", room.to_ascii_lowercase(), puzzle.to_ascii_lowercase()))
}

fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(max).collect()
}

pub async fn post_move(
    State(st): State<AppState>,
    Path((room, puzzle)): Path<(String, String)>,
    Json(m): Json<Move>,
) -> Result<Json<Scoreboard>, ApiError> {
    st.play.apply(&key(&room, &puzzle)?, m, Instant::now()).map(Json)
}

impl Store {
    fn apply(&self, k: &str, m: Move, now: Instant) -> Result<Scoreboard, ApiError> {
        let name = clean(&m.name, 24);
        let team = clean(&m.team, 24);
        let player = clean(&m.player, 40);
        if name.is_empty() || player.is_empty() || m.total > 200 || m.right > m.total {
            return Err(ApiError::BadRequest("Faltam o nome ou o jogador, ou os números não batem certo.".into()));
        }
        let mut boards = self.boards.lock().unwrap();
        boards.retain(|_, b| now.duration_since(b.touched) < TTL);
        if !boards.contains_key(k) && boards.len() >= MAX_KEYS {
            return Err(ApiError::BadRequest("Demasiadas salas abertas. Tenta mais tarde.".into()));
        }
        let board = boards.entry(k.to_string()).or_insert_with(|| Board { touched: now, players: HashMap::new() });
        if !board.players.contains_key(&player) && board.players.len() >= MAX_PLAYERS {
            return Err(ApiError::BadRequest("A sala está cheia.".into()));
        }
        board.touched = now;
        // jogo de construir a app: a primeira jogada recebe a parte com menos alunos
        let parts: Vec<String> = m.parts.iter().map(|s| clean(s, 40)).filter(|s| !s.is_empty()).take(30).collect();
        let needs_part = !parts.is_empty()
            && board.players.get(&player).is_none_or(|p| p.part.is_empty() || !parts.contains(&p.part));
        let new_part = if needs_part {
            let count = |x: &String| board.players.values().filter(|p| &p.part == x).count();
            parts.iter().min_by_key(|x| count(x)).cloned()
        } else {
            None
        };
        let p = board.players.entry(player.clone()).or_insert(Player {
            name: String::new(), team: String::new(), right: 0, total: 0, tries: 0, ms: 0, solved: false,
            part: String::new(), quiz: None, help: false, seen: None, idle: 0,
        });
        if let Some(part) = new_part {
            // parte nova, jogo novo: o progresso que vinha de outra parte não conta
            p.part = part;
            p.right = 0;
            p.tries = 0;
            p.ms = 0;
            p.solved = false;
            p.total = 0;
            p.quiz = None;
        } else if !p.solved {
            // depois de resolvido, o resultado não muda (um "Baralhar" não apaga a vitória)
            p.right = m.right;
            p.tries = m.tries;
            p.ms = m.ms;
            p.total = m.total;
            p.solved = m.solved && m.total > 0 && m.right == m.total;
        }
        if p.quiz.is_none() && p.solved {
            p.quiz = m.quiz;
        }
        if let Some(h) = m.help {
            p.help = h;
        }
        if p.solved {
            p.help = false;
        }
        p.name = name;
        p.team = team;
        p.seen = Some(now);
        let you = board.players.get(&player).cloned();
        let mut snap = snapshot(board, now);
        snap.you = you;
        Ok(snap)
    }
}

pub async fn get_board(
    State(st): State<AppState>,
    Path((room, puzzle)): Path<(String, String)>,
) -> Result<Json<Scoreboard>, ApiError> {
    let k = key(&room, &puzzle)?;
    let boards = st.play.boards.lock().unwrap();
    let now = Instant::now();
    Ok(Json(match boards.get(&k) {
        Some(b) => snapshot(b, now),
        None => Scoreboard { players: vec![], you: None },
    }))
}

#[derive(Serialize)]
pub struct Room {
    /// exercício: alunos desse exercício
    puzzles: HashMap<String, Vec<Player>>,
}

/// Painel do professor: todos os exercícios de uma sala de uma vez.
pub async fn get_room(State(st): State<AppState>, Path(room): Path<String>) -> Result<Json<Room>, ApiError> {
    Ok(Json(st.play.room(&room, Instant::now())?))
}

impl Store {
    fn room(&self, room: &str, now: Instant) -> Result<Room, ApiError> {
        let prefix = key(room, "x")?.trim_end_matches('x').to_string();
        let boards = self.boards.lock().unwrap();
        let puzzles = boards
            .iter()
            .filter_map(|(k, b)| k.strip_prefix(&prefix).map(|p| (p.to_string(), snapshot(b, now).players)))
            .collect();
        Ok(Room { puzzles })
    }
}

fn snapshot(b: &Board, now: Instant) -> Scoreboard {
    let mut players: Vec<Player> = b
        .players
        .values()
        .map(|p| Player { idle: p.seen.map(|s| now.duration_since(s).as_secs()).unwrap_or(0), ..p.clone() })
        .collect();
    players.sort_by(|a, b| a.team.cmp(&b.team).then(a.name.cmp(&b.name)));
    Scoreboard { players, you: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mv(player: &str, right: u32, total: u32, solved: bool, parts: &[&str]) -> Move {
        Move {
            player: player.into(), name: format!("N{player}"), team: "1".into(), right, total, tries: 1, ms: 1000, solved,
            parts: parts.iter().map(|s| s.to_string()).collect(), quiz: None, help: None,
        }
    }

    #[test]
    fn parts_are_spread_and_kept() {
        let s = Store::default();
        let now = Instant::now();
        let parts = ["dados", "estado", "ui"];
        let a = s.apply("r/app", mv("a", 0, 0, false, &parts), now).unwrap().you.unwrap().part;
        let b = s.apply("r/app", mv("b", 0, 0, false, &parts), now).unwrap().you.unwrap().part;
        let c = s.apply("r/app", mv("c", 0, 0, false, &parts), now).unwrap().you.unwrap().part;
        let mut all = vec![a.clone(), b, c];
        all.sort();
        assert_eq!(all, vec!["dados", "estado", "ui"]);
        // a mesma pessoa volta a entrar e fica com a mesma parte
        let again = s.apply("r/app", mv("a", 3, 10, false, &parts), now).unwrap().you.unwrap();
        assert_eq!(again.part, a);
        assert_eq!(again.right, 3);
    }

    #[test]
    fn solved_is_final() {
        let s = Store::default();
        let now = Instant::now();
        s.apply("r/p", mv("a", 5, 5, true, &[]), now).unwrap();
        let after = s.apply("r/p", mv("a", 1, 5, false, &[]), now).unwrap().you.unwrap();
        assert!(after.solved);
        assert_eq!(after.right, 5);
        assert!(s.apply("r/p", mv("a", 6, 5, false, &[]), now).is_err());
        // a primeira resposta à pergunta fica; as seguintes não mudam nada
        let mut m = mv("a", 5, 5, true, &[]);
        m.quiz = Some(false);
        s.apply("r/p", m, now).unwrap();
        let mut m = mv("a", 5, 5, true, &[]);
        m.quiz = Some(true);
        assert_eq!(s.apply("r/p", m, now).unwrap().you.unwrap().quiz, Some(false));
    }

    #[test]
    fn help_and_room() {
        let s = Store::default();
        let now = Instant::now();
        let mut m = mv("a", 1, 5, false, &[]);
        m.help = Some(true);
        assert!(s.apply("r/e1", m, now).unwrap().you.unwrap().help);
        s.apply("r/e2", mv("a", 5, 5, true, &[]), now).unwrap();
        s.apply("outra/e1", mv("b", 0, 5, false, &[]), now).unwrap();
        let room = s.room("r", now).unwrap();
        assert_eq!(room.puzzles.len(), 2);
        assert!(room.puzzles["e1"][0].help);
        // resolver tira o pedido de ajuda
        s.apply("r/e1", mv("a", 5, 5, true, &[]), now).unwrap();
        assert!(!s.room("r", now).unwrap().puzzles["e1"][0].help);
    }
}
