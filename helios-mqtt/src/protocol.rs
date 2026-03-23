use helios_host_metrics::HostMetricsSnapshot;
use helios_state::LocalState;
use helios_state::models::{Service, ServiceContainerStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceStatusGetPayload {
    #[serde(rename = "requestId")]
    pub request_id: String,
    pub timestamp: i64,
    #[serde(rename = "fleetId")]
    pub fleet_id: String,
    #[serde(rename = "deviceUUID")]
    pub device_uuid: String,
    pub intent: String,
    pub order: NameOnlyOrder,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeviceStatusUpdatePayload {
    #[serde(rename = "requestId")]
    pub request_id: String,
    pub timestamp: i64,
    pub intent: String,
    #[serde(rename = "type")]
    pub response_type: String,
    #[serde(rename = "fleetId")]
    pub fleet_id: String,
    #[serde(rename = "deviceUUID")]
    pub device_uuid: String,
    pub order: DeviceStatusUpdateOrder,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeviceStatusUpdateOrder {
    pub name: String,
    pub value: DeviceStatusValue,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStatusValue {
    pub timestamp_ms: i64,
    pub memory_total_mb: f64,
    pub memory_used_mb: f64,
    pub sd_total_mb: f64,
    pub sd_used_mb: f64,
    pub flash_total_mb: f64,
    pub flash_used_mb: f64,
    pub cpu_temperature_c: f64,
    pub cpu_used_percent: f64,
    pub local_ip: Vec<String>,
    pub public_ip: Vec<String>,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f64,
    pub lora_modem: Option<String>,
    pub lte: Option<u8>,
    pub gps: Option<u8>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseStatusGetPayload {
    #[serde(rename = "requestId")]
    pub request_id: String,
    pub timestamp: i64,
    #[serde(rename = "fleetId")]
    pub fleet_id: String,
    #[serde(rename = "deviceUUID")]
    pub device_uuid: String,
    pub intent: String,
    #[serde(default)]
    pub r#type: Option<String>,
    pub order: ReleaseOrder,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReleaseStatusUpdatePayload {
    #[serde(rename = "requestId")]
    pub request_id: String,
    pub timestamp: i64,
    pub intent: String,
    #[serde(rename = "type")]
    pub response_type: String,
    #[serde(rename = "fleetId")]
    pub fleet_id: String,
    #[serde(rename = "deviceUUID")]
    pub device_uuid: String,
    pub order: ReleaseStatusUpdateOrder,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReleaseStatusUpdateOrder {
    pub name: String,
    pub value: ReleaseStatusValue,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct ReleaseStatusValue {
    pub services: Vec<ReleaseStatusService>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReleaseStatusService {
    pub name: String,
    pub started: bool,
    pub status: String,
    #[serde(rename = "containerName", skip_serializing_if = "Option::is_none")]
    pub container_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NameOnlyOrder {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseOrder {
    pub name: String,
    #[serde(default)]
    pub value: Option<ReleaseOrderValue>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ReleaseOrderValue {
    #[serde(default)]
    pub services: Vec<ReleaseServiceControl>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseServiceControl {
    pub name: String,
    pub action: u8,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScriptRequestPayload {
    #[serde(rename = "requestId")]
    pub request_id: String,
    pub timestamp: i64,
    #[serde(rename = "fleetId")]
    pub fleet_id: String,
    #[serde(rename = "deviceUUID")]
    pub device_uuid: Option<String>,
    pub intent: String,
    pub r#type: String,
    pub script: ScriptRequest,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScriptRequest {
    pub name: String,
    pub version: String,
    #[serde(rename = "type")]
    pub response_type: i32,
    #[serde(rename = "expireTime")]
    pub expire_time: i64,
    pub cmd: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScriptResponsePayload {
    #[serde(rename = "requestId")]
    pub request_id: String,
    pub timestamp: i64,
    pub intent: String,
    #[serde(rename = "type")]
    pub response_type: String,
    #[serde(rename = "fleetId")]
    pub fleet_id: String,
    #[serde(rename = "deviceUUID")]
    pub device_uuid: String,
    pub order: ScriptResponseOrder,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScriptResponseOrder {
    pub name: String,
    pub value: ScriptResponseValue,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScriptResponseValue {
    #[serde(rename = "scriptName")]
    pub script_name: String,
    pub version: String,
    pub code: i32,
    pub msg: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShadowAcceptedPayload {
    pub state: ShadowAcceptedState,
    #[serde(default)]
    pub version: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShadowAcceptedState {
    #[serde(default)]
    pub desired: Option<ShadowEnvState>,
    #[serde(default)]
    pub reported: Option<ShadowEnvState>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShadowDeltaPayload {
    pub state: ShadowEnvState,
    pub version: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShadowEnvState {
    #[serde(default)]
    pub env: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShadowRejectedPayload {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    #[serde(rename = "clientToken")]
    pub client_token: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShadowReportedPayload {
    pub state: ShadowReportState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<i64>,
}

impl DeviceStatusUpdatePayload {
    pub fn from_snapshot(request: &DeviceStatusGetPayload, snapshot: &HostMetricsSnapshot) -> Self {
        Self {
            request_id: request.request_id.clone(),
            timestamp: snapshot.timestamp_ms,
            intent: "order".to_string(),
            response_type: "response".to_string(),
            fleet_id: request.fleet_id.clone(),
            device_uuid: request.device_uuid.clone(),
            order: DeviceStatusUpdateOrder {
                name: "deviceStatus".to_string(),
                value: DeviceStatusValue::from(snapshot),
            },
        }
    }

    pub fn periodic_report(
        identity: &crate::config::MqttIdentity,
        snapshot: &HostMetricsSnapshot,
    ) -> Self {
        Self {
            request_id: String::new(),
            timestamp: snapshot.timestamp_ms,
            intent: "order".to_string(),
            response_type: "report".to_string(),
            fleet_id: identity.fleet_id.clone(),
            device_uuid: identity.device_uuid.clone(),
            order: DeviceStatusUpdateOrder {
                name: "deviceStatus".to_string(),
                value: DeviceStatusValue::from(snapshot),
            },
        }
    }
}

impl ReleaseStatusUpdatePayload {
    pub fn from_local_state(request: &ReleaseStatusGetPayload, local_state: &LocalState) -> Self {
        Self {
            request_id: request.request_id.clone(),
            timestamp: now_millis(),
            intent: "order".to_string(),
            response_type: "response".to_string(),
            fleet_id: request.fleet_id.clone(),
            device_uuid: request.device_uuid.clone(),
            order: ReleaseStatusUpdateOrder {
                name: "releaseStatus".to_string(),
                value: release_status_value_from_local_state(local_state),
            },
        }
    }

    pub fn periodic_report(
        identity: &crate::config::MqttIdentity,
        local_state: &LocalState,
    ) -> Self {
        Self {
            request_id: String::new(),
            timestamp: now_millis(),
            intent: "order".to_string(),
            response_type: "report".to_string(),
            fleet_id: identity.fleet_id.clone(),
            device_uuid: identity.device_uuid.clone(),
            order: ReleaseStatusUpdateOrder {
                name: "releaseStatus".to_string(),
                value: release_status_value_from_local_state(local_state),
            },
        }
    }
}

fn release_status_value_from_local_state(local_state: &LocalState) -> ReleaseStatusValue {
    let mut services = Vec::new();

    for app in local_state.device.apps.values() {
        for release in app.releases.values() {
            for (service_name, service) in &release.services {
                services.push(ReleaseStatusService {
                    name: service_name.clone(),
                    started: service.started,
                    status: service_status(service),
                    container_name: service.container_name.clone(),
                });
            }
        }
    }

    ReleaseStatusValue { services }
}

impl From<&HostMetricsSnapshot> for DeviceStatusValue {
    fn from(snapshot: &HostMetricsSnapshot) -> Self {
        Self {
            timestamp_ms: snapshot.timestamp_ms,
            memory_total_mb: snapshot.memory_total_mb,
            memory_used_mb: snapshot.memory_used_mb,
            sd_total_mb: snapshot.sd_total_mb,
            sd_used_mb: snapshot.sd_used_mb,
            flash_total_mb: snapshot.flash_total_mb,
            flash_used_mb: snapshot.flash_used_mb,
            cpu_temperature_c: snapshot.cpu_temperature_c,
            cpu_used_percent: snapshot.cpu_used_percent,
            local_ip: snapshot.local_ip.clone(),
            public_ip: snapshot.public_ip.clone(),
            latitude: snapshot.latitude,
            longitude: snapshot.longitude,
            altitude: snapshot.altitude,
            lora_modem: snapshot.lora_modem.clone(),
            lte: snapshot.lte,
            gps: snapshot.gps,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ShadowReportState {
    pub reported: ShadowReportedEnv,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShadowReportedEnv {
    pub env: BTreeMap<String, Value>,
}

impl ShadowReportedPayload {
    pub fn from_changes(changes: BTreeMap<String, Option<String>>, version: Option<i64>) -> Self {
        let env = changes
            .into_iter()
            .map(|(key, value)| (key, value.map(Value::String).unwrap_or_else(|| Value::Null)))
            .collect();

        Self {
            state: ShadowReportState {
                reported: ShadowReportedEnv { env },
            },
            version,
        }
    }

    pub fn from_env(env: &BTreeMap<String, String>, version: Option<i64>) -> Self {
        let env = env
            .iter()
            .map(|(key, value)| (key.clone(), Value::String(value.clone())))
            .collect();

        Self {
            state: ShadowReportState {
                reported: ShadowReportedEnv { env },
            },
            version,
        }
    }

    pub fn with_version(&self, version: Option<i64>) -> Self {
        Self {
            state: self.state.clone(),
            version,
        }
    }
}

fn now_millis() -> i64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_millis(0));
    duration.as_millis() as i64
}

fn service_status(service: &Service) -> String {
    match service
        .container
        .as_ref()
        .map(|container| &container.status)
    {
        Some(ServiceContainerStatus::Created) => "created".to_string(),
        Some(ServiceContainerStatus::Running) => "running".to_string(),
        Some(ServiceContainerStatus::Stopping) => "stopping".to_string(),
        Some(ServiceContainerStatus::Stopped) => "stopped".to_string(),
        Some(ServiceContainerStatus::Dead) => "dead".to_string(),
        None if service.started => "running".to_string(),
        None => "stopped".to_string(),
    }
}
