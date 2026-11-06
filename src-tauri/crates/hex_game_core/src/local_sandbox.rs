use crate::engine;
use crate::flow::MatchFlow;
use crate::mod_registry::{ModRegistry, UnitTemplate};
use crate::models::GameState;
use crate::setup::{self, RosterEntry, SetupRecord};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// Current version of the saved local sandbox record.
pub const LOCAL_SANDBOX_RECORD_VERSION: u32 = 2;

#[derive(Serialize)]
struct LocalSandboxRecordOut<'a> {
    record_version: u32,
    profiles: Vec<UnitTemplate>,
    snapshot: &'a GameState,
    setup: Option<LocalSandboxSetupOut<'a>>,
}

#[derive(Serialize)]
struct LocalSandboxSetupOut<'a> {
    setup_version: u32,
    name: &'a str,
    map: &'a Value,
    roster: &'a [RosterEntry],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalSandboxRecordV1 {
    record_version: u32,
    profiles: Vec<Value>,
    snapshot: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalSandboxRecordV2 {
    record_version: u32,
    profiles: Vec<Value>,
    snapshot: Value,
    setup: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalSandboxSetupIn {
    setup_version: Value,
    name: Value,
    map: Value,
    roster: Value,
}

/// Serializes the profile registry and the live match into one versioned record.
pub fn export_local_sandbox(match_id: &str, registry: &ModRegistry) -> Result<String, String> {
    let templates = registry
        .templates
        .read()
        .map_err(|_| "mod registry read lock is poisoned".to_string())?;
    let mut profiles: Vec<UnitTemplate> = templates.values().cloned().collect();
    profiles.sort_by(|left, right| left.mod_id.cmp(&right.mod_id));
    let matches = engine::ACTIVE_MATCHES
        .lock()
        .map_err(|_| "Match registry is unavailable".to_string())?;
    let snapshot = matches
        .get(match_id)
        .ok_or_else(|| format!("Unknown match: {match_id}"))?;
    let _flows = engine::match_flows()
        .lock()
        .map_err(|_| "Match flow registry is unavailable".to_string())?;
    let setup_records = engine::setup_records()
        .lock()
        .map_err(|_| "Match setup registry is unavailable".to_string())?;
    let setup = setup_records.get(match_id).map(|record| LocalSandboxSetupOut {
        setup_version: 1,
        name: &record.name,
        map: &record.map,
        roster: &record.roster,
    });
    serde_json::to_string(&LocalSandboxRecordOut {
        record_version: LOCAL_SANDBOX_RECORD_VERSION,
        profiles,
        snapshot,
        setup,
    })
    .map_err(|error| format!("Local sandbox record serialization failed: {error}"))
}

/// Validates a saved record as a whole and, only if every check passes, commits its
/// profiles, snapshot, flow, and setup record together.
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
    let version_number = version.as_u64();
    if !matches!(version_number, Some(1 | 2)) {
        return Err(format!(
            "Unsupported saved sandbox record_version {version}; expected 1 or 2"
        ));
    }
    let (record_version, profiles, snapshot, embedded_setup) = match version_number.unwrap() {
        1 => {
            let record: LocalSandboxRecordV1 = serde_json::from_value(value)
                .map_err(|error| format!("Invalid saved sandbox record: {error}"))?;
            debug_assert_eq!(record.record_version, 1);
            (1, record.profiles, record.snapshot, None)
        }
        2 => {
            let record: LocalSandboxRecordV2 = serde_json::from_value(value)
                .map_err(|error| format!("Invalid saved sandbox record: {error}"))?;
            debug_assert_eq!(record.record_version, 2);
            (2, record.profiles, record.snapshot, Some(record.setup))
        }
        _ => unreachable!("version gate accepts only versions 1 and 2"),
    };

    let staging = ModRegistry::default();
    let profile_count = if profiles.is_empty() {
        0
    } else {
        let batch = serde_json::to_string(&profiles)
            .map_err(|error| format!("Could not prepare saved profiles: {error}"))?;
        staging
            .load_batch(&batch)
            .map_err(|error| format!("Saved profiles are invalid: {error}"))?
    };

    let state: GameState = serde_json::from_value(snapshot)
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
    let setup_record = if record_version == 2 {
        embedded_setup
            .filter(|setup| !setup.is_null())
            .map(|setup| restored_setup_record(setup, &profiles))
            .transpose()?
    } else {
        None
    };
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
    engine::restore_staged_match_snapshot(
        match_id.to_string(),
        state,
        setup_record,
        || templates.extend(staged_templates),
    )?;
    Ok(profile_count)
}

fn restored_setup_record(setup: Value, saved_profiles: &[Value]) -> Result<SetupRecord, String> {
    let setup: LocalSandboxSetupIn = serde_json::from_value(setup)
        .map_err(|error| format!("Invalid saved setup: {error}"))?;
    let profile_ids: BTreeSet<String> = setup
        .roster
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            entry
                .get("profile")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    let mut profiles = Vec::new();
    for profile_id in profile_ids {
        if let Some(profile) = saved_profiles
            .iter()
            .find(|profile| profile.get("mod_id").and_then(Value::as_str) == Some(&profile_id))
        {
            profiles.push(profile.clone());
        }
    }
    let full_setup = serde_json::json!({
        "setup_version": setup.setup_version,
        "name": setup.name,
        "map": setup.map,
        "profiles": profiles,
        "roster": setup.roster,
    });
    let validated = setup::validate_setup(&full_setup.to_string())
        .map_err(|error| format!("Invalid saved setup: {error}"))?;
    let map = serde_json::to_value(validated.map)
        .map_err(|error| format!("Could not prepare saved setup map: {error}"))?;
    Ok(SetupRecord {
        name: validated.name,
        map,
        roster: validated.roster,
    })
}

#[cfg(test)]
mod tests {
    use super::{export_local_sandbox, restore_local_sandbox};
    use crate::engine;
    use crate::flow::MatchFlow;
    use crate::mod_registry::ModRegistry;
    use crate::setup::export_setup;
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

    fn flow_snapshot(match_id: &str) -> MatchFlow {
        engine::match_flows()
            .lock()
            .unwrap()
            .get(match_id)
            .unwrap()
            .clone()
    }

    fn setup_record_snapshot(match_id: &str) -> Option<Value> {
        engine::setup_records()
            .lock()
            .unwrap()
            .get(match_id)
            .map(|record| {
                serde_json::json!({
                    "name": record.name,
                    "map": record.map,
                    "roster": record.roster,
                })
            })
    }

    fn assert_rejected_preserves_existing(
        match_id: &str,
        record: &str,
        reason: &str,
        registry: &ModRegistry,
    ) {
        let before_snapshot = snapshot(match_id);
        let before_flow = flow_snapshot(match_id);
        let before_setup = setup_record_snapshot(match_id);
        let before_registry =
            serde_json::to_value(&*registry.templates.read().unwrap()).unwrap();

        let error = restore_local_sandbox(match_id, record, registry).unwrap_err();
        assert!(error.contains(reason), "unexpected error: {error}");
        assert_eq!(snapshot(match_id), before_snapshot);
        assert_eq!(flow_snapshot(match_id), before_flow);
        assert_eq!(setup_record_snapshot(match_id), before_setup);
        assert_eq!(
            serde_json::to_value(&*registry.templates.read().unwrap()).unwrap(),
            before_registry
        );
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
    fn local_sandbox_version_1_restores_without_a_setup_record() {
        let match_id = "local_sandbox_v1_compatibility";
        let record = edited_record(&damaged_custom_match_record(match_id), |value| {
            value["record_version"] = Value::from(1);
            value.as_object_mut().unwrap().remove("setup");
        });
        let before = snapshot(match_id);
        engine::remove_match(match_id);

        let registry = ModRegistry::default();
        assert_eq!(restore_local_sandbox(match_id, &record, &registry).unwrap(), 1);
        assert_eq!(snapshot(match_id), before);
        assert_eq!(
            export_setup(match_id, &registry).unwrap_err(),
            "setup: this match has no setup record"
        );
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
        for version in [
            Value::from(0),
            Value::from(3),
            Value::from("1"),
            serde_json::json!(2.5),
        ] {
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
        engine::setup_records().lock().unwrap().remove(match_id);
        let record = export_local_sandbox(match_id, &registry).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&record).unwrap()["setup"],
            Value::Null
        );
        let before = snapshot(match_id);
        engine::remove_match(match_id);

        assert_eq!(
            restore_local_sandbox(match_id, &record, &registry).unwrap(),
            0
        );
        assert_eq!(snapshot(match_id), before);
        assert_eq!(before["context"]["turn_phase"], "attack");
        assert_eq!(
            export_setup(match_id, &registry).unwrap_err(),
            "setup: this match has no setup record"
        );
        engine::remove_match(match_id);
    }

    #[test]
    fn local_sandbox_restore_never_heals_snapshot_from_changed_profile() {
        let match_id = "local_sandbox_never_heals";
        let record = edited_record(&damaged_custom_match_record(match_id), |value| {
            value["profiles"][0]["components"]["destructible"]["data"]["current_life"] =
                Value::from(5);
            value["profiles"][0]["components"]["destructible"]["data"]["max_life"] =
                Value::from(5);
            for entry in value["setup"]["roster"].as_array_mut().unwrap() {
                if entry["profile"] == GOLIATH {
                    entry["starting_health"] = Value::from(5);
                }
            }
        });
        engine::remove_match(match_id);

        let registry = ModRegistry::default();
        restore_local_sandbox(match_id, &record, &registry).unwrap();
        let restored = snapshot(match_id);
        assert_eq!(
            restored["entities"]["UNIT_P1_1"]["components"]["destructible"]["data"]["current_life"],
            12
        );
        assert_eq!(
            restored["entities"]["UNIT_P1_1"]["components"]["destructible"]["data"]["max_life"],
            12
        );
        engine::remove_match(match_id);
    }

    #[test]
    fn local_sandbox_restore_refusals_preserve_match_flow_registry_and_setup() {
        let source_id = "local_sandbox_refusal_source";
        let target_id = "local_sandbox_refusal_target";
        let record = edited_record(&damaged_custom_match_record(source_id), |value| {
            value["snapshot"]["context"]["match_id"] = Value::from(target_id);
        });
        engine::remove_match(source_id);
        engine::try_initialize_from_hexoscape_with_registry(
            target_id.to_string(),
            MAP_JSON,
            &ModRegistry::default(),
        )
        .unwrap();
        let registry = ModRegistry::default();

        let invalid_setup = edited_record(&record, |value| {
            value["setup"]["name"] = Value::Bool(true);
        });
        assert_rejected_preserves_existing(
            target_id,
            &invalid_setup,
            "Invalid saved setup",
            &registry,
        );

        let missing_setup_profile = edited_record(&record, |value| {
            value["setup"]["roster"][0]["profile"] = Value::from("com.example.missing");
        });
        assert_rejected_preserves_existing(
            target_id,
            &missing_setup_profile,
            "not in this setup's profiles",
            &registry,
        );

        let missing_setup = edited_record(&record, |value| {
            value.as_object_mut().unwrap().remove("setup");
        });
        assert_rejected_preserves_existing(
            target_id,
            &missing_setup,
            "missing field `setup`",
            &registry,
        );

        for version in [
            Value::from(0),
            Value::from(3),
            Value::from("2"),
            serde_json::json!(2.5),
        ] {
            let unsupported_version = edited_record(&record, |value| {
                value["record_version"] = version;
            });
            assert_rejected_preserves_existing(
                target_id,
                &unsupported_version,
                "expected 1 or 2",
                &registry,
            );
        }

        let version_one_with_setup = edited_record(&record, |value| {
            value["record_version"] = Value::from(1);
        });
        assert_rejected_preserves_existing(
            target_id,
            &version_one_with_setup,
            "unknown field `setup`",
            &registry,
        );

        engine::remove_match(target_id);
    }

    #[test]
    fn local_sandbox_restore_version_1_clears_stale_setup_record() {
        let match_id = "local_sandbox_stale_setup";
        let record = edited_record(&damaged_custom_match_record(match_id), |value| {
            value["record_version"] = Value::from(1);
            value.as_object_mut().unwrap().remove("setup");
        });
        engine::remove_match(match_id);

        engine::try_initialize_from_hexoscape_with_registry(
            match_id.to_string(),
            MAP_JSON,
            &ModRegistry::default(),
        )
        .unwrap();
        assert!(setup_record_snapshot(match_id).is_some());

        let registry = ModRegistry::default();
        restore_local_sandbox(match_id, &record, &registry).unwrap();
        assert!(setup_record_snapshot(match_id).is_none());
        assert_eq!(
            export_setup(match_id, &registry).unwrap_err(),
            "setup: this match has no setup record"
        );
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
