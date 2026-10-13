// hex_game_core/src/models.rs
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum Component {
    Terrain {
        inventory_id: String,
        face_texture: String,
        side_texture: String,
    },
    TerrainType {
        terrain: String,
    },
    LifePoints {
        current: i32,
        max: i32,
    },
    Destructible {
        current_life: i32,
        max_life: i32,
    },
    Tags {
        labels: Vec<String>,
    },
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_primitive: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mod_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchContext {
    pub match_id: String,
    pub active_player: String,
    pub turn_phase: TurnPhase,
    #[serde(default)]
    pub setup_locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnPhase {
    #[serde(alias = "Action", alias = "Movement")]
    Movement,
    #[serde(alias = "Attack")]
    Attack,
    #[serde(alias = "End")]
    End,
    #[serde(alias = "Finished")]
    Finished,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    pub context: MatchContext,
    pub entities: HashMap<String, Entity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HexoscapeMapFile {
    #[serde(rename = "hexMap")]
    pub hex_map: HexMapInfo,
    #[serde(rename = "boardPieces", default)]
    pub board_pieces: Vec<BoardPiece>,
    #[serde(rename = "starterUnits", default)]
    pub starter_units: Vec<StarterUnitDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StarterUnitDefinition {
    pub id: String,
    #[serde(default)]
    pub mod_id: Option<String>,
    #[serde(default, alias = "player", alias = "owner")]
    pub seat: Option<String>,
    #[serde(default)]
    pub components: HashMap<String, Component>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HexMapInfo {
    #[serde(flatten)]
    pub properties: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardPiece {
    pub uid: String,
    #[serde(rename = "inventoryID")]
    pub inventory_id: String,
    pub altitude: i32,
    pub rotation: i32,
    #[serde(rename = "pieceCoords")]
    pub piece_coords: PieceCoords,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PieceCoords {
    pub q: i32,
    pub r: i32,
    pub s: i32,
}

#[cfg(test)]
mod tests {
    use super::{Component, HexoscapeMapFile};

    #[test]
    fn hexoscape_map_file_round_trips_serialized_layout_keys() {
        let json = r#"{
            "hexMap": {"name": "arena", "hexes": [{"q": 1, "r": -1}]},
            "boardPieces": [{
                "uid": "piece-1",
                "inventoryID": "unit-scout",
                "altitude": 2,
                "rotation": 1,
                "pieceCoords": {"q": 1, "r": -1, "s": 0}
            }]
        }"#;

        let map: HexoscapeMapFile = serde_json::from_str(json).unwrap();
        assert_eq!(map.hex_map.properties["name"], "arena");
        assert_eq!(map.board_pieces.len(), 1);
        assert_eq!(map.board_pieces[0].uid, "piece-1");
        assert_eq!(map.board_pieces[0].inventory_id, "unit-scout");
        assert_eq!(map.board_pieces[0].altitude, 2);
        assert_eq!(map.board_pieces[0].rotation, 1);
        assert_eq!(map.board_pieces[0].piece_coords.q, 1);

        let encoded = serde_json::to_value(map).unwrap();
        assert!(encoded.get("hexMap").is_some());
        assert!(encoded.get("boardPieces").is_some());
        assert!(encoded["boardPieces"][0].get("inventoryID").is_some());
        assert!(encoded["boardPieces"][0].get("pieceCoords").is_some());
    }

    #[test]
    fn hexoscape_map_file_deserializes_starter_unit_components() {
        let json = r#"{
            "hexMap": {},
            "starterUnits": [{
                "id": "UNIT_P1_1",
                "components": {
                    "destructible": {
                        "type": "Destructible",
                        "data": {"current_life": 4, "max_life": 4}
                    }
                }
            }]
        }"#;

        let map: HexoscapeMapFile = serde_json::from_str(json).unwrap();
        assert!(matches!(
            map.starter_units[0].components.get("destructible"),
            Some(Component::Destructible {
                current_life: 4,
                max_life: 4
            })
        ));
    }
}
