use std::time::Duration;

#[derive(Clone, Debug)]
pub struct MqttIdentity {
    pub fleet_id: String,
    pub device_uuid: String,
}

#[derive(Clone, Debug)]
pub struct ScriptConfig {
    pub enable: bool,
    pub exec_timeout: Duration,
    pub max_output_bytes: usize,
}

impl Default for ScriptConfig {
    fn default() -> Self {
        Self {
            enable: true,
            exec_timeout: Duration::from_secs(30),
            max_output_bytes: 64 * 1024,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ShadowEnvConfig {
    pub enable: bool,
}

impl Default for ShadowEnvConfig {
    fn default() -> Self {
        Self { enable: true }
    }
}

#[derive(Clone, Debug, Default)]
pub struct MqttCredentials {
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Clone, Debug)]
pub struct MqttConfig {
    pub broker_url: String,
    pub topic_head: String,
    pub identity: MqttIdentity,
    pub credentials: MqttCredentials,
    pub clean_session: bool,
    pub keep_alive: Duration,
    pub report_interval: Duration,
    pub script: ScriptConfig,
    pub shadow_env: ShadowEnvConfig,
}
