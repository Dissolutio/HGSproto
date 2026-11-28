use crate::{
    handshake::{MatchNeeds, Supports, format_versions},
    protocol::Setup,
};
use hex_game_core::{RULES_REVISION, canonical_setup_json};
use serde_json::Value;

const SETUP_CONTEXT: &str = "hexgamesim m4 setup v1";
const MATCH_CONTEXT: &str = "hexgamesim m4 match id v1";
const STATE_CONTEXT: &str = "hexgamesim m4 state v1";
const CHAIN_CONTEXT: &str = "hexgamesim m4 chain v1";

fn append_part(material: &mut Vec<u8>, part: &[u8]) {
    let len = u32::try_from(part.len()).expect("hash part length exceeds u32");
    material.extend_from_slice(&len.to_be_bytes());
    material.extend_from_slice(part);
}

fn derive(context: &str, parts: &[&[u8]]) -> [u8; 32] {
    let mut material = Vec::new();
    for part in parts {
        append_part(&mut material, part);
    }
    blake3::derive_key(context, &material)
}

fn decode_hex(value: &str, expected_len: Option<usize>) -> Result<Vec<u8>, String> {
    if value.len() % 2 != 0 {
        return Err("invalid hex value".into());
    }
    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            digit(pair[0])
                .zip(digit(pair[1]))
                .map(|(high, low)| high * 16 + low)
                .ok_or_else(|| "invalid hex value".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if expected_len.is_some_and(|len| bytes.len() != len) {
        return Err("invalid hex value".into());
    }
    Ok(bytes)
}

pub fn fingerprint(
    canonical_setup: &str,
    house_rule_approvals_json: &str,
    rules_revision: u32,
    dice_scheme: &str,
) -> [u8; 32] {
    let rules_revision = rules_revision.to_be_bytes();
    derive(
        SETUP_CONTEXT,
        &[
            canonical_setup.as_bytes(),
            house_rule_approvals_json.as_bytes(),
            &rules_revision,
            dice_scheme.as_bytes(),
        ],
    )
}

pub fn display_fp(fp: &[u8; 32]) -> String {
    let hex = blake3::Hash::from(*fp).to_hex().to_string().to_uppercase();
    format!(
        "{}-{}-{}-{}",
        &hex[0..4],
        &hex[4..8],
        &hex[8..12],
        &hex[12..16]
    )
}

pub fn match_identity(
    fp: &[u8; 32],
    nonce_host: &str,
    nonce_guest: &str,
) -> Result<[u8; 32], String> {
    let nonce_host = decode_hex(nonce_host, Some(16))?;
    let nonce_guest = decode_hex(nonce_guest, Some(16))?;
    Ok(derive(MATCH_CONTEXT, &[fp, &nonce_host, &nonce_guest]))
}

pub fn state_hash(canonical_state_bytes: &[u8]) -> [u8; 32] {
    derive(STATE_CONTEXT, &[canonical_state_bytes])
}

pub fn chain_next(
    prev: &str,
    seq: u64,
    cmd_id: &str,
    seat: &str,
    token: &str,
    roll_json_or_empty: &str,
    state_hash: &str,
) -> Result<[u8; 32], String> {
    let prev = decode_hex(prev, Some(32))?;
    let cmd_id = decode_hex(cmd_id, Some(16))?;
    let state_hash = decode_hex(state_hash, Some(32))?;
    let seq = u32::try_from(seq)
        .map_err(|_| "sequence exceeds the 4-byte chain encoding".to_string())?
        .to_be_bytes();
    Ok(derive(
        CHAIN_CONTEXT,
        &[
            &prev,
            &seq,
            &cmd_id,
            seat.as_bytes(),
            token.as_bytes(),
            roll_json_or_empty.as_bytes(),
            &state_hash,
        ],
    ))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedSetup {
    pub name: String,
    pub display_fp: String,
    pub house_rules: Vec<Value>,
    pub canonical_text: String,
}

pub fn check_setup_frame(
    frame: &Setup,
    guest_supports: &Supports,
) -> Result<AcceptedSetup, String> {
    let canonical = canonical_setup_json(&frame.setup);
    if let Ok(canonical) = &canonical
        && canonical != &frame.setup
    {
        return Err(
            "Can't join: the host sent a scenario file that is not in standard form.".into(),
        );
    }
    let canonical_text = canonical.map_err(|error| {
        format!("Can't join: this game can't load the host's scenario: {error}.")
    })?;
    let needs = MatchNeeds::from_setup(&canonical_text).map_err(|error| {
        format!("Can't join: this game can't load the host's scenario: {error}.")
    })?;
    let setup: Value = serde_json::from_str(&canonical_text).map_err(|error| {
        format!("Can't join: this game can't load the host's scenario: {error}.")
    })?;
    let house_rules = setup
        .get("house_rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(rule) = house_rules.iter().find(|rule| {
        rule.get("approval")
            .and_then(|approval| approval.get("mode"))
            .and_then(Value::as_str)
            == Some("local_sandbox")
    }) {
        let slot = rule.get("slot").and_then(Value::as_str).unwrap_or("");
        let kind = rule.get("kind").and_then(Value::as_str).unwrap_or("");
        return Err(format!(
            "Can't join: this scenario contains a house rule that was approved only for local play (`{slot}`: `{kind}`). Both players must approve it."
        ));
    }
    let approvals = serde_json::to_string(
        &house_rules
            .iter()
            .filter_map(|rule| rule.get("approval").cloned())
            .collect::<Vec<_>>(),
    )
    .map_err(|error| error.to_string())?;
    let dice_scheme = needs.dice_scheme.as_deref().unwrap_or("");
    let expected_fp = fingerprint(&canonical_text, &approvals, RULES_REVISION, dice_scheme);
    let matches_fp = decode_hex(&frame.fp, Some(32))
        .is_ok_and(|provided| provided.as_slice() == expected_fp.as_slice());
    if !matches_fp {
        return Err("Can't join: the scenario the host sent doesn't match its fingerprint. Ask the host to start the match again.".into());
    }
    check_supports(&needs, guest_supports)?;
    let name = setup
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Ok(AcceptedSetup {
        name,
        display_fp: display_fp(&expected_fp),
        house_rules,
        canonical_text,
    })
}

fn check_supports(needs: &MatchNeeds, supports: &Supports) -> Result<(), String> {
    if !supports.setup_versions.contains(&needs.setup_version) {
        return Err(format!(
            "Can't join: this match needs scenario format version {}, and your game supports {}. Update your game, or ask the host to choose a different scenario.",
            needs.setup_version,
            format_versions(&supports.setup_versions)
        ));
    }
    for schema in &needs.profile_schemas {
        if !supports.profile_schemas.contains(schema) {
            return Err(format!(
                "Can't join: this match uses unit card format version {schema}, and your game supports {}. Update your game, or ask the host to choose a different scenario.",
                format_versions(&supports.profile_schemas)
            ));
        }
    }
    for mode in &needs.approval_modes {
        if !supports.approval_modes.contains(mode) {
            return Err(format!(
                "Can't join: this match uses a kind of house-rule approval your game doesn't know (`{mode}`). Update your game, or ask the host to remove the house rule."
            ));
        }
    }
    if let Some(scheme) = &needs.dice_scheme
        && !supports.dice_schemes.contains(scheme)
    {
        return Err(format!(
            "Can't join: this match rolls dice in a way your game doesn't support (`{scheme}`). Update your game."
        ));
    }
    Ok(())
}

pub fn check_rejoin_fp(saved_fp: &str, host_fp: &str) -> Result<(), String> {
    let saved = decode_hex(saved_fp, Some(32))?;
    let host = decode_hex(host_fp, Some(32))?;
    if saved != host {
        let saved: [u8; 32] = saved.try_into().expect("validated fingerprint length");
        let host: [u8; 32] = host.try_into().expect("validated fingerprint length");
        return Err(format!(
            "Can't rejoin: the host's match uses a different scenario (fingerprint `{}`) from the match saved on this computer (`{}`).",
            display_fp(&host),
            display_fp(&saved)
        ));
    }
    Ok(())
}

pub fn check_start_hash(host_hash: &str, own_hash: &str) -> Result<(), String> {
    if decode_hex(host_hash, Some(32))? != decode_hex(own_hash, Some(32))? {
        return Err("Can't start: this computer built a different starting position from the same scenario. Both players need the same game version.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Message;
    use hex_game_core::{
        canonical_state_json, engine,
        mod_registry::ModRegistry,
        restore_canonical_match,
        setup::{Approval, apply_setup, apply_setup_with_seed},
    };
    use serde_json::json;

    const V1_SETUP: &str = r#"{
      "setup_version":1,"name":"v1",
      "map":{"hexMap":{"name":"test"},"boardPieces":[
        {"uid":"board","inventoryID":"g1","altitude":0,"rotation":0,"pieceCoords":{"q":0,"r":0,"s":0}},
        {"uid":"board2","inventoryID":"g1","altitude":0,"rotation":0,"pieceCoords":{"q":1,"r":0,"s":-1}}
      ],"starterUnits":[
        {"id":"P1","seat":"P1","components":{"tags":["Unit","P1"],"destructible":{"current_life":1,"max_life":1}}},
        {"id":"P2","seat":"P2","components":{"tags":["Unit","P2"],"destructible":{"current_life":1,"max_life":1}}}
      ]},
      "profiles":[{"schema_version":1,"mod_id":"test.profile","display_name":"Test","visual_primitive":"CUBE","components":{"destructible":{"current_life":1,"max_life":1},"tags":["Unit"]}}],
      "roster":[
        {"slot":"P1","seat":"P1","profile":"test.profile","starting_health":1},
        {"slot":"P2","seat":"P2","profile":"test.profile","starting_health":1}
      ]
    }"#;

    fn guest_supports() -> Supports {
        Supports {
            setup_versions: vec![1, 2],
            profile_schemas: vec![1, 2],
            approval_modes: vec!["all_players".into()],
            dice_schemes: vec!["m4-commit-reveal-v1".into()],
            save_records: vec![1, 2, 3],
            match_files: vec![1],
        }
    }

    fn setup_frame(setup: &str) -> Setup {
        let canonical = canonical_setup_json(setup).unwrap();
        let value: Value = serde_json::from_str(&canonical).unwrap();
        let approvals = serde_json::to_string(
            &value["house_rules"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|rule| rule.get("approval").cloned())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let needs = MatchNeeds::from_setup(&canonical).unwrap();
        let fp = fingerprint(
            &canonical,
            &approvals,
            RULES_REVISION,
            needs.dice_scheme.as_deref().unwrap_or(""),
        );
        Setup {
            v: crate::protocol::Version,
            setup: canonical,
            fp: blake3::Hash::from(fp).to_hex().to_string(),
        }
    }

    #[test]
    fn fingerprint_known_answer_and_input_sensitivity() {
        let input = (
            "{\"setup\":1}",
            "[{\"mode\":\"all_players\"}]",
            7,
            "dice-v1",
        );
        let fp = fingerprint(input.0, input.1, input.2, input.3);
        // Reference vector for the length-prefixed encoding of these inputs.
        assert_eq!(
            blake3::Hash::from(fp).to_hex().to_string(),
            "e0523eaa117a8208aad2233bff6d91c0e90e2b0f312923b4c563b196b5785154"
        );
        assert_eq!(fp, fingerprint(input.0, input.1, input.2, input.3));
        assert_ne!(fp, fingerprint("{\"setup\":2}", input.1, input.2, input.3));
        assert_ne!(fp, fingerprint(input.0, "[]", input.2, input.3));
        assert_ne!(fp, fingerprint(input.0, input.1, 8, input.3));
        assert_ne!(fp, fingerprint(input.0, input.1, input.2, "dice-v2"));
    }

    #[test]
    fn length_prefixes_disambiguate_adjacent_parts() {
        assert_ne!(fingerprint("ab", "c", 1, ""), fingerprint("a", "bc", 1, ""));
        let fp = [7; 32];
        assert_ne!(
            match_identity(
                &fp,
                "000102030405060708090a0b0c0d0e0f",
                "101112131415161718191a1b1c1d1e1f"
            )
            .unwrap(),
            match_identity(
                &fp,
                "000102030405060708090a0b0c0d0e0f",
                "1112131415161718191a1b1c1d1e1f20"
            )
            .unwrap()
        );
        let prev = "11".repeat(32);
        let state = "22".repeat(32);
        assert_ne!(
            chain_next(&prev, 1, "33".repeat(16).as_str(), "ab", "c", "", &state).unwrap(),
            chain_next(&prev, 1, "33".repeat(16).as_str(), "a", "bc", "", &state).unwrap()
        );
    }

    #[test]
    fn display_fp_uses_first_eight_bytes_in_uppercase_groups() {
        let fp = core::array::from_fn(|index| index as u8);
        assert_eq!(display_fp(&fp), "0001-0203-0405-0607");
    }

    #[test]
    fn setup_message_has_a_strict_typed_frame_shape() {
        let frame = Message::Setup(setup_frame(V1_SETUP));
        let value = serde_json::to_value(&frame).unwrap();
        assert_eq!(value["type"], "SETUP");
        assert_eq!(serde_json::from_value::<Message>(value).unwrap(), frame);
        assert!(
            serde_json::from_str::<Message>(
                r#"{"type":"SETUP","v":1,"setup":"{}","fp":"00","extra":true}"#
            )
            .is_err()
        );
    }

    #[test]
    fn match_identity_binds_both_nonces_and_rejects_bad_hex() {
        let fp = [0; 32];
        let first = match_identity(&fp, &"11".repeat(16), &"22".repeat(16)).unwrap();
        let second = match_identity(&fp, &"11".repeat(16), &"23".repeat(16)).unwrap();
        let third = match_identity(&fp, &"12".repeat(16), &"22".repeat(16)).unwrap();
        assert_ne!(first, second);
        assert_ne!(first, third);
        assert!(match_identity(&fp, "bad", &"22".repeat(16)).is_err());
    }

    #[test]
    fn chain_next_binds_each_field() {
        let prev = "11".repeat(32);
        let cmd_id = "22".repeat(16);
        let state = "33".repeat(32);
        let base = chain_next(&prev, 4, &cmd_id, "P1", "S", "", &state).unwrap();
        assert_ne!(
            base,
            chain_next(&"12".repeat(32), 4, &cmd_id, "P1", "S", "", &state).unwrap()
        );
        assert_ne!(
            base,
            chain_next(&prev, 5, &cmd_id, "P1", "S", "", &state).unwrap()
        );
        assert_ne!(
            base,
            chain_next(&prev, 4, &"23".repeat(16), "P1", "S", "", &state).unwrap()
        );
        assert_ne!(
            base,
            chain_next(&prev, 4, &cmd_id, "P2", "S", "", &state).unwrap()
        );
        assert_ne!(
            base,
            chain_next(&prev, 4, &cmd_id, "P1", "E", "", &state).unwrap()
        );
        assert_ne!(
            base,
            chain_next(&prev, 4, &cmd_id, "P1", "S", "{}", &state).unwrap()
        );
        assert_ne!(
            base,
            chain_next(&prev, 4, &cmd_id, "P1", "S", "", &"34".repeat(32)).unwrap()
        );
        assert!(chain_next("zz", 4, &cmd_id, "P1", "S", "", &state).is_err());
        assert!(
            chain_next(
                &prev,
                u64::from(u32::MAX) + 1,
                &cmd_id,
                "P1",
                "S",
                "",
                &state
            )
            .is_err()
        );
    }

    #[test]
    fn guest_check_accepts_supported_canonical_setups_and_refuses_exact_cases() {
        let supports = guest_supports();
        let v1 = setup_frame(V1_SETUP);
        let accepted = check_setup_frame(&v1, &supports).unwrap();
        assert_eq!(accepted.name, "v1");
        assert_eq!(accepted.canonical_text, v1.setup);
        assert_eq!(
            accepted.display_fp,
            display_fp(&fingerprint(&v1.setup, "[]", RULES_REVISION, ""))
        );
        assert!(accepted.house_rules.is_empty());
        let demo = include_str!("../../../../docs/examples/m3-dice-demo.setup.json");
        assert!(check_setup_frame(&setup_frame(demo), &supports).is_ok());

        let bare_map_as_v1 = bare_map_as_v1();
        assert!(check_setup_frame(&setup_frame(&bare_map_as_v1), &supports).is_ok());

        let mut tampered = v1.clone();
        tampered.setup = tampered.setup.replace("\"name\":\"v1\"", "\"name\":\"v2\"");
        assert_eq!(
            check_setup_frame(&tampered, &supports).unwrap_err(),
            "Can't join: the scenario the host sent doesn't match its fingerprint. Ask the host to start the match again."
        );
        let invalid_fp = Setup {
            fp: "not-hex".into(),
            ..v1.clone()
        };
        assert_eq!(
            check_setup_frame(&invalid_fp, &supports).unwrap_err(),
            "Can't join: the scenario the host sent doesn't match its fingerprint. Ask the host to start the match again."
        );

        let mut noncanonical = v1.clone();
        noncanonical.setup.push(' ');
        assert_eq!(
            check_setup_frame(&noncanonical, &supports).unwrap_err(),
            "Can't join: the host sent a scenario file that is not in standard form."
        );

        let invalid = Setup {
            setup: r#"{"setup_version":1}"#.into(),
            fp: String::new(),
            ..v1.clone()
        };
        assert_eq!(
            check_setup_frame(&invalid, &supports).unwrap_err(),
            "Can't join: this game can't load the host's scenario: invalid setup: missing field 'name'."
        );

        let mut local_rule: Value = serde_json::from_str(demo).unwrap();
        local_rule["map"]["starterUnits"][0]["components"]["destructible"]["current_life"] =
            json!(11);
        local_rule["map"]["starterUnits"][0]["components"]["destructible"]["max_life"] = json!(11);
        local_rule["rules"]["health_cap"]["max_health"] = json!(10);
        local_rule["house_rules"] = json!([{
            "kind":"allow_over_health_cap",
            "slot":"UNIT_P1_1",
            "name":"Approved local rule",
            "starting_health":11,
            "max_health":11,
            "approval":{"mode":"local_sandbox"}
        }]);
        let frame = setup_frame(&local_rule.to_string());
        assert_eq!(
            check_setup_frame(&frame, &supports).unwrap_err(),
            "Can't join: this scenario contains a house rule that was approved only for local play (`UNIT_P1_1`: `allow_over_health_cap`). Both players must approve it."
        );
    }

    #[test]
    fn setup_frame_checks_declared_guest_supports() {
        let frame = setup_frame(V1_SETUP);
        let mut supports = guest_supports();
        supports.setup_versions.clear();
        assert_eq!(
            check_setup_frame(&frame, &supports).unwrap_err(),
            "Can't join: this match needs scenario format version 1, and your game supports no versions. Update your game, or ask the host to choose a different scenario."
        );
    }

    fn bare_map_as_v1() -> String {
        let board_pieces = (0..6)
            .map(|q| {
                serde_json::json!({
                    "uid":format!("board-{q}"),
                    "inventoryID":"g1",
                    "altitude":0,
                    "rotation":0,
                    "pieceCoords":{"q":q,"r":0,"s":-q}
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "setup_version":1,
            "name":"Built-in bare map",
            "map":{
                "hexMap":{"name":"Built-in bare map"},
                "boardPieces":board_pieces,
                "starterUnits":[]
            },
            "profiles":[],
            "roster":[]
        })
        .to_string()
    }

    #[test]
    fn rejoin_and_start_checks_use_exact_refusal_texts() {
        let saved = [1; 32];
        let host = [2; 32];
        let saved_hex = blake3::Hash::from(saved).to_hex().to_string();
        let host_hex = blake3::Hash::from(host).to_hex().to_string();
        assert_eq!(
            check_rejoin_fp(&saved_hex, &host_hex).unwrap_err(),
            format!(
                "Can't rejoin: the host's match uses a different scenario (fingerprint `{}`) from the match saved on this computer (`{}`).",
                display_fp(&host),
                display_fp(&saved)
            )
        );
        assert_eq!(
            check_start_hash(&host_hex, &saved_hex).unwrap_err(),
            "Can't start: this computer built a different starting position from the same scenario. Both players need the same game version."
        );
        assert!(check_start_hash(&saved_hex, &saved_hex).is_ok());
    }

    fn initial_state_hash(setup: &str, id: &str, mid: &str, seed: u64) -> String {
        let registry = ModRegistry::default();
        let value: Value = serde_json::from_str(setup).unwrap();
        if value["setup_version"] == 2 {
            apply_setup_with_seed(id.into(), setup, &registry, seed, Approval::LocalSandbox)
                .unwrap();
        } else {
            apply_setup(id.into(), setup, &registry).unwrap();
        }
        let hash = state_hash(canonical_state_json(id, mid).unwrap().as_bytes());
        engine::remove_match(id);
        blake3::Hash::from(hash).to_hex().to_string()
    }

    #[test]
    fn canonical_initial_hash_is_match_id_independent_and_restorable() {
        let demo = include_str!("../../../../docs/examples/m3-dice-demo.setup.json");
        for (setup, seed, suffix) in [(V1_SETUP, 0, "v1"), (demo, 0x1234, "v2")] {
            let source = format!("integrity_source_{suffix}");
            let target = format!("integrity_target_{suffix}");
            let canonical_setup = canonical_setup_json(setup).unwrap();
            let source_registry = ModRegistry::default();
            let target_registry = ModRegistry::default();
            if suffix == "v2" {
                apply_setup_with_seed(
                    source.clone(),
                    &canonical_setup,
                    &source_registry,
                    seed,
                    Approval::LocalSandbox,
                )
                .unwrap();
                apply_setup_with_seed(
                    target.clone(),
                    &canonical_setup,
                    &target_registry,
                    seed,
                    Approval::LocalSandbox,
                )
                .unwrap();
            } else {
                apply_setup(source.clone(), &canonical_setup, &source_registry).unwrap();
                apply_setup(target.clone(), &canonical_setup, &target_registry).unwrap();
            }
            let state = canonical_state_json(&source, "shared-mid").unwrap();
            let source_hash = state_hash(state.as_bytes());
            let target_state = canonical_state_json(&target, "shared-mid").unwrap();
            assert_eq!(source_hash, state_hash(target_state.as_bytes()));
            engine::remove_match(&target);
            restore_canonical_match(&target, &canonical_setup, &state, &target_registry).unwrap();
            assert_eq!(
                source_hash,
                state_hash(
                    canonical_state_json(&target, "shared-mid")
                        .unwrap()
                        .as_bytes()
                )
            );
            let different_start_hash = if suffix == "v2" {
                initial_state_hash(
                    setup,
                    &format!("integrity_other_{suffix}"),
                    "shared-mid",
                    seed + 1,
                )
            } else {
                let mut different: Value = serde_json::from_str(setup).unwrap();
                different["map"]["boardPieces"][1]["pieceCoords"]["q"] = json!(2);
                different["map"]["boardPieces"][1]["pieceCoords"]["s"] = json!(-2);
                initial_state_hash(
                    &different.to_string(),
                    &format!("integrity_other_{suffix}"),
                    "shared-mid",
                    seed,
                )
            };
            assert_eq!(
                check_start_hash(
                    &blake3::Hash::from(source_hash).to_hex().to_string(),
                    &different_start_hash
                )
                .unwrap_err(),
                "Can't start: this computer built a different starting position from the same scenario. Both players need the same game version."
            );
            engine::remove_match(&source);
            engine::remove_match(&target);
        }
    }
}
