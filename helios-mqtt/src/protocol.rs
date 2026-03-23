use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

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
}
