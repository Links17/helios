use std::time::Duration;

#[derive(Clone, Debug)]
pub struct MqttIdentity {
    pub fleet_id: String,
    pub device_uuid: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportStartup {
    Immediate,
    Delayed,
}

impl Default for ReportStartup {
    fn default() -> Self {
        Self::Delayed
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ReportConfig {
    pub interval: Duration,
    pub startup: ReportStartup,
}

impl Default for ReportConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(300),
            startup: ReportStartup::default(),
        }
    }
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
    pub device_status_report: ReportConfig,
    pub release_status_report: ReportConfig,
    pub script: ScriptConfig,
    pub shadow_env: ShadowEnvConfig,
}
