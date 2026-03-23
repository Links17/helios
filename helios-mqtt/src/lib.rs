mod client;
mod config;
mod env_store;
mod protocol;
mod release_control;
mod reporter;
mod script;
mod shadow_env;
mod topics;

use std::collections::BTreeMap;
use std::time::Duration;

use helios_host_metrics::{HostMetricsSnapshot, MetricsTrigger};
use serde::Serialize;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;
use tracing::{debug, warn};

pub use client::{ClientError, start_client};
pub use config::{
    MqttConfig, MqttCredentials, MqttIdentity, ReportConfig, ReportStartup, ScriptConfig,
    ShadowEnvConfig,
};
pub use env_store::{PersistedShadowEnv, ShadowEnvStore};
pub use protocol::{
    DeviceStatusGetPayload, DeviceStatusUpdatePayload, ReleaseStatusGetPayload,
    ReleaseStatusUpdatePayload, ScriptRequestPayload, ScriptResponsePayload, ShadowAcceptedPayload,
    ShadowDeltaPayload, ShadowRejectedPayload, ShadowReportedPayload,
};
pub use release_control::{ReleaseAction, ReleaseControlError, seek_requests_from_release_control};
pub use reporter::{start_device_status_reporter, start_release_status_reporter};
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
    ShadowGetRejected(ShadowRejectedPayload),
    ShadowUpdateRejected(ShadowRejectedPayload),
}

#[derive(Debug)]
pub enum OutboundMessage {
    Publish(PublishMessage),
    SchedulePublish(ScheduledPublish),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishMessage {
    pub topic: String,
    pub payload: String,
}

impl PublishMessage {
    pub fn raw(topic: String, payload: impl Into<String>) -> Self {
        Self {
            topic,
            payload: payload.into(),
        }
    }

    pub fn json<T>(topic: String, payload: &T) -> Result<Self, RuntimeError>
    where
        T: Serialize,
    {
        Ok(Self {
            topic,
            payload: serde_json::to_string(payload)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledPublish {
    pub delay: Duration,
    pub message: PublishMessage,
}

impl ScheduledPublish {
    pub fn json<T>(delay: Duration, topic: String, payload: &T) -> Result<Self, RuntimeError>
    where
        T: Serialize,
    {
        Ok(Self {
            delay,
            message: PublishMessage::json(topic, payload)?,
        })
    }

    pub fn raw(delay: Duration, topic: String, payload: impl Into<String>) -> Self {
        Self {
            delay,
            message: PublishMessage::raw(topic, payload),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("release control error: {0}")]
    ReleaseControl(#[from] ReleaseControlError),

    #[error("script execution error: {0}")]
    Script(#[from] ScriptError),

    #[error("failed to serialize outbound payload: {0}")]
    Serialize(#[from] serde_json::Error),

    #[error("shadow env persistence error: {0}")]
    ShadowEnvStore(#[from] helios_store::Error),
}

pub async fn start(
    config: MqttConfig,
    shadow_env_store: Option<ShadowEnvStore>,
    mut inbound_rx: mpsc::Receiver<InboundMessage>,
    seek_tx: watch::Sender<SeekRequest>,
    local_state_rx: watch::Receiver<LocalState>,
    metrics_trigger_tx: mpsc::Sender<MetricsTrigger>,
    mut metrics_rx: watch::Receiver<HostMetricsSnapshot>,
    outbound_tx: mpsc::Sender<OutboundMessage>,
) -> Result<(), RuntimeError> {
    let persisted_shadow_env = if let Some(store) = shadow_env_store.as_ref() {
        store.load().await?
    } else {
        PersistedShadowEnv::default()
    };
    let mut current_env = persisted_shadow_env.env;
    let mut shadow_version = persisted_shadow_env.version;
    let mut last_shadow_report = None::<ShadowReportedPayload>;
    let mut pending_shadow_resync = false;

    while let Some(message) = inbound_rx.recv().await {
        match message {
            InboundMessage::DeviceStatusGet(payload) => {
                if !matches_identity(
                    &config.identity,
                    &payload.fleet_id,
                    Some(payload.device_uuid.as_str()),
                ) {
                    warn!("dropping deviceStatus request for another device");
                    continue;
                }

                let _ = metrics_trigger_tx.send(MetricsTrigger::Immediate).await;
                let _ = timeout(Duration::from_secs(3), metrics_rx.changed()).await;
                let payload =
                    DeviceStatusUpdatePayload::from_snapshot(&payload, &metrics_rx.borrow());
                let _ = outbound_tx
                    .send(OutboundMessage::Publish(PublishMessage::json(
                        topics::device_status_update(&config),
                        &payload,
                    )?))
                    .await;
            }
            InboundMessage::ReleaseStatusGet(payload) => {
                if !matches_identity(
                    &config.identity,
                    &payload.fleet_id,
                    Some(payload.device_uuid.as_str()),
                ) {
                    warn!("dropping releaseStatus request for another device");
                    continue;
                }

                if payload.order.name == "releaseStatus" {
                    let local_state = local_state_rx.borrow().clone();
                    let payload =
                        ReleaseStatusUpdatePayload::from_local_state(&payload, &local_state);
                    let _ = outbound_tx
                        .send(OutboundMessage::Publish(PublishMessage::json(
                            topics::release_status_update(&config),
                            &payload,
                        )?))
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
                let payload = ReleaseStatusUpdatePayload::from_local_state(&payload, &local_state);
                let _ = outbound_tx
                    .send(OutboundMessage::Publish(PublishMessage::json(
                        topics::release_status_update(&config),
                        &payload,
                    )?))
                    .await;
            }
            InboundMessage::Script(payload) => {
                if !matches_identity(
                    &config.identity,
                    &payload.fleet_id,
                    payload.device_uuid.as_deref(),
                ) {
                    warn!("dropping script request for another device");
                    continue;
                }

                if let Some(response) =
                    handle_script_request(&payload, &config.identity, &config.script).await?
                {
                    let _ = outbound_tx
                        .send(OutboundMessage::Publish(PublishMessage::json(
                            topics::script_update(&config),
                            &response,
                        )?))
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
                persist_shadow_env(shadow_env_store.as_ref(), &current_env, shadow_version).await?;
                let has_reported_changes = !apply_result.reported_changes.is_empty();
                if !apply_result.reported_changes.is_empty() {
                    let report = ShadowReportedPayload::from_changes(
                        apply_result.reported_changes,
                        shadow_version,
                    );
                    last_shadow_report = Some(report.clone());
                    let _ = outbound_tx
                        .send(OutboundMessage::Publish(PublishMessage::json(
                            topics::shadow_update(&config),
                            &report,
                        )?))
                        .await;
                }

                if pending_shadow_resync && !has_reported_changes {
                    pending_shadow_resync = false;
                    if let Some(report) = last_shadow_report
                        .as_ref()
                        .map(|report| report.with_version(shadow_version))
                    {
                        let _ = outbound_tx
                            .send(OutboundMessage::Publish(PublishMessage::json(
                                topics::shadow_update(&config),
                                &report,
                            )?))
                            .await;
                    }
                } else {
                    pending_shadow_resync = false;
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
                persist_shadow_env(shadow_env_store.as_ref(), &current_env, shadow_version).await?;
                if !apply_result.reported_changes.is_empty() {
                    let report = ShadowReportedPayload::from_changes(
                        apply_result.reported_changes,
                        shadow_version,
                    );
                    last_shadow_report = Some(report.clone());
                    let _ = outbound_tx
                        .send(OutboundMessage::Publish(PublishMessage::json(
                            topics::shadow_update(&config),
                            &report,
                        )?))
                        .await;
                }
            }
            InboundMessage::ShadowGetRejected(payload) => {
                if !config.shadow_env.enable {
                    continue;
                }
                debug!(
                    code = payload.code,
                    message = payload.message,
                    "shadow get rejected"
                );

                if payload.code == 404 {
                    let report = ShadowReportedPayload::from_env(&current_env, shadow_version);
                    last_shadow_report = Some(report.clone());
                    let _ = outbound_tx
                        .send(OutboundMessage::Publish(PublishMessage::json(
                            topics::shadow_update(&config),
                            &report,
                        )?))
                        .await;
                    continue;
                }

                let _ = outbound_tx
                    .send(OutboundMessage::SchedulePublish(ScheduledPublish::raw(
                        Duration::from_secs(30),
                        topics::shadow_get(&config),
                        "",
                    )))
                    .await;
            }
            InboundMessage::ShadowUpdateRejected(payload) => {
                if !config.shadow_env.enable {
                    continue;
                }
                debug!(
                    code = payload.code,
                    message = payload.message,
                    "shadow update rejected"
                );

                if payload.code == 409 {
                    shadow_version = None;
                    pending_shadow_resync = true;
                    let _ = outbound_tx
                        .send(OutboundMessage::Publish(PublishMessage::raw(
                            topics::shadow_get(&config),
                            "",
                        )))
                        .await;
                    continue;
                }

                if let Some(report) = &last_shadow_report {
                    let _ = outbound_tx
                        .send(OutboundMessage::SchedulePublish(ScheduledPublish::json(
                            Duration::from_secs(30),
                            topics::shadow_update(&config),
                            report,
                        )?))
                        .await;
                }
            }
        }
    }

    Ok(())
}

async fn persist_shadow_env(
    shadow_env_store: Option<&ShadowEnvStore>,
    current_env: &BTreeMap<String, String>,
    shadow_version: Option<i64>,
) -> Result<(), RuntimeError> {
    if let Some(store) = shadow_env_store {
        store
            .save(&PersistedShadowEnv {
                env: current_env.clone(),
                version: shadow_version,
            })
            .await?;
    }

    Ok(())
}

fn matches_identity(identity: &MqttIdentity, fleet_id: &str, device_uuid: Option<&str>) -> bool {
    fleet_id == identity.fleet_id
        && device_uuid
            .map(|device_uuid| device_uuid == identity.device_uuid)
            .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use helios_host_metrics::{HostMetricsSnapshot, MetricsTrigger};
    use helios_state::models::Device;
    use helios_store::DocumentStore;
    use helios_util::types::Uuid;
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::*;

    fn mqtt_config() -> MqttConfig {
        MqttConfig {
            broker_url: "mqtt://localhost:1883".to_string(),
            topic_head: "sensecapmx".to_string(),
            identity: MqttIdentity {
                fleet_id: "12".to_string(),
                device_uuid: "dev-1".to_string(),
            },
            credentials: MqttCredentials::default(),
            clean_session: true,
            keep_alive: Duration::from_secs(30),
            device_status_report: ReportConfig::default(),
            release_status_report: ReportConfig::default(),
            script: ScriptConfig {
                enable: true,
                exec_timeout: Duration::from_secs(1),
                max_output_bytes: 1024,
            },
            shadow_env: ShadowEnvConfig { enable: true },
        }
    }

    fn local_state() -> LocalState {
        LocalState {
            authorized_apps: vec![],
            device: Device::new(Uuid::from("my-device"), None),
            status: helios_state::UpdateStatus::Done,
        }
    }

    #[tokio::test]
    async fn it_publishes_device_status_after_immediate_sampling() {
        let config = mqtt_config();
        let (inbound_tx, inbound_rx) = mpsc::channel(4);
        let (seek_tx, _seek_rx) = watch::channel(SeekRequest::default());
        let (local_state_tx, local_state_rx) = watch::channel(local_state());
        let (metrics_trigger_tx, mut metrics_trigger_rx) = mpsc::channel(4);
        let (metrics_tx, metrics_rx) = watch::channel(HostMetricsSnapshot::default());
        let (outbound_tx, mut outbound_rx) = mpsc::channel(4);

        let handle = tokio::spawn(start(
            config.clone(),
            None,
            inbound_rx,
            seek_tx,
            local_state_rx,
            metrics_trigger_tx,
            metrics_rx,
            outbound_tx,
        ));

        inbound_tx
            .send(InboundMessage::DeviceStatusGet(
                serde_json::from_value(json!({
                    "requestId": "req-1",
                    "timestamp": 1,
                    "fleetId": "12",
                    "deviceUUID": "dev-1",
                    "intent": "order",
                    "order": { "name": "deviceStatus" }
                }))
                .unwrap(),
            ))
            .await
            .unwrap();

        assert_eq!(
            metrics_trigger_rx.recv().await,
            Some(MetricsTrigger::Immediate)
        );

        metrics_tx
            .send(HostMetricsSnapshot {
                timestamp_ms: 42,
                memory_total_mb: 100.0,
                memory_used_mb: 10.0,
                sd_total_mb: 200.0,
                sd_used_mb: 20.0,
                flash_total_mb: 300.0,
                flash_used_mb: 30.0,
                cpu_temperature_c: 40.0,
                cpu_used_percent: 50.0,
                local_ip: vec!["192.168.1.10".to_string()],
                public_ip: Vec::new(),
                latitude: 0.0,
                longitude: 0.0,
                altitude: 0.0,
                lora_modem: None,
                lte: None,
                gps: None,
            })
            .unwrap();

        let outbound = outbound_rx.recv().await.unwrap();
        match outbound {
            OutboundMessage::Publish(message) => {
                assert_eq!(message.topic, topics::device_status_update(&config));
                let payload: Value = serde_json::from_str(&message.payload).unwrap();
                assert_eq!(payload["requestId"], "req-1");
                assert_eq!(payload["order"]["value"]["timestampMs"], 42);
                assert_eq!(
                    payload["order"]["value"]["localIp"],
                    json!(["192.168.1.10"])
                );
            }
            other => panic!("expected publish message, got {other:?}"),
        }

        drop(inbound_tx);
        drop(local_state_tx);
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn it_publishes_script_responses_to_script_topic() {
        let config = mqtt_config();
        let (inbound_tx, inbound_rx) = mpsc::channel(4);
        let (seek_tx, _seek_rx) = watch::channel(SeekRequest::default());
        let (_local_state_tx, local_state_rx) = watch::channel(local_state());
        let (metrics_trigger_tx, _metrics_trigger_rx) = mpsc::channel(4);
        let (_metrics_tx, metrics_rx) = watch::channel(HostMetricsSnapshot::default());
        let (outbound_tx, mut outbound_rx) = mpsc::channel(4);

        let handle = tokio::spawn(start(
            config.clone(),
            None,
            inbound_rx,
            seek_tx,
            local_state_rx,
            metrics_trigger_tx,
            metrics_rx,
            outbound_tx,
        ));

        inbound_tx
            .send(InboundMessage::Script(
                serde_json::from_value(json!({
                    "requestId": "req-1",
                    "timestamp": 10,
                    "fleetId": "12",
                    "deviceUUID": "dev-1",
                    "intent": "script",
                    "type": "request",
                    "script": {
                        "name": "my-script",
                        "version": "1.0.0",
                        "type": 1,
                        "expireTime": 0,
                        "cmd": "echo hello"
                    }
                }))
                .unwrap(),
            ))
            .await
            .unwrap();

        let outbound = outbound_rx.recv().await.unwrap();
        match outbound {
            OutboundMessage::Publish(message) => {
                assert_eq!(message.topic, topics::script_update(&config));
                let payload: Value = serde_json::from_str(&message.payload).unwrap();
                assert_eq!(payload["order"]["name"], "script");
                assert_eq!(payload["order"]["value"]["msg"], "hello");
            }
            other => panic!("expected publish message, got {other:?}"),
        }

        drop(inbound_tx);
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn it_publishes_shadow_delta_reports_to_shadow_update_topic() {
        let config = mqtt_config();
        let (inbound_tx, inbound_rx) = mpsc::channel(4);
        let (seek_tx, _seek_rx) = watch::channel(SeekRequest::default());
        let (_local_state_tx, local_state_rx) = watch::channel(local_state());
        let (metrics_trigger_tx, _metrics_trigger_rx) = mpsc::channel(4);
        let (_metrics_tx, metrics_rx) = watch::channel(HostMetricsSnapshot::default());
        let (outbound_tx, mut outbound_rx) = mpsc::channel(4);

        let handle = tokio::spawn(start(
            config.clone(),
            None,
            inbound_rx,
            seek_tx,
            local_state_rx,
            metrics_trigger_tx,
            metrics_rx,
            outbound_tx,
        ));

        inbound_tx
            .send(InboundMessage::ShadowDelta(
                serde_json::from_value(json!({
                    "state": {
                        "env": {
                            "FOO": "bar",
                            "OLD": null
                        }
                    },
                    "version": 7
                }))
                .unwrap(),
            ))
            .await
            .unwrap();

        let outbound = outbound_rx.recv().await.unwrap();
        match outbound {
            OutboundMessage::Publish(message) => {
                assert_eq!(message.topic, topics::shadow_update(&config));
                let payload: Value = serde_json::from_str(&message.payload).unwrap();
                assert_eq!(payload["version"], 7);
                assert_eq!(payload["state"]["reported"]["env"]["FOO"], "bar");
                assert_eq!(payload["state"]["reported"]["env"]["OLD"], Value::Null);
            }
            other => panic!("expected publish message, got {other:?}"),
        }

        drop(inbound_tx);
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn it_triggers_release_report_after_release_control() {
        let config = mqtt_config();
        let (inbound_tx, inbound_rx) = mpsc::channel(4);
        let (seek_tx, mut seek_rx) = watch::channel(SeekRequest::default());
        let (_local_state_tx, local_state_rx) = watch::channel(LocalState {
            authorized_apps: vec![],
            device: serde_json::from_value(json!({
                "uuid": "my-device",
                "apps": {
                    "my-app": {
                        "id": 1,
                        "name": "my-app",
                        "releases": {
                            "my-release": {
                                "installed": true,
                                "services": {
                                    "svc": {
                                        "id": 1,
                                        "image": "ubuntu:latest",
                                        "container_name": "svc_my-release",
                                        "started": true,
                                        "config": {}
                                    }
                                }
                            }
                        }
                    }
                }
            }))
            .unwrap(),
            status: helios_state::UpdateStatus::Done,
        });
        let (metrics_trigger_tx, _metrics_trigger_rx) = mpsc::channel(4);
        let (_metrics_tx, metrics_rx) = watch::channel(HostMetricsSnapshot::default());
        let (outbound_tx, mut outbound_rx) = mpsc::channel(4);

        let handle = tokio::spawn(start(
            config.clone(),
            None,
            inbound_rx,
            seek_tx,
            local_state_rx,
            metrics_trigger_tx,
            metrics_rx,
            outbound_tx,
        ));

        inbound_tx
            .send(InboundMessage::ReleaseStatusGet(
                serde_json::from_value(json!({
                    "requestId": "r1",
                    "timestamp": 1,
                    "fleetId": "12",
                    "deviceUUID": "dev-1",
                    "intent": "order",
                    "type": "request",
                    "order": {
                        "name": "releaseControl",
                        "value": {
                            "services": [{ "name": "svc", "action": 3 }]
                        }
                    }
                }))
                .unwrap(),
            ))
            .await
            .unwrap();

        seek_rx.changed().await.unwrap();
        let request = seek_rx.borrow().clone();
        assert!(matches!(
            request.target,
            helios_state::TargetState::Local { .. }
        ));

        match outbound_rx.recv().await.unwrap() {
            OutboundMessage::Publish(message) => {
                assert_eq!(message.topic, topics::release_status_update(&config));
                let payload: Value = serde_json::from_str(&message.payload).unwrap();
                assert_eq!(payload["order"]["name"], "releaseStatus");
            }
            other => panic!("expected publish message, got {other:?}"),
        }

        drop(inbound_tx);
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn it_reports_current_env_on_shadow_get_404() {
        let config = mqtt_config();
        let (inbound_tx, inbound_rx) = mpsc::channel(4);
        let (seek_tx, _seek_rx) = watch::channel(SeekRequest::default());
        let (_local_state_tx, local_state_rx) = watch::channel(local_state());
        let (metrics_trigger_tx, _metrics_trigger_rx) = mpsc::channel(4);
        let (_metrics_tx, metrics_rx) = watch::channel(HostMetricsSnapshot::default());
        let (outbound_tx, mut outbound_rx) = mpsc::channel(8);

        let handle = tokio::spawn(start(
            config.clone(),
            None,
            inbound_rx,
            seek_tx,
            local_state_rx,
            metrics_trigger_tx,
            metrics_rx,
            outbound_tx,
        ));

        inbound_tx
            .send(InboundMessage::ShadowDelta(
                serde_json::from_value(json!({
                    "state": {
                        "env": {
                            "FOO": "bar"
                        }
                    },
                    "version": 7
                }))
                .unwrap(),
            ))
            .await
            .unwrap();
        let _ = outbound_rx.recv().await.unwrap();

        inbound_tx
            .send(InboundMessage::ShadowGetRejected(
                serde_json::from_value(json!({
                    "code": 404,
                    "message": "not found"
                }))
                .unwrap(),
            ))
            .await
            .unwrap();

        match outbound_rx.recv().await.unwrap() {
            OutboundMessage::Publish(message) => {
                assert_eq!(message.topic, topics::shadow_update(&config));
                let payload: Value = serde_json::from_str(&message.payload).unwrap();
                assert_eq!(payload["state"]["reported"]["env"]["FOO"], "bar");
            }
            other => panic!("expected publish message, got {other:?}"),
        }

        drop(inbound_tx);
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn it_schedules_shadow_update_retry_after_rejection() {
        let config = mqtt_config();
        let (inbound_tx, inbound_rx) = mpsc::channel(4);
        let (seek_tx, _seek_rx) = watch::channel(SeekRequest::default());
        let (_local_state_tx, local_state_rx) = watch::channel(local_state());
        let (metrics_trigger_tx, _metrics_trigger_rx) = mpsc::channel(4);
        let (_metrics_tx, metrics_rx) = watch::channel(HostMetricsSnapshot::default());
        let (outbound_tx, mut outbound_rx) = mpsc::channel(8);

        let handle = tokio::spawn(start(
            config.clone(),
            None,
            inbound_rx,
            seek_tx,
            local_state_rx,
            metrics_trigger_tx,
            metrics_rx,
            outbound_tx,
        ));

        inbound_tx
            .send(InboundMessage::ShadowDelta(
                serde_json::from_value(json!({
                    "state": {
                        "env": {
                            "FOO": "bar"
                        }
                    },
                    "version": 7
                }))
                .unwrap(),
            ))
            .await
            .unwrap();
        let _ = outbound_rx.recv().await.unwrap();

        inbound_tx
            .send(InboundMessage::ShadowUpdateRejected(
                serde_json::from_value(json!({
                    "code": 500,
                    "message": "server error"
                }))
                .unwrap(),
            ))
            .await
            .unwrap();

        match outbound_rx.recv().await.unwrap() {
            OutboundMessage::SchedulePublish(scheduled) => {
                assert_eq!(scheduled.delay, Duration::from_secs(30));
                assert_eq!(scheduled.message.topic, topics::shadow_update(&config));
            }
            other => panic!("expected scheduled publish, got {other:?}"),
        }

        drop(inbound_tx);
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn it_recovers_from_shadow_update_conflicts() {
        let config = mqtt_config();
        let (inbound_tx, inbound_rx) = mpsc::channel(8);
        let (seek_tx, _seek_rx) = watch::channel(SeekRequest::default());
        let (_local_state_tx, local_state_rx) = watch::channel(local_state());
        let (metrics_trigger_tx, _metrics_trigger_rx) = mpsc::channel(4);
        let (_metrics_tx, metrics_rx) = watch::channel(HostMetricsSnapshot::default());
        let (outbound_tx, mut outbound_rx) = mpsc::channel(8);

        let handle = tokio::spawn(start(
            config.clone(),
            None,
            inbound_rx,
            seek_tx,
            local_state_rx,
            metrics_trigger_tx,
            metrics_rx,
            outbound_tx,
        ));

        inbound_tx
            .send(InboundMessage::ShadowDelta(
                serde_json::from_value(json!({
                    "state": {
                        "env": {
                            "FOO": "bar"
                        }
                    },
                    "version": 7
                }))
                .unwrap(),
            ))
            .await
            .unwrap();
        let _ = outbound_rx.recv().await.unwrap();

        inbound_tx
            .send(InboundMessage::ShadowUpdateRejected(
                serde_json::from_value(json!({
                    "code": 409,
                    "message": "version conflict"
                }))
                .unwrap(),
            ))
            .await
            .unwrap();

        match outbound_rx.recv().await.unwrap() {
            OutboundMessage::Publish(message) => {
                assert_eq!(message.topic, topics::shadow_get(&config));
                assert_eq!(message.payload, "");
            }
            other => panic!("expected immediate shadow get publish, got {other:?}"),
        }

        inbound_tx
            .send(InboundMessage::ShadowAccepted(
                serde_json::from_value(json!({
                    "state": {
                        "desired": {
                            "env": {
                                "FOO": "bar"
                            }
                        }
                    },
                    "version": 9
                }))
                .unwrap(),
            ))
            .await
            .unwrap();

        match outbound_rx.recv().await.unwrap() {
            OutboundMessage::Publish(message) => {
                assert_eq!(message.topic, topics::shadow_update(&config));
                let payload: Value = serde_json::from_str(&message.payload).unwrap();
                assert_eq!(payload["version"], 9);
                assert_eq!(payload["state"]["reported"]["env"]["FOO"], "bar");
            }
            other => panic!("expected replayed shadow update, got {other:?}"),
        }

        drop(inbound_tx);
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn it_persists_shadow_env_changes() {
        let config = mqtt_config();
        let dir = tempdir().unwrap();
        let document_store = DocumentStore::with_root(dir.path()).await.unwrap();
        let shadow_env_store = ShadowEnvStore::new(document_store);
        let (inbound_tx, inbound_rx) = mpsc::channel(4);
        let (seek_tx, _seek_rx) = watch::channel(SeekRequest::default());
        let (_local_state_tx, local_state_rx) = watch::channel(local_state());
        let (metrics_trigger_tx, _metrics_trigger_rx) = mpsc::channel(4);
        let (_metrics_tx, metrics_rx) = watch::channel(HostMetricsSnapshot::default());
        let (outbound_tx, mut outbound_rx) = mpsc::channel(4);

        let handle = tokio::spawn(start(
            config,
            Some(shadow_env_store.clone()),
            inbound_rx,
            seek_tx,
            local_state_rx,
            metrics_trigger_tx,
            metrics_rx,
            outbound_tx,
        ));

        inbound_tx
            .send(InboundMessage::ShadowDelta(
                serde_json::from_value(json!({
                    "state": {
                        "env": {
                            "FOO": "bar"
                        }
                    },
                    "version": 7
                }))
                .unwrap(),
            ))
            .await
            .unwrap();

        let _ = outbound_rx.recv().await.unwrap();

        drop(inbound_tx);
        handle.await.unwrap().unwrap();

        let persisted = shadow_env_store.load().await.unwrap();
        assert_eq!(persisted.version, Some(7));
        assert_eq!(persisted.env.get("FOO"), Some(&"bar".to_string()));
    }

    #[tokio::test]
    async fn it_restores_shadow_env_from_store_on_start() {
        let config = mqtt_config();
        let dir = tempdir().unwrap();
        let document_store = DocumentStore::with_root(dir.path()).await.unwrap();
        let shadow_env_store = ShadowEnvStore::new(document_store);
        shadow_env_store
            .save(&PersistedShadowEnv {
                env: [("FOO".to_string(), "bar".to_string())].into(),
                version: Some(7),
            })
            .await
            .unwrap();

        let (inbound_tx, inbound_rx) = mpsc::channel(4);
        let (seek_tx, _seek_rx) = watch::channel(SeekRequest::default());
        let (_local_state_tx, local_state_rx) = watch::channel(local_state());
        let (metrics_trigger_tx, _metrics_trigger_rx) = mpsc::channel(4);
        let (_metrics_tx, metrics_rx) = watch::channel(HostMetricsSnapshot::default());
        let (outbound_tx, mut outbound_rx) = mpsc::channel(4);

        let handle = tokio::spawn(start(
            config.clone(),
            Some(shadow_env_store),
            inbound_rx,
            seek_tx,
            local_state_rx,
            metrics_trigger_tx,
            metrics_rx,
            outbound_tx,
        ));

        inbound_tx
            .send(InboundMessage::ShadowGetRejected(
                serde_json::from_value(json!({
                    "code": 404,
                    "message": "not found"
                }))
                .unwrap(),
            ))
            .await
            .unwrap();

        match outbound_rx.recv().await.unwrap() {
            OutboundMessage::Publish(message) => {
                assert_eq!(message.topic, topics::shadow_update(&config));
                let payload: Value = serde_json::from_str(&message.payload).unwrap();
                assert_eq!(payload["version"], 7);
                assert_eq!(payload["state"]["reported"]["env"]["FOO"], "bar");
            }
            other => panic!("expected publish message, got {other:?}"),
        }

        drop(inbound_tx);
        handle.await.unwrap().unwrap();
    }
}
