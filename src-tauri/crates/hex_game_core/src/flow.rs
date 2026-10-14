use crate::models::{MatchContext, TurnPhase};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowDefinition {
    pub schema_version: u32,
    pub id: String,
    pub initial_phase: String,
    pub phases: Vec<FlowPhase>,
    pub roster_lock_policy: String,
    pub game_end_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowPhase {
    pub id: String,
    pub order: String,
    pub seats: Vec<String>,
    pub steps: Vec<TurnStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnStep {
    pub id: String,
    pub action: String,
    pub allow_skip: bool,
}

pub static REFERENCE_FLOW: LazyLock<FlowDefinition> = LazyLock::new(|| {
    serde_json::from_str(include_str!("reference_flow.json"))
        .expect("built-in reference flow must be valid")
});

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
        use crate::engine;

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
