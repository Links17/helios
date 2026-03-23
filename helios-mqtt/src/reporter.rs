use helios_host_metrics::HostMetricsSnapshot;
use helios_state::LocalState;
use helios_util::ticker;
use tokio::sync::{mpsc, watch};

use crate::protocol::{DeviceStatusUpdatePayload, ReleaseStatusUpdatePayload};
use crate::{
    MqttConfig, OutboundMessage, PublishMessage, ReportConfig, ReportStartup, RuntimeError, topics,
};

fn report_interval(config: ReportConfig) -> tokio::time::Interval {
    match config.startup {
        ReportStartup::Immediate => ticker::interval(config.interval),
        ReportStartup::Delayed => ticker::delayed_interval(config.interval),
    }
}

async fn start_reporter<F>(
    report_config: ReportConfig,
    outbound_tx: mpsc::Sender<OutboundMessage>,
    mut build_message: F,
) -> Result<(), RuntimeError>
where
    F: FnMut() -> Result<PublishMessage, RuntimeError>,
{
    let mut interval = report_interval(report_config);

    loop {
        interval.tick().await;

        let _ = outbound_tx
            .send(OutboundMessage::Publish(build_message()?))
            .await;
    }
}

pub async fn start_device_status_reporter(
    config: MqttConfig,
    metrics_rx: watch::Receiver<HostMetricsSnapshot>,
    outbound_tx: mpsc::Sender<OutboundMessage>,
) -> Result<(), RuntimeError> {
    start_reporter(config.device_status_report, outbound_tx, move || {
        let payload =
            DeviceStatusUpdatePayload::periodic_report(&config.identity, &metrics_rx.borrow());
        PublishMessage::json(topics::device_status_update(&config), &payload)
    })
    .await
}

pub async fn start_release_status_reporter(
    config: MqttConfig,
    local_state_rx: watch::Receiver<LocalState>,
    outbound_tx: mpsc::Sender<OutboundMessage>,
) -> Result<(), RuntimeError> {
    start_reporter(config.release_status_report, outbound_tx, move || {
        let payload =
            ReleaseStatusUpdatePayload::periodic_report(&config.identity, &local_state_rx.borrow());
        PublishMessage::json(topics::release_status_update(&config), &payload)
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use helios_host_metrics::HostMetricsSnapshot;
    use helios_state::LocalState;
    use serde_json::Value;
    use tokio::sync::{mpsc, watch};
    use tokio::time::timeout;

    use super::*;
    use crate::config::{
        MqttCredentials, MqttIdentity, ReportConfig, ReportStartup, ScriptConfig, ShadowEnvConfig,
    };

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
            device_status_report: ReportConfig {
                interval: Duration::from_millis(20),
                startup: ReportStartup::Immediate,
            },
            release_status_report: ReportConfig {
                interval: Duration::from_millis(20),
                startup: ReportStartup::Immediate,
            },
            script: ScriptConfig::default(),
            shadow_env: ShadowEnvConfig::default(),
        }
    }

    #[tokio::test]
    async fn it_periodically_publishes_device_status_updates() {
        let config = mqtt_config();
        let (metrics_tx, metrics_rx) = watch::channel(HostMetricsSnapshot {
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
        });
        let (outbound_tx, mut outbound_rx) = mpsc::channel(4);

        let handle = tokio::spawn(start_device_status_reporter(
            config.clone(),
            metrics_rx,
            outbound_tx,
        ));

        let outbound = timeout(Duration::from_secs(1), outbound_rx.recv())
            .await
            .unwrap()
            .unwrap();
        match outbound {
            OutboundMessage::Publish(message) => {
                assert_eq!(message.topic, topics::device_status_update(&config));
                let payload: Value = serde_json::from_str(&message.payload).unwrap();
                assert_eq!(payload["requestId"], "");
                assert_eq!(payload["type"], "report");
                assert_eq!(payload["order"]["name"], "deviceStatus");
                assert_eq!(payload["order"]["value"]["timestampMs"], 42);
                assert_eq!(
                    payload["order"]["value"]["localIp"],
                    serde_json::json!(["192.168.1.10"])
                );
            }
            other => panic!("expected publish message, got {other:?}"),
        }

        metrics_tx.send(HostMetricsSnapshot::default()).unwrap();
        handle.abort();
    }

    #[tokio::test]
    async fn it_supports_delayed_start_for_device_status_reports() {
        let mut config = mqtt_config();
        config.device_status_report = ReportConfig {
            interval: Duration::from_millis(50),
            startup: ReportStartup::Delayed,
        };

        let (_metrics_tx, metrics_rx) = watch::channel(HostMetricsSnapshot::default());
        let (outbound_tx, mut outbound_rx) = mpsc::channel(4);

        let handle = tokio::spawn(start_device_status_reporter(
            config,
            metrics_rx,
            outbound_tx,
        ));

        let first_attempt = timeout(Duration::from_millis(10), outbound_rx.recv()).await;
        assert!(first_attempt.is_err());

        let delayed_publish = timeout(Duration::from_millis(200), outbound_rx.recv())
            .await
            .unwrap();
        assert!(matches!(delayed_publish, Some(OutboundMessage::Publish(_))));

        handle.abort();
    }

    #[tokio::test]
    async fn it_periodically_publishes_release_status_updates() {
        let config = mqtt_config();
        let (_local_state_tx, local_state_rx) = watch::channel(LocalState {
            authorized_apps: vec![],
            device: serde_json::from_value(serde_json::json!({
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
        let (outbound_tx, mut outbound_rx) = mpsc::channel(4);

        let handle = tokio::spawn(start_release_status_reporter(
            config.clone(),
            local_state_rx,
            outbound_tx,
        ));

        let outbound = timeout(Duration::from_secs(1), outbound_rx.recv())
            .await
            .unwrap()
            .unwrap();
        match outbound {
            OutboundMessage::Publish(message) => {
                assert_eq!(message.topic, topics::release_status_update(&config));
                let payload: Value = serde_json::from_str(&message.payload).unwrap();
                assert_eq!(payload["requestId"], "");
                assert_eq!(payload["type"], "report");
                assert_eq!(payload["order"]["name"], "releaseStatus");
                assert_eq!(payload["order"]["value"]["services"][0]["name"], "svc");
                assert_eq!(payload["order"]["value"]["services"][0]["started"], true);
            }
            other => panic!("expected publish message, got {other:?}"),
        }

        handle.abort();
    }
}
