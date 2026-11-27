use crate::models::Component;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

const MAX_PROFILE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct UnitTemplate {
    pub schema_version: u32,
    pub mod_id: String,
    pub display_name: String,
    pub visual_primitive: String,
    pub components: HashMap<String, Component>,
}

impl<'de> Deserialize<'de> for UnitTemplate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawTemplate {
            schema_version: u32,
            mod_id: String,
            display_name: String,
            visual_primitive: String,
            components: HashMap<String, Value>,
        }

        let raw = RawTemplate::deserialize(deserializer)?;
        let components = raw
            .components
            .into_iter()
            .map(|(name, value)| {
                if raw.schema_version == 1 && name == "combat" {
                    return Err(serde::de::Error::custom("unsupported unit component 'combat'"));
                }
                parse_profile_component(&name, value)
                    .map(|component| (name, component))
                    .map_err(serde::de::Error::custom)
            })
            .collect::<Result<_, D::Error>>()?;
        Ok(Self {
            schema_version: raw.schema_version,
            mod_id: raw.mod_id,
            display_name: raw.display_name,
            visual_primitive: raw.visual_primitive,
            components,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModRegistry {
    pub templates: Arc<RwLock<HashMap<String, UnitTemplate>>>,
}

impl ModRegistry {
    pub fn load_batch(&self, batch_json: &str) -> Result<u32, String> {
        let profiles: Vec<Value> = serde_json::from_str(batch_json)
            .map_err(|error| format!("invalid profile batch JSON: {error}"))?;
        if profiles.is_empty() {
            return Err("profile batch must include at least one unit profile".to_string());
        }

        let mut parsed_templates = Vec::with_capacity(profiles.len());
        let mut batch_ids = HashSet::with_capacity(profiles.len());

        for (index, profile) in profiles.into_iter().enumerate() {
            let profile_json = serde_json::to_vec(&profile).map_err(|error| {
                format!("profile {index}: could not measure profile size: {error}")
            })?;
            if profile_json.len() > MAX_PROFILE_BYTES {
                return Err(format!("profile {index} exceeds the 64 KiB profile limit"));
            }

            let template: UnitTemplate = serde_json::from_value(profile)
                .map_err(|error| format!("profile {index}: invalid profile: {error}"))?;
            validate_template(&template)
                .map_err(|error| format!("profile {}: {error}", template.mod_id))?;
            if !batch_ids.insert(template.mod_id.clone()) {
                return Err(format!(
                    "profile batch contains duplicate mod_id '{}'",
                    template.mod_id
                ));
            }
            parsed_templates.push(template);
        }

        let mut templates = self
            .templates
            .write()
            .map_err(|_| "mod registry write lock is poisoned".to_string())?;
        if let Some(duplicate) = parsed_templates
            .iter()
            .find(|template| templates.contains_key(&template.mod_id))
        {
            return Err(format!(
                "mod registry already contains mod_id '{}'",
                duplicate.mod_id
            ));
        }

        let loaded_count = u32::try_from(parsed_templates.len()).map_err(|_| {
            "profile batch contains more entries than the registry can report".to_string()
        })?;
        templates.extend(
            parsed_templates
                .into_iter()
                .map(|template| (template.mod_id.clone(), template)),
        );
        Ok(loaded_count)
    }

    pub fn get_template(&self, mod_id: &str) -> Result<UnitTemplate, String> {
        self.templates
            .read()
            .map_err(|_| "mod registry read lock is poisoned".to_string())?
            .get(mod_id)
            .cloned()
            .ok_or_else(|| format!("unknown mod_id '{mod_id}'"))
    }
}

fn validate_template(template: &UnitTemplate) -> Result<(), String> {
    if !matches!(template.schema_version, 1 | 2) {
        return Err(format!(
            "unsupported schema_version {}; expected 1 or 2",
            template.schema_version
        ));
    }
    if !is_valid_mod_id(&template.mod_id) {
        return Err(format!(
            "invalid mod_id '{}'; expected lowercase ASCII domain-style labels",
            template.mod_id
        ));
    }
    if template.display_name.trim().is_empty() {
        return Err("display_name must not be empty".to_string());
    }
    if template.visual_primitive.trim().is_empty() {
        return Err("visual_primitive must not be empty".to_string());
    }

    if template
        .components
        .keys()
        .any(|key| !matches!(key.as_str(), "destructible" | "tags" | "combat"))
    {
        return Err("unit profiles support only destructible and tags components".to_string());
    }
    let Some(Component::Destructible {
        current_life,
        max_life,
    }) = template.components.get("destructible")
    else {
        return Err("unit profiles require a destructible component".to_string());
    };
    if !(1..=i32::MAX).contains(max_life) {
        return Err("destructible.max_life must be within 1..=i32::MAX".to_string());
    }
    if !(1..=*max_life).contains(current_life) {
        return Err("destructible.current_life must be within 1..=max_life".to_string());
    }
    let Some(Component::Tags { labels }) = template.components.get("tags") else {
        return Err("unit profiles require a tags component".to_string());
    };
    if !labels.iter().any(|label| label == "Unit") {
        return Err("tags must include the 'Unit' label".to_string());
    }
    if labels.iter().any(|label| label.trim().is_empty()) {
        return Err("tag labels must not be empty".to_string());
    }
    if labels.iter().collect::<HashSet<_>>().len() != labels.len() {
        return Err("tag labels must not contain duplicates".to_string());
    }
    if template.schema_version == 2 {
        let Some(Component::Combat { attack, defense, damage_per_strike }) = template.components.get("combat") else {
            return Err("unit profiles require a combat component".to_string());
        };
        for (name, value, max) in [
            ("attack", attack, 999),
            ("defense", defense, 999),
            ("damage_per_strike", damage_per_strike, 100),
        ] {
            if !(0..=max).contains(value) {
                return Err(format!("combat.{name} must be within 0..={max}"));
            }
        }
    }
    Ok(())
}

fn is_valid_mod_id(mod_id: &str) -> bool {
    if !mod_id.is_ascii() {
        return false;
    }

    let labels: Vec<&str> = mod_id.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte)
                })
        })
}

fn parse_profile_component(name: &str, value: Value) -> Result<Component, String> {
    match name {
        "combat" => {
            let data = tagged_component_data(&value, "Combat", "combat")?;
            reject_unknown_fields(data, &["attack", "defense", "damage_per_strike"], "combat")?;
            Ok(Component::Combat {
                attack: profile_integer(data, "attack", "combat")?,
                defense: profile_integer(data, "defense", "combat")?,
                damage_per_strike: if data.contains_key("damage_per_strike") {
                    profile_integer(data, "damage_per_strike", "combat")?
                } else {
                    1
                },
            })
        }
        "destructible" => {
            let data = tagged_component_data(&value, "Destructible", "destructible")?;
            reject_unknown_fields(data, &["current_life", "max_life"], "destructible")?;
            Ok(Component::Destructible {
                current_life: profile_integer(data, "current_life", "destructible")?,
                max_life: profile_integer(data, "max_life", "destructible")?,
            })
        }
        "tags" => {
            let labels = if let Some(labels) = value.as_array() {
                labels
                    .into_iter()
                    .map(|label| {
                        label
                            .as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| "tags entries must be strings".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                let data = tagged_component_data(&value, "Tags", "tags")?;
                reject_unknown_fields(data, &["labels"], "tags")?;
                data.get("labels")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "tags.labels must be an array of strings".to_string())?
                    .iter()
                    .map(|label| {
                        label
                            .as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| "tags entries must be strings".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?
            };
            Ok(Component::Tags { labels })
        }
        _ => Err(format!("unsupported unit component '{name}'")),
    }
}

fn tagged_component_data<'a>(
    value: &'a Value,
    expected_type: &str,
    component_name: &str,
) -> Result<&'a serde_json::Map<String, Value>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{component_name} must be an object"))?;
    if object.contains_key("type") {
        reject_unknown_fields(object, &["type", "data"], component_name)?;
        let actual_type = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{component_name}.type must be a string"))?;
        if actual_type != expected_type {
            return Err(format!(
                "{component_name} tagged type must be '{expected_type}'"
            ));
        }
        return object
            .get("data")
            .and_then(Value::as_object)
            .ok_or_else(|| format!("{component_name}.data must be an object"));
    }
    Ok(object)
}

fn reject_unknown_fields(
    object: &serde_json::Map<String, Value>,
    allowed_fields: &[&str],
    object_name: &str,
) -> Result<(), String> {
    if let Some(field) = object
        .keys()
        .find(|field| !allowed_fields.contains(&field.as_str()))
    {
        return Err(format!("unsupported {object_name} field '{field}'"));
    }
    Ok(())
}

fn profile_integer(
    object: &serde_json::Map<String, Value>,
    key: &str,
    object_name: &str,
) -> Result<i32, String> {
    object
        .get(key)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| format!("{object_name}.{key} must be a 32-bit integer"))
}

#[cfg(test)]
mod tests {
    use super::{ModRegistry, UnitTemplate};
    use crate::models::Component;
    use serde_json::Value;

    const VALID_PROFILE: &str = r#"{
        "schema_version": 1,
        "mod_id": "com.community.heavy_mech",
        "display_name": "Goliath Exo-Suit",
        "visual_primitive": "CUBE",
        "components": {
            "destructible": { "max_life": 12, "current_life": 12 },
            "tags": ["Unit", "Heavy", "Armored"]
        }
    }"#;

    fn combat_profile() -> Value {
        let mut profile: Value = serde_json::from_str(VALID_PROFILE).unwrap();
        profile["schema_version"] = 2.into();
        profile["components"]["combat"] = serde_json::json!({
            "attack": 4, "defense": 3, "damage_per_strike": 1
        });
        profile
    }

    #[test]
    fn combat_values_validate_boundaries_and_default() {
        for (field, max) in [("attack", 999), ("defense", 999), ("damage_per_strike", 100)] {
            for value in [serde_json::json!(0), serde_json::json!(max)] {
                let mut profile = combat_profile();
                profile["components"]["combat"][field] = value.clone();
                let registry = ModRegistry::default();
                registry.load_batch(&serde_json::json!([profile]).to_string()).unwrap();
                let template = registry.get_template("com.community.heavy_mech").unwrap();
                assert_eq!(serde_json::to_value(&template.components["combat"]).unwrap()["data"][field], value);
            }
            for value in [serde_json::json!(-1), serde_json::json!(max + 1), serde_json::json!(1.5), Value::Null] {
                let mut profile = combat_profile();
                profile["components"]["combat"][field] = value.clone();
                let error = ModRegistry::default().load_batch(&serde_json::json!([profile]).to_string()).unwrap_err();
                assert!(error.contains(&format!("combat.{field}")), "{field}={value}: {error}");
            }
        }
        let mut profile = combat_profile();
        profile["components"]["combat"].as_object_mut().unwrap().remove("damage_per_strike");
        let registry = ModRegistry::default();
        registry.load_batch(&serde_json::json!([profile]).to_string()).unwrap();
        assert!(matches!(registry.get_template("com.community.heavy_mech").unwrap().components["combat"],
            Component::Combat { damage_per_strike: 1, .. }));
    }

    #[test]
    fn combat_schema_and_strict_fields_are_enforced() {
        let mut v1 = combat_profile();
        v1["schema_version"] = 1.into();
        v1["components"]["combat"] = Value::Null;
        let error = ModRegistry::default().load_batch(&serde_json::json!([v1]).to_string()).unwrap_err();
        assert_eq!(error, "profile 0: invalid profile: unsupported unit component 'combat'");

        let mut missing = combat_profile();
        missing["components"].as_object_mut().unwrap().remove("combat");
        let error = ModRegistry::default().load_batch(&serde_json::json!([missing]).to_string()).unwrap_err();
        assert_eq!(error, "profile com.community.heavy_mech: unit profiles require a combat component");

        let mut unknown = combat_profile();
        unknown["components"]["combat"]["extra"] = 1.into();
        let error = ModRegistry::default().load_batch(&serde_json::json!([unknown]).to_string()).unwrap_err();
        assert!(error.contains("unsupported combat field 'extra'"), "{error}");

        let mut future = combat_profile();
        future["schema_version"] = 3.into();
        let error = ModRegistry::default().load_batch(&serde_json::json!([future]).to_string()).unwrap_err();
        assert!(error.contains("unsupported schema_version 3"), "{error}");

        for field in ["attack", "defense"] {
            let mut profile = combat_profile();
            profile["components"]["combat"].as_object_mut().unwrap().remove(field);
            let error = ModRegistry::default().load_batch(&serde_json::json!([profile]).to_string()).unwrap_err();
            assert!(error.contains(&format!("combat.{field}")), "{error}");
        }
    }

    #[test]
    fn mixed_schema_batches_are_atomic() {
        let registry = ModRegistry::default();
        let mut v1: Value = serde_json::from_str(VALID_PROFILE).unwrap();
        v1["mod_id"] = "com.example.v1".into();
        let mut v2 = combat_profile();
        v2["mod_id"] = "com.example.v2".into();
        assert_eq!(registry.load_batch(&serde_json::json!([v1, v2]).to_string()).unwrap(), 2);
        let before = serde_json::to_value(&*registry.templates.read().unwrap()).unwrap();
        let mut valid = combat_profile();
        valid["mod_id"] = "com.example.new".into();
        let mut invalid = combat_profile();
        invalid["mod_id"] = "com.example.bad".into();
        invalid["components"]["combat"]["attack"] = 1000.into();
        assert!(registry.load_batch(&serde_json::json!([valid, invalid]).to_string()).is_err());
        assert_eq!(serde_json::to_value(&*registry.templates.read().unwrap()).unwrap(), before);
    }

    #[test]
    fn tagged_combat_profile_round_trips() {
        let mut profile = combat_profile();
        profile["components"]["combat"] = serde_json::json!({
            "type": "Combat", "data": {"attack": 4, "defense": 3, "damage_per_strike": 1}
        });
        let registry = ModRegistry::default();
        registry.load_batch(&serde_json::json!([profile]).to_string()).unwrap();
        let template = registry.get_template("com.community.heavy_mech").unwrap();
        let encoded = serde_json::to_value(&template).unwrap();
        assert_eq!(encoded["components"]["combat"], serde_json::json!({
            "type": "Combat", "data": {"attack": 4, "defense": 3, "damage_per_strike": 1}
        }));
        let restored: UnitTemplate = serde_json::from_value(encoded.clone()).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), encoded);
    }

    #[test]
    fn test_mod_registry_atomic_batch_ingestion_and_validation() {
        let registry = ModRegistry::default();
        assert_eq!(
            registry.load_batch(&format!("[{VALID_PROFILE}]")).unwrap(),
            1
        );

        {
            let templates = registry.templates.read().unwrap();
            let template = templates.get("com.community.heavy_mech").unwrap();
            assert_eq!(template.display_name, "Goliath Exo-Suit");
            assert_eq!(template.visual_primitive, "CUBE");
            assert!(matches!(
                template.components.get("destructible"),
                Some(Component::Destructible {
                    current_life: 12,
                    max_life: 12
                })
            ));
            assert!(matches!(
                template.components.get("tags"),
                Some(Component::Tags { labels })
                    if labels == &["Unit", "Heavy", "Armored"]
            ));
        }

        let corrupted_negative_health = VALID_PROFILE.replace(
            r#""max_life": 12, "current_life": 12"#,
            r#""max_life": -2, "current_life": 12"#,
        );
        let mixed_batch = format!("[{VALID_PROFILE},{}]", corrupted_negative_health);
        assert!(registry.load_batch(&mixed_batch).is_err());

        let invalid_id = VALID_PROFILE.replace("com.community.heavy_mech", "Community/Heavy Mech");
        assert!(registry.load_batch(&format!("[{invalid_id}]")).is_err());

        let duplicate_ids = format!("[{VALID_PROFILE},{VALID_PROFILE}]");
        assert!(registry.load_batch(&duplicate_ids).is_err());

        let templates = registry.templates.read().unwrap();
        assert_eq!(templates.len(), 1);
        assert!(templates.contains_key("com.community.heavy_mech"));
    }

    #[test]
    fn unit_profile_schema_rejects_unsupported_and_invalid_fields() {
        let mut profiles = Vec::new();

        let mut profile: Value = serde_json::from_str(VALID_PROFILE).unwrap();
        profile["movement_speed"] = Value::from(4);
        profiles.push((profile, "unknown field `movement_speed`"));

        let mut profile: Value = serde_json::from_str(VALID_PROFILE).unwrap();
        profile["components"]["destructible"]["movement_speed"] = Value::from(4);
        profiles.push((profile, "unsupported destructible field 'movement_speed'"));

        let mut profile: Value = serde_json::from_str(VALID_PROFILE).unwrap();
        profile["components"]["destructible"]["current_life"] = Value::from(0);
        profiles.push((
            profile,
            "destructible.current_life must be within 1..=max_life",
        ));

        let mut profile: Value = serde_json::from_str(VALID_PROFILE).unwrap();
        profile["components"]["tags"] = serde_json::json!(["Heavy"]);
        profiles.push((profile, "tags must include the 'Unit' label"));

        let mut profile: Value = serde_json::from_str(VALID_PROFILE).unwrap();
        profile["components"]["movement"] = serde_json::json!({"speed": 4});
        profiles.push((profile, "unsupported unit component 'movement'"));

        for (profile, expected_error) in profiles {
            let registry = ModRegistry::default();
            let error = registry
                .load_batch(&serde_json::to_string(&vec![profile]).unwrap())
                .unwrap_err();
            assert!(
                error.contains(expected_error),
                "expected {expected_error:?} in {error:?}"
            );
            assert!(registry.templates.read().unwrap().is_empty());
        }
    }

    #[test]
    fn empty_profile_batch_is_rejected() {
        let error = ModRegistry::default().load_batch("[]").unwrap_err();
        assert!(error.contains("at least one unit profile"));
    }

    #[test]
    fn profile_deserialization_accepts_tagged_core_components() {
        let json = r#"{
            "schema_version": 1,
            "mod_id": "com.example.tagged",
            "display_name": "Tagged",
            "visual_primitive": "SPHERE",
            "components": {
                "destructible": {
                    "type": "Destructible",
                    "data": {"current_life": 3, "max_life": 5}
                }
            }
        }"#;
        let template: UnitTemplate = serde_json::from_str(json).unwrap();
        assert!(matches!(
            template.components.get("destructible"),
            Some(Component::Destructible {
                current_life: 3,
                max_life: 5
            })
        ));
    }

    #[test]
    fn profile_larger_than_64_kib_is_rejected() {
        let mut profile: serde_json::Value = serde_json::from_str(VALID_PROFILE).unwrap();
        profile["display_name"] = serde_json::Value::String("x".repeat(64 * 1024));
        let registry = ModRegistry::default();
        assert!(
            registry
                .load_batch(&serde_json::to_string(&vec![profile]).unwrap())
                .is_err()
        );
        assert!(registry.templates.read().unwrap().is_empty());
    }
}
