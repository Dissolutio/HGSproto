use crate::models::{MatchContext, TurnPhase};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowDefinition {
    pub schema_version: u32,
    pub id: String,
    pub initial_phase: String,
    pub phases: Vec<FlowPhase>,
    pub roster_lock_policy: String,
    pub game_end_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowPhase {
    pub id: String,
    pub order: String,
    pub seats: Vec<String>,
    pub steps: Vec<TurnStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnStep {
    pub id: String,
    pub action: String,
    pub allow_skip: bool,
}

pub const MAX_FLOW_BYTES: usize = 1024;
pub const MAX_IDENTIFIER_BYTES: usize = "reference_first_accepted_action".len();
pub const MAX_PHASES: usize = 1;
pub const MAX_STEPS: usize = 3;
const REFERENCE_JSON: &str = include_str!("reference_flow.json");

pub static REFERENCE_FLOW: LazyLock<FlowDefinition> = LazyLock::new(|| {
    let definition: FlowDefinition = serde_json::from_str(REFERENCE_JSON)
        .expect("built-in reference flow must parse");
    validate_limits(&definition).expect("built-in reference flow must fit its limits");
    definition
});

pub fn validate_definition(json: &str) -> Result<FlowDefinition, String> {
    if json.len() > MAX_FLOW_BYTES {
        return Err(format!("Flow definition exceeds {MAX_FLOW_BYTES} bytes"));
    }
    let definition: FlowDefinition = serde_json::from_str(json)
        .map_err(|error| format!("Invalid flow definition: {error}"))?;
    validate_limits(&definition)?;
    if definition.schema_version != 1 {
        return Err(format!("Unsupported flow schema version: {}", definition.schema_version));
    }
    if definition != *REFERENCE_FLOW {
        return Err("Only the built-in reference flow is supported: reference/play, P1 then P2, cycle, move/attack/end_turn steps, reference skip and policy settings".into());
    }
    Ok(definition)
}

fn validate_limits(definition: &FlowDefinition) -> Result<(), String> {
    if definition.phases.len() != MAX_PHASES {
        return Err(format!("Flow requires exactly {MAX_PHASES} phase"));
    }
    let phase = &definition.phases[0];
    if phase.steps.len() != MAX_STEPS {
        return Err(format!("Flow requires exactly {MAX_STEPS} turn steps"));
    }
    if phase.seats.len() != 2 {
        return Err("Flow requires exactly two seats".into());
    }
    for id in [&definition.id, &definition.initial_phase, &definition.roster_lock_policy,
        &definition.game_end_policy, &phase.id, &phase.order]
        .into_iter()
        .chain(phase.seats.iter())
        .chain(phase.steps.iter().flat_map(|step| [&step.id, &step.action]))
    {
        if id.is_empty() || id.len() > MAX_IDENTIFIER_BYTES {
            return Err(format!("Flow identifier must contain 1..={MAX_IDENTIFIER_BYTES} bytes"));
        }
        if !id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
            return Err(format!("Invalid flow identifier: {id}"));
        }
    }
    Ok(())
}

pub(crate) fn reject_metadata_flow(properties: &serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
    for key in ["flow", "flow_definition", "flow_schema_version", "stages", "active_players",
        "activePlayers", "phases", "turn_order", "turn_steps", "scripts", "expressions", "timers"]
    {
        if properties.contains_key(key) {
            return Err(format!("Unsupported scenario flow declaration '{key}': only the implicit built-in reference flow is supported"));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FlowCursor {
    pub seat_index: usize,
    pub step_index: usize,
    pub finished: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MatchFlow {
    pub definition: &'static FlowDefinition,
    pub cursor: FlowCursor,
}

impl MatchFlow {
    pub fn reference(context: &MatchContext) -> Result<Self, String> {
        Self::from_context(&REFERENCE_FLOW, context)
    }

    pub(crate) fn from_context(
        definition: &'static FlowDefinition,
        context: &MatchContext,
    ) -> Result<Self, String> {
        let phase = &definition.phases[0];
        let seat = match context.active_player.as_str() {
            "Player_1" => "P1",
            "Player_2" => "P2",
            seat => seat,
        };
        let seat_index = phase.seats.iter().position(|candidate| candidate == seat)
            .ok_or_else(|| format!("Flow cannot map active seat: {}", context.active_player))?;
        let finished = context.turn_phase == TurnPhase::Finished;
        let step_index = if finished {
            phase.steps.len()
        } else {
            phase.steps.iter().position(|step| step.id == phase_name(context.turn_phase))
                .ok_or_else(|| "Flow cannot map current turn step".to_string())?
        };
        Ok(Self {
            definition,
            cursor: FlowCursor { seat_index, step_index, finished },
        })
    }

    pub(crate) fn advance(&self) -> Result<FlowCursor, String> {
        let phase = &self.definition.phases[0];
        if self.cursor.finished {
            return Err("Match is finished".into());
        }
        let mut cursor = self.cursor.clone();
        cursor.step_index += 1;
        if cursor.step_index == phase.steps.len() {
            cursor.step_index = 0;
            cursor.seat_index = (cursor.seat_index + 1) % phase.seats.len();
        }
        Ok(cursor)
    }

    pub(crate) fn project(&self, context: &mut MatchContext) {
        let phase = &self.definition.phases[0];
        let current_seat = match context.active_player.as_str() {
            "Player_1" => "P1",
            "Player_2" => "P2",
            seat => seat,
        };
        let next_seat = &phase.seats[self.cursor.seat_index];
        if current_seat != next_seat {
            context.active_player = next_seat.clone();
        }
        context.turn_phase = if self.cursor.finished {
            TurnPhase::Finished
        } else {
            match phase.steps[self.cursor.step_index].id.as_str() {
                "movement" => TurnPhase::Movement,
                "attack" => TurnPhase::Attack,
                "end" => TurnPhase::End,
                _ => unreachable!("validated reference step"),
            }
        };
    }

    pub(crate) fn finish(&mut self) {
        self.cursor.finished = true;
        self.cursor.step_index = self.definition.phases[0].steps.len();
    }
}

fn phase_name(phase: TurnPhase) -> &'static str {
    match phase {
        TurnPhase::Movement => "movement",
        TurnPhase::Attack => "attack",
        TurnPhase::End => "end",
        TurnPhase::Finished => "finished",
    }
}

#[cfg(test)]
mod tests {
        use super::*;
        use crate::engine;

        #[test]
        fn legacy_restore_continues_all_four_turn_states_without_reapplying_effects() {
            for (index, phase) in [TurnPhase::Movement, TurnPhase::Attack,
                TurnPhase::End, TurnPhase::Finished].into_iter().enumerate()
            {
                let id = format!("wb_restore_{index}");
                let mut state = engine::generate_test_sandbox(id.clone());
                state.context.active_player = "Player_1".into();
                state.context.turn_phase = phase;
                state.context.setup_locked = index != 0;
                if let Some(crate::models::Component::Destructible { current_life, .. }) =
                    state.entities.get_mut("HERO_P1").unwrap().components.get_mut("destructible")
                {
                    *current_life = 5;
                }
                if phase == TurnPhase::Finished {
                    state.entities.retain(|id, _| !id.contains("P2"));
                    state.context.winner = Some("P1".into());
                }
                let before = serde_json::to_value(&state).unwrap();
                engine::restore_match_snapshot(&id, &before.to_string()).unwrap();
                {
                    let matches = engine::ACTIVE_MATCHES.lock().unwrap();
                    assert_eq!(serde_json::to_value(&matches[&id]).unwrap(), before);
                }
                let flow = engine::get_match_flow(&id).unwrap();
                assert_eq!(flow.cursor.step_index, index);
                assert_eq!(flow.cursor.finished, phase == TurnPhase::Finished);
                assert_eq!(flow.definition.schema_version, 1);
                match phase {
                    TurnPhase::Movement => engine::process_action(&id, "Player_1", "S").unwrap(),
                    TurnPhase::Attack => {
                        engine::process_action(&id, "P1", "A:HERO_P1:UNIT_P2_1").unwrap();
                        let matches = engine::ACTIVE_MATCHES.lock().unwrap();
                        let after = serde_json::to_value(&matches[&id]).unwrap();
                        assert_eq!(after["entities"]["UNIT_P2_1"]["components"]["destructible"]["data"]["current_life"], 3);
                        assert_eq!(after["context"]["active_player"], "Player_1");
                    }
                    TurnPhase::End => {
                        engine::process_action(&id, "P1", "E").unwrap();
                        assert_eq!(engine::get_match_flow(&id).unwrap().cursor.seat_index, 1);
                    }
                    TurnPhase::Finished => {
                        assert!(engine::process_action(&id, "P1", "E").is_err());
                        assert_eq!(engine::get_match_flow(&id).unwrap(), flow);
                    }
                }
            }
        }

        #[test]
        fn rejected_commands_preserve_entities_and_entire_flow_record() {
            let id = "wb_rejection_atomicity";
            for (phase, seat, action) in [
                (TurnPhase::Movement, "P2", "S"),
                (TurnPhase::Movement, "P1", "A:HERO_P1:UNIT_P2_1"),
                (TurnPhase::Movement, "P1", "M:UNIT_P2_1:Q0R0Z0"),
                (TurnPhase::Movement, "P1", "M:UNIT_P1_1:Q99R99Z0"),
                (TurnPhase::Attack, "P1", "M:UNIT_P1_1:Q0R0Z0"),
                (TurnPhase::Attack, "P1", "A:UNIT_P1_1:UNIT_P2_1"),
                (TurnPhase::Attack, "P1", "A:HERO_P1:UNIT_P2_1:9"),
                (TurnPhase::End, "P1", "S"),
            ] {
                let mut state = engine::generate_test_sandbox(id.into());
                state.context.turn_phase = phase;
                let before = serde_json::to_value(&state).unwrap();
                engine::store_match(id.into(), state);
                let flow = engine::get_match_flow(id).unwrap();
                assert!(engine::process_action(id, seat, action).is_err(), "{action}");
                assert_eq!(engine::get_match_flow(id).unwrap(), flow, "{action}");
                let matches = engine::ACTIVE_MATCHES.lock().unwrap();
                assert_eq!(serde_json::to_value(&matches[id]).unwrap(), before, "{action}");
            }
        }

        #[test]
        fn flow_limits_accept_exact_boundaries_and_reject_over_limit() {
            assert!(REFERENCE_JSON.len() <= MAX_FLOW_BYTES);
            let mut exact = REFERENCE_JSON.to_string();
            exact.push_str(&" ".repeat(MAX_FLOW_BYTES - exact.len()));
            assert!(validate_definition(&exact).is_ok());
            assert!(validate_definition(&(exact + " ")).unwrap_err().contains("bytes"));
            assert_eq!(REFERENCE_FLOW.roster_lock_policy.len(), MAX_IDENTIFIER_BYTES);
            let mut value = serde_json::to_value(&*REFERENCE_FLOW).unwrap();
            value["roster_lock_policy"] = "a".repeat(MAX_IDENTIFIER_BYTES + 1).into();
            assert!(validate_definition(&value.to_string()).unwrap_err().contains("identifier"));
            assert_eq!(REFERENCE_FLOW.phases.len(), MAX_PHASES);
            let mut value = serde_json::to_value(&*REFERENCE_FLOW).unwrap();
            let phase = value["phases"][0].clone();
            value["phases"].as_array_mut().unwrap().push(phase);
            // Compact JSON keeps this structural test below the byte limit.
            assert!(validate_definition(&value.to_string()).unwrap_err().contains("phase"));
            assert_eq!(REFERENCE_FLOW.phases[0].steps.len(), MAX_STEPS);
            let mut value = serde_json::to_value(&*REFERENCE_FLOW).unwrap();
            let step = value["phases"][0]["steps"][0].clone();
            value["phases"][0]["steps"].as_array_mut().unwrap().push(step);
            assert!(validate_definition(&value.to_string()).unwrap_err().contains("turn steps"));
        }

        #[test]
        fn unsupported_flow_declarations_are_rejected() {
            for (field, replacement) in [
                ("schema_version", serde_json::json!(2)),
                ("id", serde_json::json!("custom")),
                ("initial_phase", serde_json::json!("setup")),
                ("game_end_policy", serde_json::json!("score")),
                ("stages", serde_json::json!({})),
                ("activePlayers", serde_json::json!(["P1", "P2"])),
                ("script", serde_json::json!("execute()")),
                ("expression", serde_json::json!("turn + 1")),
                ("timer", serde_json::json!(10)),
            ] {
                let mut value = serde_json::to_value(&*REFERENCE_FLOW).unwrap();
                value[field] = replacement;
                assert!(validate_definition(&value.to_string()).is_err(), "{field}");
            }
            for (field, replacement) in [
                ("seats", serde_json::json!(["P1", "P2", "P3"])),
                ("seats", serde_json::json!(["P2", "P1"])),
                ("seats", serde_json::json!(["P1", "P1"])),
                ("order", serde_json::json!("once")),
                ("next", serde_json::json!({"expression": "choose()"})),
                ("stages", serde_json::json!({})),
            ] {
                let mut value = serde_json::to_value(&*REFERENCE_FLOW).unwrap();
                value["phases"][0][field] = replacement;
                assert!(validate_definition(&value.to_string()).is_err(), "{field}");
            }
            for (field, replacement) in [
                ("action", serde_json::json!("deploy")),
                ("allow_skip", serde_json::json!(false)),
                ("budget", serde_json::json!(2)),
                ("script", serde_json::json!("run()")),
            ] {
                let mut value = serde_json::to_value(&*REFERENCE_FLOW).unwrap();
                value["phases"][0]["steps"][0][field] = replacement;
                assert!(validate_definition(&value.to_string()).is_err(), "{field}");
            }
        }

        #[test]
        fn scenario_metadata_cannot_enable_or_hide_flow_declarations() {
            let registry = crate::mod_registry::ModRegistry::default();
            for json in [
                r#"{"hexMap":{},"flow":{"schema_version":2}}"#,
                r#"{"hexMap":{"flow":{"schema_version":1}}}"#,
                r#"{"hexMap":{"activePlayers":["P1","P2"]}}"#,
            ] {
                let result = engine::try_initialize_from_hexoscape_with_registry(
                    "wb_invalid_metadata".into(), json, &registry);
                assert!(result.unwrap_err().contains("flow declaration"));
                assert!(!engine::match_exists("wb_invalid_metadata"));
            }
        }

        #[test]
        fn reference_schedule_advances_with_unchanged_public_snapshot() {
            let id = "wb_reference_schedule";
            engine::store_match(id.into(), engine::generate_test_sandbox(id.into()));
            for (seat, token, expected_seat, expected_step) in [
                ("P1", "S", "P1", 1),
                ("P1", "S", "P1", 2),
                ("P1", "E", "P2", 0),
                ("P2", "S", "P2", 1),
                ("P2", "S", "P2", 2),
                ("P2", "E", "P1", 0),
            ] {
                engine::process_action(id, seat, token).unwrap();
                let flow = engine::get_match_flow(id).unwrap();
                assert_eq!(flow.cursor.step_index, expected_step);
                assert_eq!(flow.definition.phases[0].seats[flow.cursor.seat_index], expected_seat);
                let matches = engine::ACTIVE_MATCHES.lock().unwrap();
                let json = serde_json::to_value(&matches[id]).unwrap();
                assert_eq!(json["context"]["active_player"], expected_seat);
                assert_eq!(json["context"].as_object().unwrap().len(), 4);
                assert_eq!(json.as_object().unwrap().len(), 2);
        }
    }
}
