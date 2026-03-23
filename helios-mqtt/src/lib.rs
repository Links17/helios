mod config;
mod protocol;
mod release_control;
mod script;
mod shadow_env;
mod topics;

use std::collections::BTreeMap;

use tokio::sync::{mpsc, watch};
use tracing::{debug, warn};

pub use config::{MqttConfig, MqttIdentity, ScriptConfig, ShadowEnvConfig};
pub use protocol::{
    DeviceStatusGetPayload, ReleaseStatusGetPayload, ScriptRequestPayload, ScriptResponsePayload,
    ShadowAcceptedPayload, ShadowDeltaPayload, ShadowRejectedPayload, ShadowReportedPayload,
};
pub use release_control::{ReleaseAction, ReleaseControlError, seek_requests_from_release_control};
pub use script::{ScriptError, ScriptExecution, execute_script_command, handle_script_request};
pub use shadow_env::{ApplyMode, ApplyResult, apply_shadow_env_changes};

use helios_state::{LocalState, SeekRequest};

#[derive(Debug)]
pub enum InboundMessage {
    DeviceStatusGet(DeviceStatusGetPayload),
    ReleaseStatusGet(ReleaseStatusGetPayload),
    Script(ScriptRequestPayload),
    ShadowAccepted(ShadowAcceptedPayload),
    ShadowDelta(ShadowDeltaPayload),
    ShadowRejected(ShadowRejectedPayload),
}

#[derive(Debug)]
pub enum OutboundMessage {
    ScriptResponse(ScriptResponsePayload),
    ShadowReport(ShadowReportedPayload),
    TriggerDeviceStatusReport,
    TriggerReleaseStatusReport,
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("release control error: {0}")]
    ReleaseControl(#[from] ReleaseControlError),

    #[error("script execution error: {0}")]
    Script(#[from] ScriptError),
}

pub async fn start(
    config: MqttConfig,
    mut inbound_rx: mpsc::Receiver<InboundMessage>,
    seek_tx: watch::Sender<SeekRequest>,
    local_state_rx: watch::Receiver<LocalState>,
    outbound_tx: mpsc::Sender<OutboundMessage>,
) -> Result<(), RuntimeError> {
    let mut current_env = BTreeMap::<String, String>::new();
    let mut shadow_version = None::<i64>;

    while let Some(message) = inbound_rx.recv().await {
        match message {
            InboundMessage::DeviceStatusGet(_) => {
                let _ = outbound_tx
                    .send(OutboundMessage::TriggerDeviceStatusReport)
                    .await;
            }
            InboundMessage::ReleaseStatusGet(payload) => {
                if payload.order.name == "releaseStatus" {
                    let _ = outbound_tx
                        .send(OutboundMessage::TriggerReleaseStatusReport)
                        .await;
                    continue;
                }

                let local_state = local_state_rx.borrow().clone();
                let seek_requests = seek_requests_from_release_control(&local_state, &payload)?;
                for request in seek_requests {
                    if seek_tx.send(request).is_err() {
                        warn!("seek request channel closed while handling release control");
                        break;
                    }
                }
                let _ = outbound_tx
                    .send(OutboundMessage::TriggerReleaseStatusReport)
                    .await;
            }
            InboundMessage::Script(payload) => {
                if let Some(response) =
                    handle_script_request(&payload, &config.identity, &config.script).await?
                {
                    let _ = outbound_tx
                        .send(OutboundMessage::ScriptResponse(response))
                        .await;
                }
            }
            InboundMessage::ShadowAccepted(payload) => {
                if !config.shadow_env.enable {
                    continue;
                }
                shadow_version = payload.version;
                let desired_env = payload
                    .state
                    .desired
                    .map(|state| state.env)
                    .unwrap_or_default();
                let apply_result =
                    apply_shadow_env_changes(&current_env, &desired_env, ApplyMode::Full);
                current_env = apply_result.next_env;
                if !apply_result.reported_changes.is_empty() {
                    let report = ShadowReportedPayload::from_changes(
                        apply_result.reported_changes,
                        shadow_version,
                    );
                    let _ = outbound_tx
                        .send(OutboundMessage::ShadowReport(report))
                        .await;
                }
            }
            InboundMessage::ShadowDelta(payload) => {
                if !config.shadow_env.enable {
                    continue;
                }
                shadow_version = Some(payload.version);
                let apply_result =
                    apply_shadow_env_changes(&current_env, &payload.state.env, ApplyMode::Delta);
                current_env = apply_result.next_env;
                if !apply_result.reported_changes.is_empty() {
                    let report = ShadowReportedPayload::from_changes(
                        apply_result.reported_changes,
                        shadow_version,
                    );
                    let _ = outbound_tx
                        .send(OutboundMessage::ShadowReport(report))
                        .await;
                }
            }
            InboundMessage::ShadowRejected(payload) => {
                if !config.shadow_env.enable {
                    continue;
                }
                debug!(
                    code = payload.code,
                    message = payload.message,
                    "shadow rejected"
                );
                if payload.code == 409 {
                    shadow_version = None;
                }
            }
        }
    }

    Ok(())
}
