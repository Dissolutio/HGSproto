use crate::flow::MatchFlow;
use serde::Serialize;

pub const MAX_QUERY_TOKEN_BYTES: usize = 1024;

pub(crate) fn query_json(match_id: &str, seat: &str, token: &str) -> String {
    let candidate = if token.is_empty() { None } else { Some(token) };
    match crate::engine::query_actions(match_id, seat, candidate) {
        Ok(query) => match serde_json::to_string(&query) {
            Ok(json) => json,
            Err(error) => error_json(&format!("Query serialization failed: {error}")),
        },
        Err(error) => error_json(&error),
    }
}

pub(crate) fn error_json(message: &str) -> String {
    serde_json::json!({"error": message}).to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    UnknownSeat,
    Finished,
    NotYourTurn,
    WrongStep,
    MalformedAction,
    UnknownEntity,
    NotAUnit,
    WrongOwner,
    OutOfRange,
    OffBoard,
    Occupied,
    InvalidTarget,
    InvalidState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rejection {
    pub code: ReasonCode,
    pub message: String,
}

impl Rejection {
    pub(crate) fn new(code: ReasonCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Move,
    Attack,
    Skip,
    EndTurn,
}

impl ActionKind {
    pub(crate) fn from_token(token: &str) -> Option<Self> {
        if token.starts_with("M:") {
            Some(Self::Move)
        } else if token.starts_with("A:") {
            Some(Self::Attack)
        } else {
            match token {
                "S" => Some(Self::Skip),
                "E" => Some(Self::EndTurn),
                _ => None,
            }
        }
    }

    pub(crate) fn action_id(self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::Attack => "attack",
            Self::Skip => "skip",
            Self::EndTurn => "end_turn",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionAvailability {
    pub action: ActionKind,
    pub available_in_step: bool,
    pub requires_arguments: bool,
    pub rejection: Option<Rejection>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CandidateLegality {
    pub token: String,
    pub legal: bool,
    pub rejection: Option<Rejection>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionQuery {
    pub query_schema_version: u32,
    pub seat: String,
    pub flow: MatchFlow,
    pub actions: Vec<ActionAvailability>,
    pub candidate: Option<CandidateLegality>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{engine, models::TurnPhase};
    use std::ffi::{CStr, CString};

    #[test]
    fn query_uses_execution_validators_and_stable_reasons_without_mutation() {
        let id = "wb_query_reasons";
        for (phase, seat, token, code) in [
            (TurnPhase::Movement, "P2", "S", ReasonCode::NotYourTurn),
            (TurnPhase::Movement, "P9", "S", ReasonCode::UnknownSeat),
            (TurnPhase::Movement, "P1", "E", ReasonCode::WrongStep),
            (TurnPhase::Movement, "P1", "M:missing:Q0R0Z0", ReasonCode::UnknownEntity),
            (TurnPhase::Movement, "P1", "M:HEX_0:Q1R0Z0", ReasonCode::NotAUnit),
            (TurnPhase::Movement, "P1", "M:UNIT_P2_1:Q0R0Z0", ReasonCode::WrongOwner),
            (TurnPhase::Movement, "P1", "M:UNIT_P1_1:Q99R99Z0", ReasonCode::OutOfRange),
            (TurnPhase::Movement, "P1", "M:UNIT_P1_1:Q-2R0Z0", ReasonCode::OffBoard),
            (TurnPhase::Movement, "P1", "M:UNIT_P1_1:Q-1R1Z0", ReasonCode::Occupied),
            (TurnPhase::Attack, "P1", "A:HERO_P1:UNIT_P2_1:9", ReasonCode::MalformedAction),
            (TurnPhase::Attack, "P1", "A:HERO_P1:HEX_0", ReasonCode::InvalidTarget),
            (TurnPhase::Finished, "P1", "E", ReasonCode::Finished),
        ] {
            let mut state = engine::generate_test_sandbox(id.into());
            state.context.turn_phase = phase;
            let before = serde_json::to_value(&state).unwrap();
            engine::store_match(id.into(), state);
            let flow = engine::get_match_flow(id).unwrap();
            let query = engine::query_actions(id, seat, Some(token)).unwrap();
            let check = query.candidate.unwrap();
            assert!(!check.legal);
            let rejection = check.rejection.unwrap();
            assert_eq!(rejection.code, code, "{token}");
            assert_eq!(engine::process_action(id, seat, token).unwrap_err(), rejection.message);
            assert_eq!(engine::get_match_flow(id).unwrap(), flow);
            let matches = engine::ACTIVE_MATCHES.lock().unwrap();
            assert_eq!(serde_json::to_value(&matches[id]).unwrap(), before);
        }
    }

    #[test]
    fn native_wasm_and_core_queries_match_and_do_not_commit_legal_candidates() {
        let id = "wb_query_bridges";
        engine::store_match(id.into(), engine::generate_test_sandbox(id.into()));
        for (token, legal) in [("", None), ("S", Some(true)),
            ("M:UNIT_P1_1:Q0R0Z0", Some(true)), ("E", Some(false))]
        {
            let before = engine::get_match_flow(id).unwrap();
            let core = query_json(id, "Player_1", token);
            let wasm = crate::wasm_bridge::wasm_query_actions(id.into(), "Player_1".into(), token.into());
            let args = [CString::new(id).unwrap(), CString::new("Player_1").unwrap(), CString::new(token).unwrap()];
            let native = unsafe {
                let result = crate::ffi::hgs_query_actions(args[0].as_ptr(), args[1].as_ptr(), args[2].as_ptr());
                let json = CStr::from_ptr(result).to_str().unwrap().to_string();
                crate::ffi::hgs_free_string(result);
                json
            };
            assert_eq!(core, wasm);
            assert_eq!(core, native);
            let query: serde_json::Value = serde_json::from_str(&core).unwrap();
            assert_eq!(query["candidate"]["legal"].as_bool(), legal);
            assert_eq!(query["actions"][0]["requires_arguments"], true);
            assert_eq!(engine::get_match_flow(id).unwrap(), before);
        }
        engine::process_action(id, "P1", "M:UNIT_P1_1:Q0R0Z0").unwrap();
        assert_eq!(engine::get_match_flow(id).unwrap().cursor.step_index, 1);
    }

    #[test]
    fn query_payload_boundary_and_errors_are_explicit() {
        let id = "wb_query_bounds";
        engine::store_match(id.into(), engine::generate_test_sandbox(id.into()));
        assert!(engine::query_actions(id, "P1", Some(&"x".repeat(MAX_QUERY_TOKEN_BYTES))).is_ok());
        assert!(engine::query_actions(id, "P1", Some(&"x".repeat(MAX_QUERY_TOKEN_BYTES + 1))).unwrap_err().contains("1024"));
        assert!(query_json("wb_missing_query", "P1", "").contains("Unknown match"));
        unsafe {
            let raw = crate::ffi::hgs_query_actions(std::ptr::null(), std::ptr::null(), std::ptr::null());
            assert!(CStr::from_ptr(raw).to_str().unwrap().contains("INVALID_ARGUMENT"));
            crate::ffi::hgs_free_string(raw);
        }
    }
}
