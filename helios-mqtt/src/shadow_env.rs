use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyMode {
    Full,
    Delta,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyResult {
    pub next_env: BTreeMap<String, String>,
    pub reported_changes: BTreeMap<String, Option<String>>,
}

fn normalize_env_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(v) if v.is_empty() => None,
        Value::String(v) => Some(v.clone()),
        Value::Number(v) => Some(v.to_string()),
        Value::Bool(v) => Some(v.to_string()),
        _ => Some(value.to_string()),
    }
}

pub fn apply_shadow_env_changes(
    current: &BTreeMap<String, String>,
    incoming: &BTreeMap<String, Value>,
    mode: ApplyMode,
) -> ApplyResult {
    let mut next_env = match mode {
        ApplyMode::Full => BTreeMap::new(),
        ApplyMode::Delta => current.clone(),
    };
    let mut reported_changes = BTreeMap::new();

    for (key, value) in incoming {
        match normalize_env_value(value) {
            Some(next_value) => {
                let has_changed = current.get(key) != Some(&next_value);
                next_env.insert(key.clone(), next_value.clone());
                if has_changed {
                    reported_changes.insert(key.clone(), Some(next_value));
                }
            }
            None => {
                let existed = next_env.remove(key).is_some() || current.contains_key(key);
                if existed {
                    reported_changes.insert(key.clone(), None);
                }
            }
        }
    }

    if mode == ApplyMode::Full {
        for key in current.keys() {
            if !incoming.contains_key(key) {
                reported_changes.insert(key.clone(), None);
            }
        }
    }

    ApplyResult {
        next_env,
        reported_changes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn delta_mode_keeps_missing_keys() {
        let current: BTreeMap<_, _> = [
            ("A".to_string(), "1".to_string()),
            ("B".to_string(), "2".to_string()),
        ]
        .into();
        let incoming: BTreeMap<_, _> = [("A".to_string(), json!("11"))].into();

        let result = apply_shadow_env_changes(&current, &incoming, ApplyMode::Delta);

        assert_eq!(result.next_env.get("A"), Some(&"11".to_string()));
        assert_eq!(result.next_env.get("B"), Some(&"2".to_string()));
    }

    #[test]
    fn full_mode_removes_missing_keys() {
        let current: BTreeMap<_, _> = [
            ("A".to_string(), "1".to_string()),
            ("B".to_string(), "2".to_string()),
        ]
        .into();
        let incoming: BTreeMap<_, _> = [("A".to_string(), json!("11"))].into();

        let result = apply_shadow_env_changes(&current, &incoming, ApplyMode::Full);

        assert_eq!(result.next_env.get("A"), Some(&"11".to_string()));
        assert!(!result.next_env.contains_key("B"));
        assert_eq!(result.reported_changes.get("B"), Some(&None));
    }

    #[test]
    fn null_and_empty_string_mean_delete() {
        let current: BTreeMap<_, _> = [
            ("A".to_string(), "1".to_string()),
            ("B".to_string(), "2".to_string()),
        ]
        .into();
        let incoming: BTreeMap<_, _> =
            [("A".to_string(), Value::Null), ("B".to_string(), json!(""))].into();

        let result = apply_shadow_env_changes(&current, &incoming, ApplyMode::Delta);
        assert!(!result.next_env.contains_key("A"));
        assert!(!result.next_env.contains_key("B"));
        assert_eq!(result.reported_changes.get("A"), Some(&None));
        assert_eq!(result.reported_changes.get("B"), Some(&None));
    }
}
