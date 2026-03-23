use crate::config::{MqttConfig, MqttIdentity};

pub const TOPIC_SHADOW_GET: &str = "%s/%s/shadow/name/deviceEnv/get";
pub const TOPIC_SHADOW_UPDATE: &str = "%s/%s/shadow/name/deviceEnv/update";
pub const TOPIC_SHADOW_GET_ACCEPTED: &str = "%s/%s/shadow/name/deviceEnv/get/accepted";
pub const TOPIC_SHADOW_GET_REJECTED: &str = "%s/%s/shadow/name/deviceEnv/get/rejected";
pub const TOPIC_SHADOW_UPDATE_ACCEPTED: &str = "%s/%s/shadow/name/deviceEnv/update/accepted";
pub const TOPIC_SHADOW_UPDATE_REJECTED: &str = "%s/%s/shadow/name/deviceEnv/update/rejected";
pub const TOPIC_SHADOW_UPDATE_DELTA: &str = "%s/%s/shadow/name/deviceEnv/update/delta";
pub const TOPIC_RELEASE_STATUS_UPDATE: &str = "%s/%s/update/releaseStatus";
pub const TOPIC_DEVICE_STATUS_UPDATE: &str = "%s/%s/update/deviceStatus";
pub const TOPIC_SCRIPT_UPDATE: &str = "%s/%s/update/script";
pub const TOPIC_DEVICE_STATUS_GET: &str = "%s/%s/get/deviceStatus";
pub const TOPIC_RELEASE_STATUS_GET: &str = "%s/%s/get/releaseStatus";
pub const TOPIC_SCRIPT_GET: &str = "%s/%s/get/script";

fn format_topic(format_str: &str, topic_head: &str, identity: &MqttIdentity) -> String {
    format_str
        .replace("%s", "{}")
        .replacen("{}", topic_head, 1)
        .replacen(
            "{}",
            format!("{}-{}", identity.fleet_id, identity.device_uuid).as_str(),
            1,
        )
}

pub fn shadow_get(config: &MqttConfig) -> String {
    format_topic(TOPIC_SHADOW_GET, &config.topic_head, &config.identity)
}

pub fn shadow_update(config: &MqttConfig) -> String {
    format_topic(TOPIC_SHADOW_UPDATE, &config.topic_head, &config.identity)
}

pub fn shadow_get_accepted(config: &MqttConfig) -> String {
    format_topic(
        TOPIC_SHADOW_GET_ACCEPTED,
        &config.topic_head,
        &config.identity,
    )
}

pub fn shadow_get_rejected(config: &MqttConfig) -> String {
    format_topic(
        TOPIC_SHADOW_GET_REJECTED,
        &config.topic_head,
        &config.identity,
    )
}

pub fn shadow_update_accepted(config: &MqttConfig) -> String {
    format_topic(
        TOPIC_SHADOW_UPDATE_ACCEPTED,
        &config.topic_head,
        &config.identity,
    )
}

pub fn shadow_update_rejected(config: &MqttConfig) -> String {
    format_topic(
        TOPIC_SHADOW_UPDATE_REJECTED,
        &config.topic_head,
        &config.identity,
    )
}

pub fn shadow_update_delta(config: &MqttConfig) -> String {
    format_topic(
        TOPIC_SHADOW_UPDATE_DELTA,
        &config.topic_head,
        &config.identity,
    )
}

pub fn release_status_update(config: &MqttConfig) -> String {
    format_topic(
        TOPIC_RELEASE_STATUS_UPDATE,
        &config.topic_head,
        &config.identity,
    )
}

pub fn device_status_update(config: &MqttConfig) -> String {
    format_topic(
        TOPIC_DEVICE_STATUS_UPDATE,
        &config.topic_head,
        &config.identity,
    )
}

pub fn script_update(config: &MqttConfig) -> String {
    format_topic(TOPIC_SCRIPT_UPDATE, &config.topic_head, &config.identity)
}

pub fn device_status_get(config: &MqttConfig) -> String {
    format_topic(
        TOPIC_DEVICE_STATUS_GET,
        &config.topic_head,
        &config.identity,
    )
}

pub fn release_status_get(config: &MqttConfig) -> String {
    format_topic(
        TOPIC_RELEASE_STATUS_GET,
        &config.topic_head,
        &config.identity,
    )
}

pub fn script_get(config: &MqttConfig) -> String {
    format_topic(TOPIC_SCRIPT_GET, &config.topic_head, &config.identity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::config::{MqttCredentials, MqttIdentity, ScriptConfig, ShadowEnvConfig};

    #[test]
    fn it_builds_expected_script_topics() {
        let cfg = MqttConfig {
            broker_url: "mqtt://localhost:1883".to_string(),
            topic_head: "sensecapmx".to_string(),
            identity: MqttIdentity {
                fleet_id: "12".to_string(),
                device_uuid: "5000001".to_string(),
            },
            credentials: MqttCredentials::default(),
            clean_session: true,
            keep_alive: Duration::from_secs(30),
            report_interval: Duration::from_secs(300),
            script: ScriptConfig::default(),
            shadow_env: ShadowEnvConfig::default(),
        };

        assert_eq!(script_get(&cfg), "sensecapmx/12-5000001/get/script");
        assert_eq!(script_update(&cfg), "sensecapmx/12-5000001/update/script");
    }
}
