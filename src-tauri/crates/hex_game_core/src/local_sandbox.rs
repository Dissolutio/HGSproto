use crate::engine;
use crate::flow::MatchFlow;
use crate::mod_registry::{ModRegistry, UnitTemplate};
use crate::models::GameState;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Version of the saved local sandbox record. Any other value is rejected.
pub const LOCAL_SANDBOX_RECORD_VERSION: u32 = 1;

#[derive(Serialize)]
struct LocalSandboxRecordOut<'a> {
    record_version: u32,
    profiles: Vec<UnitTemplate>,
    snapshot: &'a GameState,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalSandboxRecordIn {
    record_version: u32,
    profiles: Vec<Value>,
    snapshot: Value,
}

/// Serializes the profile registry and the live match into one versioned record.
pub fn export_local_sandbox(match_id: &str, registry: &ModRegistry) -> Result<String, String> {
    let mut profiles: Vec<UnitTemplate> = registry
        .templates
        .read()
        .map_err(|_| "mod registry read lock is poisoned".to_string())?
        .values()
        .cloned()
        .collect();
    profiles.sort_by(|left, right| left.mod_id.cmp(&right.mod_id));
    let matches = engine::ACTIVE_MATCHES
        .lock()
        .map_err(|_| "Match registry is unavailable".to_string())?;
    let snapshot = matches
        .get(match_id)
        .ok_or_else(|| format!("Unknown match: {match_id}"))?;
    serde_json::to_string(&LocalSandboxRecordOut {
        record_version: LOCAL_SANDBOX_RECORD_VERSION,
        profiles,
        snapshot,
    })
    .map_err(|error| format!("Local sandbox record serialization failed: {error}"))
}

/// Validates a saved record as a whole and, only if every check passes, commits its
/// profiles into the (empty) registry and its snapshot into the match registry.
pub fn restore_local_sandbox(
    match_id: &str,
    record_json: &str,
    registry: &ModRegistry,
) -> Result<u32, String> {
    let value: Value = serde_json::from_str(record_json)
        .map_err(|error| format!("Saved sandbox record is not valid JSON: {error}"))?;
    let version = value
        .as_object()
        .ok_or_else(|| "Saved sandbox record must be a JSON object".to_string())?
        .get("record_version")
        .ok_or_else(|| "Saved sandbox record has no record_version".to_string())?;
    if version.as_u64() != Some(u64::from(LOCAL_SANDBOX_RECORD_VERSION)) {
        return Err(format!(
            "Unsupported saved sandbox record_version {version}; expected {LOCAL_SANDBOX_RECORD_VERSION}"
        ));
    }
    let record: LocalSandboxRecordIn = serde_json::from_value(value)
        .map_err(|error| format!("Invalid saved sandbox record: {error}"))?;
    debug_assert_eq!(record.record_version, LOCAL_SANDBOX_RECORD_VERSION);

    let staging = ModRegistry::default();
    let profile_count = if record.profiles.is_empty() {
        0
    } else {
        let batch = serde_json::to_string(&record.profiles)
            .map_err(|error| format!("Could not prepare saved profiles: {error}"))?;
        staging
            .load_batch(&batch)
            .map_err(|error| format!("Saved profiles are invalid: {error}"))?
    };

    let state: GameState = serde_json::from_value(record.snapshot.clone())
        .map_err(|error| format!("Invalid match snapshot: {error}"))?;
    if state.context.match_id != match_id {
        return Err("Saved snapshot match ID does not match the local sandbox".into());
    }
    let mut referenced: Vec<&str> = state
        .entities
        .values()
        .filter_map(|entity| entity.mod_id.as_deref())
        .collect();
    referenced.sort_unstable();
    if let Some(missing) = referenced
        .into_iter()
        .find(|mod_id| staging.get_template(mod_id).is_err())
    {
        return Err(format!(
            "Saved snapshot references profile '{missing}' that is not in the saved profiles"
        ));
    }
    MatchFlow::reference(&state.context)?;
    let snapshot_json = serde_json::to_string(&record.snapshot)
        .map_err(|error| format!("Could not prepare saved snapshot: {error}"))?;
    let staged_templates = std::mem::take(
        &mut *staging
            .templates
            .write()
            .map_err(|_| "mod registry write lock is poisoned".to_string())?,
    );

    let mut templates = registry
        .templates
        .write()
        .map_err(|_| "mod registry write lock is poisoned".to_string())?;
    if !templates.is_empty() {
        return Err("Saved sandbox can only be restored into an empty profile registry".into());
    }
    engine::restore_match_snapshot(match_id, &snapshot_json)?;
    templates.extend(staged_templates);
    Ok(profile_count)
}

#[cfg(test)]
mod tests {
    use super::{export_local_sandbox, restore_local_sandbox};
    use crate::engine;
    use crate::mod_registry::ModRegistry;
    use serde_json::Value;

    const MAP_JSON: &str = r#"{
        "hexMap": {},
        "boardPieces": [{
            "uid": "g7",
            "inventoryID": "g7",
            "altitude": 0,
            "rotation": 0,
            "pieceCoords": {"q": 0, "r": 0, "s": 0}
        }]
    }"#;
    const GOLIATH: &str = "com.example.local_goliath";

    fn goliath_profile() -> String {
        format!(
            r#"[{{
                "schema_version": 1,
                "mod_id": "{GOLIATH}",
                "display_name": "Local Goliath",
                "visual_primitive": "CUBE",
                "components": {{
                    "destructible": {{"current_life": 12, "max_life": 12}},
                    "tags": ["Unit", "Heavy"]
                }}
            }}]"#
        )
    }

    fn snapshot(match_id: &str) -> Value {
        let matches = engine::ACTIVE_MATCHES.lock().unwrap();
        serde_json::to_value(matches.get(match_id).unwrap()).unwrap()
    }

    fn template_count(registry: &ModRegistry) -> usize {
        registry.templates.read().unwrap().len()
    }

    /// Builds a match with two deployed custom units, one of them damaged, and
    /// returns its exported record.
    fn damaged_custom_match_record(match_id: &str) -> String {
        let registry = ModRegistry::default();
        registry.load_batch(&goliath_profile()).unwrap();
        engine::try_initialize_from_hexoscape_with_registry(
            match_id.to_string(),
            MAP_JSON,
            &registry,
        )
        .unwrap();
        engine::deploy_profile_unit(match_id, "UNIT_P1_1", GOLIATH, "P1", 12, &registry).unwrap();
        engine::deploy_profile_unit(match_id, "HERO_P2", GOLIATH, "P2", 12, &registry).unwrap();
        engine::process_action(match_id, "P1", "M:UNIT_P1_2:Q0R2Z0").unwrap();
        engine::process_action(match_id, "P1", "A:UNIT_P1_1:HERO_P2").unwrap();
        export_local_sandbox(match_id, &registry).unwrap()
    }

    fn assert_rejected_leaves_fresh_sandbox(match_id: &str, record: &str, reason: &str) {
        engine::remove_match(match_id);
        let registry = ModRegistry::default();
        let error = restore_local_sandbox(match_id, record, &registry).unwrap_err();
        assert!(error.contains(reason), "unexpected error: {error}");
        assert!(!engine::match_exists(match_id));
        assert_eq!(template_count(&registry), 0);

        engine::try_initialize_from_hexoscape_with_registry(
            match_id.to_string(),
            MAP_JSON,
            &registry,
        )
        .unwrap();
        engine::process_action(match_id, "P1", "M:UNIT_P1_2:Q0R2Z0").unwrap();
        engine::remove_match(match_id);
    }

    fn edited_record(record: &str, edit: impl FnOnce(&mut Value)) -> String {
        let mut value: Value = serde_json::from_str(record).unwrap();
        edit(&mut value);
        value.to_string()
    }

    #[test]
    fn local_sandbox_round_trip_restores_damaged_custom_unit_and_continues_legally() {
        let match_id = "local_sandbox_round_trip";
        let record = damaged_custom_match_record(match_id);
        let before = snapshot(match_id);
        engine::remove_match(match_id);

        let registry = ModRegistry::default();
        assert_eq!(
            restore_local_sandbox(match_id, &record, &registry).unwrap(),
            1
        );
        let restored = snapshot(match_id);
        assert_eq!(restored, before);
        assert_eq!(
            restored["entities"]["HERO_P2"]["components"]["destructible"]["data"]["current_life"],
            11
        );
        assert_eq!(restored["entities"]["HERO_P2"]["mod_id"], GOLIATH);
        assert_eq!(
            restored["entities"]["UNIT_P1_1"]["display_name"],
            "Local Goliath"
        );
        assert_eq!(restored["context"]["active_player"], "P1");
        assert_eq!(restored["context"]["turn_phase"], "end");
        assert_eq!(restored["context"]["setup_locked"], true);
        assert!(registry.get_template(GOLIATH).is_ok());
        let re_exported: Value =
            serde_json::from_str(&export_local_sandbox(match_id, &registry).unwrap()).unwrap();
        assert_eq!(re_exported, serde_json::from_str::<Value>(&record).unwrap());

        assert!(engine::process_action(match_id, "P2", "E").is_err());
        assert!(engine::process_action(match_id, "P1", "M:UNIT_P1_2:Q0R1Z0").is_err());
        assert!(
            engine::deploy_profile_unit(match_id, "UNIT_P1_2", GOLIATH, "P1", 12, &registry)
                .is_err()
        );
        assert_eq!(snapshot(match_id), before);

        engine::process_action(match_id, "P1", "E").unwrap();
        let after = snapshot(match_id);
        assert_eq!(after["context"]["active_player"], "P2");
        assert_eq!(after["context"]["turn_phase"], "movement");
        engine::remove_match(match_id);
    }

    #[test]
    fn local_sandbox_restore_rejects_corrupt_record() {
        let match_id = "local_sandbox_corrupt";
        let record = damaged_custom_match_record(match_id);
        assert_rejected_leaves_fresh_sandbox(match_id, "not json", "not valid JSON");
        assert_rejected_leaves_fresh_sandbox(
            match_id,
            &record[..record.len() / 2],
            "not valid JSON",
        );
        assert_rejected_leaves_fresh_sandbox(match_id, "[]", "must be a JSON object");
        let unknown_field = edited_record(&record, |value| {
            value["extra"] = Value::Bool(true);
        });
        assert_rejected_leaves_fresh_sandbox(match_id, &unknown_field, "unknown field");
        let bad_snapshot = edited_record(&record, |value| {
            value["snapshot"]["context"]["turn_phase"] = Value::from("dance");
        });
        assert_rejected_leaves_fresh_sandbox(match_id, &bad_snapshot, "Invalid match snapshot");
        let bad_profile = edited_record(&record, |value| {
            value["profiles"][0]["display_name"] = Value::from(" ");
        });
        assert_rejected_leaves_fresh_sandbox(match_id, &bad_profile, "Saved profiles are invalid");
        let bad_seat = edited_record(&record, |value| {
            value["snapshot"]["context"]["active_player"] = Value::from("P3");
        });
        assert_rejected_leaves_fresh_sandbox(match_id, &bad_seat, "Flow cannot map active seat");
    }

    #[test]
    fn local_sandbox_restore_rejects_wrong_or_missing_version() {
        let match_id = "local_sandbox_version";
        let record = damaged_custom_match_record(match_id);
        for version in [Value::from(0), Value::from(2), Value::from("1")] {
            let wrong = edited_record(&record, |value| value["record_version"] = version);
            assert_rejected_leaves_fresh_sandbox(
                match_id,
                &wrong,
                "Unsupported saved sandbox record_version",
            );
        }
        let missing = edited_record(&record, |value| {
            value.as_object_mut().unwrap().remove("record_version");
        });
        assert_rejected_leaves_fresh_sandbox(match_id, &missing, "no record_version");
    }

    #[test]
    fn local_sandbox_restore_rejects_snapshot_referencing_unsaved_profile() {
        let match_id = "local_sandbox_unknown_profile";
        let record = damaged_custom_match_record(match_id);
        let without_profiles = edited_record(&record, |value| {
            value["profiles"] = Value::Array(Vec::new());
        });
        assert_rejected_leaves_fresh_sandbox(
            match_id,
            &without_profiles,
            &format!("references profile '{GOLIATH}' that is not in the saved profiles"),
        );
    }

    #[test]
    fn local_sandbox_restore_rejects_profiles_without_snapshot() {
        let match_id = "local_sandbox_missing_snapshot";
        let record = damaged_custom_match_record(match_id);
        let without_snapshot = edited_record(&record, |value| {
            value.as_object_mut().unwrap().remove("snapshot");
        });
        assert_rejected_leaves_fresh_sandbox(
            match_id,
            &without_snapshot,
            "missing field `snapshot`",
        );
    }

    #[test]
    fn local_sandbox_restore_rejects_other_match_and_non_empty_registry() {
        let match_id = "local_sandbox_guards";
        let record = damaged_custom_match_record(match_id);
        engine::remove_match(match_id);
        assert_rejected_leaves_fresh_sandbox(
            "local_sandbox_other_match",
            &record,
            "does not match the local sandbox",
        );

        let registry = ModRegistry::default();
        registry
            .load_batch(&goliath_profile().replace(GOLIATH, "com.example.already_loaded"))
            .unwrap();
        let error = restore_local_sandbox(match_id, &record, &registry).unwrap_err();
        assert!(error.contains("empty profile registry"));
        assert!(!engine::match_exists(match_id));
        assert_eq!(template_count(&registry), 1);
        assert!(registry.get_template(GOLIATH).is_err());
    }

    #[test]
    fn local_sandbox_restore_accepts_record_without_profiles() {
        let match_id = "local_sandbox_no_profiles";
        let registry = ModRegistry::default();
        engine::try_initialize_from_hexoscape_with_registry(
            match_id.to_string(),
            MAP_JSON,
            &registry,
        )
        .unwrap();
        engine::process_action(match_id, "P1", "M:UNIT_P1_2:Q0R2Z0").unwrap();
        let record = export_local_sandbox(match_id, &registry).unwrap();
        let before = snapshot(match_id);
        engine::remove_match(match_id);

        assert_eq!(
            restore_local_sandbox(match_id, &record, &registry).unwrap(),
            0
        );
        assert_eq!(snapshot(match_id), before);
        assert_eq!(before["context"]["turn_phase"], "attack");
        engine::remove_match(match_id);
    }

    #[test]
    fn failed_import_after_restore_leaves_restored_state_intact() {
        let match_id = "local_sandbox_failed_import";
        let record = damaged_custom_match_record(match_id);
        let before = snapshot(match_id);
        engine::remove_match(match_id);
        let registry = ModRegistry::default();
        restore_local_sandbox(match_id, &record, &registry).unwrap();

        let invalid = goliath_profile()
            .replace(GOLIATH, "com.example.local_invalid")
            .replace(r#""current_life": 12"#, r#""current_life": 0"#);
        assert!(registry.load_batch(&invalid).is_err());
        assert!(registry.load_batch(&goliath_profile()).is_err());
        assert!(registry.load_batch("not json").is_err());

        assert_eq!(snapshot(match_id), before);
        assert_eq!(template_count(&registry), 1);
        let re_exported: Value =
            serde_json::from_str(&export_local_sandbox(match_id, &registry).unwrap()).unwrap();
        assert_eq!(re_exported, serde_json::from_str::<Value>(&record).unwrap());
        engine::remove_match(match_id);
    }
}
