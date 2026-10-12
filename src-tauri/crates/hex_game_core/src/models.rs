// hex_game_core/src/models.rs
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum Component {
    TerrainType { terrain: String },
    LifePoints { current: i32, max: i32 },
    Tags { labels: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpatialPosition {
    pub q: i32,
    pub r: i32,
    pub z: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub spatial: SpatialPosition,
    pub components: HashMap<String, Component>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchContext {
    pub match_id: String,
    pub active_player: String,
    pub turn_phase: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    pub context: MatchContext,
    pub entities: HashMap<String, Entity>,
}
