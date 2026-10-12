use crate::models::Component;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

const MAX_PROFILE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitTemplate {
    pub schema_version: u32,
    pub mod_id: String,
    pub display_name: String,
    pub visual_primitive: String,
    #[serde(deserialize_with = "deserialize_profile_components")]
    pub components: HashMap<String, Component>,
}

#[derive(Debug, Clone, Default)]
pub struct ModRegistry {
    pub templates: Arc<RwLock<HashMap<String, UnitTemplate>>>,
}

impl ModRegistry {
    pub fn load_batch(&self, batch_json: &str) -> Result<u32, String> {
        let profiles: Vec<Value> = serde_json::from_str(batch_json)
            .map_err(|error| format!("invalid profile batch JSON: {error}"))?;

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
}

fn validate_template(template: &UnitTemplate) -> Result<(), String> {
    if template.schema_version != 1 {
        return Err(format!(
            "unsupported schema_version {}; expected 1",
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

    if let Some(Component::Destructible {
        current_life,
        max_life,
    }) = template.components.get("destructible")
    {
        if !(1..=i32::MAX).contains(max_life) {
            return Err("destructible.max_life must be within 1..=i32::MAX".to_string());
        }
        if !(0..=*max_life).contains(current_life) {
            return Err("destructible.current_life must be within 0..=max_life".to_string());
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

fn deserialize_profile_components<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, Component>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw_components = HashMap::<String, Value>::deserialize(deserializer)?;
    raw_components
        .into_iter()
        .map(|(name, value)| {
            parse_profile_component(&name, value)
                .map(|component| (name.clone(), component))
                .map_err(serde::de::Error::custom)
        })
        .collect()
}

fn parse_profile_component(name: &str, value: Value) -> Result<Component, String> {
    if let Ok(component) = serde_json::from_value::<Component>(value.clone()) {
        if component_matches_key(name, &component) {
            return Ok(component);
        }
        return Err(format!(
            "component key '{name}' does not match its tagged component type"
        ));
    }

    match name {
        "destructible" => serde_json::from_value::<DestructibleProfile>(value)
            .map(|profile| Component::Destructible {
                current_life: profile.current_life,
                max_life: profile.max_life,
            })
            .map_err(|error| format!("invalid destructible component: {error}")),
        "tags" => {
            let labels = match value {
                Value::Array(labels) => labels
                    .into_iter()
                    .map(|label| {
                        label
                            .as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| "tags entries must be strings".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                Value::Object(_) => {
                    serde_json::from_value::<TagsProfile>(value)
                        .map_err(|error| format!("invalid tags component: {error}"))?
                        .labels
                }
                _ => return Err("tags must be an array of strings".to_string()),
            };
            Ok(Component::Tags { labels })
        }
        _ => Err(format!("unsupported component '{name}'")),
    }
}

fn component_matches_key(name: &str, component: &Component) -> bool {
    matches!(
        (name, component),
        ("terrain", Component::Terrain { .. })
            | ("terrain_type", Component::TerrainType { .. })
            | ("life_points", Component::LifePoints { .. })
            | ("destructible", Component::Destructible { .. })
            | ("tags", Component::Tags { .. })
    )
}

#[derive(Deserialize)]
struct DestructibleProfile {
    current_life: i32,
    max_life: i32,
}

#[derive(Deserialize)]
struct TagsProfile {
    labels: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::{ModRegistry, UnitTemplate};
    use crate::models::Component;

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
