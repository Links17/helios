use clap::{Parser, ValueEnum};
use std::num::ParseIntError;
use std::path::PathBuf;
use std::time::Duration;

use crate::api::LocalAddress;
use crate::util::http::Uri;
use crate::util::types::{ApiKey, OperatingSystem, Uuid};

fn parse_duration(s: &str) -> Result<Duration, ParseIntError> {
    let millis: u64 = s.parse()?;
    Ok(Duration::from_millis(millis))
}

fn parse_duration_secs(s: &str) -> Result<Duration, ParseIntError> {
    let secs: u64 = s.parse()?;
    Ok(Duration::from_secs(secs))
}

#[derive(Clone, Debug, ValueEnum)]
pub enum MqttReportStartup {
    Immediate,
    Delayed,
}

#[derive(Clone, Debug, Parser)]
#[command(version, about, long_about = None)] // read from Cargo.toml
pub struct Cli {
    /// Unique identifier for this device
    #[arg(env = "HELIOS_UUID", long = "uuid", value_name = "uuid")]
    pub uuid: Option<Uuid>,

    /// Host OS name and version with metadata, e.g. "balenaOS 6.5.39+rev1"
    #[arg(
        env = "HELIOS_HOST_OS_VERSION",
        long = "host-os-version",
        value_name = "str"
    )]
    pub host_os: Option<OperatingSystem>,

    /// Host OS runtime directory for locks and update scripts, e.g. "/tmp/helios"
    #[arg(
        env = "HELIOS_HOST_RUNTIME_DIR",
        long = "host-runtime-dir",
        value_name = "path"
    )]
    pub host_runtime_dir: Option<PathBuf>,

    /// Local API listen address
    #[arg(
        env = "HELIOS_LOCAL_API_ADDRESS",
        long = "local-api-address",
        value_name = "addr"
    )]
    pub local_api_address: Option<LocalAddress>,

    /// Remote API endpoint URI
    #[arg(
        env = "HELIOS_REMOTE_API_ENDPOINT",
        long = "remote-api-endpoint",
        value_name = "uri"
    )]
    pub remote_api_endpoint: Option<Uri>,

    /// API key for authentication with remote
    #[arg(
        env = "HELIOS_REMOTE_API_KEY",
        long = "remote-api-key",
        value_name = "key",
        requires = "uuid",
        requires = "remote_api_endpoint"
    )]
    pub remote_api_key: Option<ApiKey>,

    /// Remote request timeout in milliseconds
    #[arg(
        env = "HELIOS_REMOTE_REQUEST_TIMEOUT_MS",
        long = "remote-request-timeout-ms",
        value_name = "ms",
        value_parser = parse_duration,
        requires = "remote_api_endpoint"
    )]
    pub remote_request_timeout: Option<Duration>,

    /// Remote target state poll interval in milliseconds
    #[arg(
        env = "HELIOS_REMOTE_POLL_INTERVAL_MS",
        long = "remote-poll-interval-ms",
        value_name = "ms",
        value_parser = parse_duration,
        requires = "remote_api_endpoint"
    )]
    pub remote_poll_interval: Option<Duration>,

    /// Remote rate limiting interval in milliseconds
    #[arg(
        env = "HELIOS_REMOTE_POLL_MIN_INTERVAL_MS",
        long = "remote-poll-min-interval-ms",
        value_name = "ms",
        value_parser = parse_duration,
        requires = "remote_api_endpoint"
    )]
    pub remote_poll_min_interval: Option<Duration>,

    /// Remote target state poll max jitter in milliseconds
    #[arg(
        env = "HELIOS_REMOTE_POLL_MAX_JITTER_MS",
        long = "remote-poll-max-jitter-ms",
        value_name = "ms",
        value_parser = parse_duration,
        requires = "remote_api_endpoint"
    )]
    pub remote_poll_max_jitter: Option<Duration>,

    /// URI of legacy Supervisor API
    #[arg(
        env = "HELIOS_LEGACY_API_ENDPOINT",
        long = "legacy-api-endpoint",
        value_name = "uri",
        requires = "legacy_api_key"
    )]
    pub legacy_api_endpoint: Option<Uri>,

    /// API key for authentication with legacy Supervisor API
    #[arg(
        env = "HELIOS_LEGACY_API_KEY",
        long = "legacy-api-key",
        value_name = "key",
        requires = "legacy_api_endpoint"
    )]
    pub legacy_api_key: Option<ApiKey>,

    /// Provisioning key to use for authenticating with remote during registration
    #[arg(
        env = "HELIOS_PROVISIONING_KEY",
        long = "provisioning-key",
        value_name = "key",
        requires = "remote_api_endpoint",
        requires = "provisioning_fleet",
        requires = "provisioning_device_type"
    )]
    pub provisioning_key: Option<String>,

    /// ID of the fleet to provision this device into
    #[arg(
        env = "HELIOS_PROVISIONING_FLEET",
        long = "provisioning-fleet",
        value_name = "int",
        requires = "provisioning_key"
    )]
    pub provisioning_fleet: Option<u32>, // FIXME: should fleet_uuid

    /// Device type
    #[arg(
        env = "HELIOS_PROVISIONING_DEVICE_TYPE",
        long = "provisioning-device-type",
        value_name = "slug",
        requires = "provisioning_key"
    )]
    pub provisioning_device_type: Option<String>,

    /// Host OS name and version, eg. "balenaOS 6.5.39+rev1"
    #[arg(
        env = "HELIOS_PROVISIONING_OS_VERSION",
        long = "provisioning-os-version",
        value_name = "str",
        requires = "provisioning_key"
    )]
    pub provisioning_os_version: Option<String>,

    /// Supervisor version
    #[arg(
        env = "HELIOS_PROVISIONING_SUPERVISOR_VERSION",
        long = "provisioning-supervisor-version",
        value_name = "str",
        requires = "provisioning_key"
    )]
    pub provisioning_supervisor_version: Option<String>,

    /// MQTT topic head (e.g. "sensecapmx")
    #[arg(
        env = "HELIOS_MQTT_TOPIC_HEAD",
        long = "mqtt-topic-head",
        value_name = "str"
    )]
    pub mqtt_topic_head: Option<String>,

    /// MQTT broker URL, e.g. mqtt://localhost:1883 or mqtts://broker.example.com:8883
    #[arg(
        env = "HELIOS_MQTT_BROKER_URL",
        long = "mqtt-broker-url",
        value_name = "url"
    )]
    pub mqtt_broker_url: Option<String>,

    /// MQTT fleet identifier used by csg topics
    #[arg(
        env = "HELIOS_MQTT_FLEET_ID",
        long = "mqtt-fleet-id",
        value_name = "str"
    )]
    pub mqtt_fleet_id: Option<String>,

    /// MQTT device uuid used by csg topics
    #[arg(
        env = "HELIOS_MQTT_DEVICE_UUID",
        long = "mqtt-device-uuid",
        value_name = "str"
    )]
    pub mqtt_device_uuid: Option<String>,

    /// MQTT username
    #[arg(
        env = "HELIOS_MQTT_USERNAME",
        long = "mqtt-username",
        value_name = "str"
    )]
    pub mqtt_username: Option<String>,

    /// MQTT password
    #[arg(
        env = "HELIOS_MQTT_PASSWORD",
        long = "mqtt-password",
        value_name = "str"
    )]
    pub mqtt_password: Option<String>,

    /// MQTT clean session flag
    #[arg(
        env = "HELIOS_MQTT_CLEAN_SESSION",
        long = "mqtt-clean-session",
        value_name = "bool",
        default_value_t = true
    )]
    pub mqtt_clean_session: bool,

    /// MQTT keep alive in seconds
    #[arg(
        env = "HELIOS_MQTT_KEEP_ALIVE_SEC",
        long = "mqtt-keep-alive-sec",
        value_name = "sec",
        value_parser = parse_duration_secs
    )]
    pub mqtt_keep_alive: Option<Duration>,

    /// MQTT periodic report interval in seconds
    #[arg(
        env = "HELIOS_MQTT_REPORT_INTERVAL_SEC",
        long = "mqtt-report-interval-sec",
        value_name = "sec",
        value_parser = parse_duration_secs
    )]
    pub mqtt_report_interval: Option<Duration>,

    /// MQTT deviceStatus report interval in seconds
    #[arg(
        env = "HELIOS_MQTT_DEVICE_STATUS_REPORT_INTERVAL_SEC",
        long = "mqtt-device-status-report-interval-sec",
        value_name = "sec",
        value_parser = parse_duration_secs
    )]
    pub mqtt_device_status_report_interval: Option<Duration>,

    /// MQTT releaseStatus report interval in seconds
    #[arg(
        env = "HELIOS_MQTT_RELEASE_STATUS_REPORT_INTERVAL_SEC",
        long = "mqtt-release-status-report-interval-sec",
        value_name = "sec",
        value_parser = parse_duration_secs
    )]
    pub mqtt_release_status_report_interval: Option<Duration>,

    /// MQTT deviceStatus periodic reporter startup strategy
    #[arg(
        env = "HELIOS_MQTT_DEVICE_STATUS_REPORT_STARTUP",
        long = "mqtt-device-status-report-startup",
        value_name = "mode",
        value_enum
    )]
    pub mqtt_device_status_report_startup: Option<MqttReportStartup>,

    /// MQTT releaseStatus periodic reporter startup strategy
    #[arg(
        env = "HELIOS_MQTT_RELEASE_STATUS_REPORT_STARTUP",
        long = "mqtt-release-status-report-startup",
        value_name = "mode",
        value_enum
    )]
    pub mqtt_release_status_report_startup: Option<MqttReportStartup>,

    /// Enable script command handling in MQTT runtime
    #[arg(
        env = "HELIOS_MQTT_SCRIPT_ENABLE",
        long = "mqtt-script-enable",
        value_name = "bool",
        default_value_t = true
    )]
    pub mqtt_script_enable: bool,

    /// MQTT script execution timeout in milliseconds
    #[arg(
        env = "HELIOS_MQTT_SCRIPT_TIMEOUT_MS",
        long = "mqtt-script-timeout-ms",
        value_name = "ms",
        value_parser = parse_duration
    )]
    pub mqtt_script_timeout: Option<Duration>,

    /// MQTT script stdout/stderr max size
    #[arg(
        env = "HELIOS_MQTT_SCRIPT_MAX_OUTPUT_BYTES",
        long = "mqtt-script-max-output-bytes",
        value_name = "int"
    )]
    pub mqtt_script_max_output_bytes: Option<usize>,

    /// Enable Shadow env handling in MQTT runtime
    #[arg(
        env = "HELIOS_MQTT_SHADOW_ENV_ENABLE",
        long = "mqtt-shadow-env-enable",
        value_name = "bool",
        default_value_t = true
    )]
    pub mqtt_shadow_env_enable: bool,

    /// Host metrics collection interval in milliseconds
    #[arg(
        env = "HELIOS_HOST_METRICS_INTERVAL_MS",
        long = "host-metrics-interval-ms",
        value_name = "ms",
        value_parser = parse_duration
    )]
    pub host_metrics_interval: Option<Duration>,

    /// Enable CPU temperature collection when the platform supports it
    #[arg(
        env = "HELIOS_HOST_METRICS_CPU_TEMPERATURE_ENABLE",
        long = "host-metrics-cpu-temperature-enable",
        value_name = "bool",
        default_value_t = true
    )]
    pub host_metrics_cpu_temperature_enable: bool,

    /// Optional endpoint returning a plain-text public IP for metrics enrichment
    #[arg(
        env = "HELIOS_HOST_METRICS_PUBLIC_IP_ENDPOINT",
        long = "host-metrics-public-ip-endpoint",
        value_name = "url"
    )]
    pub host_metrics_public_ip_endpoint: Option<String>,

    /// Timeout in milliseconds for the optional public IP endpoint
    #[arg(
        env = "HELIOS_HOST_METRICS_PUBLIC_IP_TIMEOUT_MS",
        long = "host-metrics-public-ip-timeout-ms",
        value_name = "ms",
        value_parser = parse_duration
    )]
    pub host_metrics_public_ip_timeout: Option<Duration>,
}

pub fn parse() -> Cli {
    Parser::parse()
}
