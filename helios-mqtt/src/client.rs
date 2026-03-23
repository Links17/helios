use std::time::Duration;

use rumqttc::{AsyncClient, Event, MqttOptions, Outgoing, Packet, Publish, QoS, Transport};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use url::Url;

use crate::config::MqttConfig;
use crate::topics;
use crate::{InboundMessage, OutboundMessage, PublishMessage, ScheduledPublish};

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("invalid MQTT broker URL: {0}")]
    InvalidBrokerUrl(#[from] url::ParseError),

    #[error("MQTT broker URL is missing host")]
    MissingBrokerHost,

    #[error("unsupported MQTT broker scheme: {0}")]
    UnsupportedBrokerScheme(String),

    #[error("mqtt client error: {0}")]
    Client(#[from] rumqttc::ClientError),

    #[error("mqtt connection error: {0}")]
    Connection(#[from] rumqttc::ConnectionError),

    #[error("failed to decode mqtt payload for topic {topic}: {source}")]
    Decode {
        topic: String,
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Clone)]
struct SubscriptionTopics {
    device_status_get: String,
    release_status_get: String,
    script_get: String,
    shadow_get_accepted: String,
    shadow_get_rejected: String,
    shadow_update_accepted: String,
    shadow_update_rejected: String,
    shadow_update_delta: String,
}

impl SubscriptionTopics {
    fn new(config: &MqttConfig) -> Self {
        Self {
            device_status_get: topics::device_status_get(config),
            release_status_get: topics::release_status_get(config),
            script_get: topics::script_get(config),
            shadow_get_accepted: topics::shadow_get_accepted(config),
            shadow_get_rejected: topics::shadow_get_rejected(config),
            shadow_update_accepted: topics::shadow_update_accepted(config),
            shadow_update_rejected: topics::shadow_update_rejected(config),
            shadow_update_delta: topics::shadow_update_delta(config),
        }
    }

    fn all(&self) -> [&str; 8] {
        [
            &self.device_status_get,
            &self.release_status_get,
            &self.script_get,
            &self.shadow_get_accepted,
            &self.shadow_get_rejected,
            &self.shadow_update_accepted,
            &self.shadow_update_rejected,
            &self.shadow_update_delta,
        ]
    }

    fn decode_publish(&self, publish: &Publish) -> Result<Option<InboundMessage>, ClientError> {
        let topic = publish.topic.as_str();
        let payload = publish.payload.as_ref();

        if topic == self.device_status_get {
            return Ok(Some(InboundMessage::DeviceStatusGet(decode_payload(
                topic, payload,
            )?)));
        }

        if topic == self.release_status_get {
            return Ok(Some(InboundMessage::ReleaseStatusGet(decode_payload(
                topic, payload,
            )?)));
        }

        if topic == self.script_get {
            return Ok(Some(InboundMessage::Script(decode_payload(
                topic, payload,
            )?)));
        }

        if topic == self.shadow_get_accepted || topic == self.shadow_update_accepted {
            return Ok(Some(InboundMessage::ShadowAccepted(decode_payload(
                topic, payload,
            )?)));
        }

        if topic == self.shadow_get_rejected {
            return Ok(Some(InboundMessage::ShadowGetRejected(decode_payload(
                topic, payload,
            )?)));
        }

        if topic == self.shadow_update_rejected {
            return Ok(Some(InboundMessage::ShadowUpdateRejected(decode_payload(
                topic, payload,
            )?)));
        }

        if topic == self.shadow_update_delta {
            return Ok(Some(InboundMessage::ShadowDelta(decode_payload(
                topic, payload,
            )?)));
        }

        Ok(None)
    }
}

pub async fn start_client(
    config: MqttConfig,
    inbound_tx: mpsc::Sender<InboundMessage>,
    mut outbound_rx: mpsc::Receiver<OutboundMessage>,
) -> Result<(), ClientError> {
    let subscriptions = SubscriptionTopics::new(&config);
    let (scheduled_tx, mut scheduled_rx) = mpsc::channel::<PublishMessage>(32);
    let mut reconnect_attempt = 0u32;

    loop {
        if reconnect_attempt > 0 {
            let current_delay = reconnect_delay(reconnect_attempt - 1);
            warn!(
                delay_secs = current_delay.as_secs(),
                "mqtt reconnect backoff"
            );
            tokio::time::sleep(current_delay).await;
        }

        let mqtt_options = mqtt_options_from_config(&config)?;
        let (client, mut eventloop) = AsyncClient::new(mqtt_options, 32);

        subscribe_all(&client, &subscriptions).await?;
        client
            .publish(topics::shadow_get(&config), QoS::AtMostOnce, false, "")
            .await?;
        info!("mqtt connected and subscribed");
        reconnect_attempt = 0;

        let mut needs_reconnect = false;
        while !needs_reconnect {
            tokio::select! {
                event = eventloop.poll() => {
                    match event {
                        Ok(Event::Incoming(Packet::Publish(publish))) => {
                            if let Some(message) = subscriptions.decode_publish(&publish)? {
                                if inbound_tx.send(message).await.is_err() {
                                    return Ok(());
                                }
                            }
                        }
                        Ok(Event::Outgoing(Outgoing::Disconnect)) => {
                            warn!("mqtt disconnected");
                            needs_reconnect = true;
                        }
                        Ok(_) => {}
                        Err(error) => {
                            warn!(error = %error, "mqtt poll failed, reconnecting");
                            needs_reconnect = true;
                        }
                    }
                }
                outbound = outbound_rx.recv() => {
                    match outbound {
                        Some(OutboundMessage::Publish(message)) => {
                            publish_message(&client, message).await?;
                        }
                        Some(OutboundMessage::SchedulePublish(scheduled)) => {
                            schedule_publish(scheduled_tx.clone(), scheduled);
                        }
                        None => return Ok(()),
                    }
                }
                scheduled = scheduled_rx.recv() => {
                    if let Some(message) = scheduled {
                        publish_message(&client, message).await?;
                    }
                }
            }
        }

        reconnect_attempt = reconnect_attempt.saturating_add(1);
    }
}

fn mqtt_options_from_config(config: &MqttConfig) -> Result<MqttOptions, ClientError> {
    let broker_url = Url::parse(&config.broker_url)?;
    let host = broker_url
        .host_str()
        .ok_or(ClientError::MissingBrokerHost)?
        .to_string();

    let port = broker_url
        .port()
        .unwrap_or_else(|| default_port_for_scheme(broker_url.scheme()));
    let client_id = format!(
        "helios-{}-{}",
        config.identity.fleet_id, config.identity.device_uuid
    );

    let mut mqtt_options = MqttOptions::new(client_id, host, port);
    mqtt_options.set_clean_session(config.clean_session);
    mqtt_options.set_keep_alive(config.keep_alive);

    let username =
        config.credentials.username.clone().or_else(|| {
            (!broker_url.username().is_empty()).then(|| broker_url.username().to_string())
        });
    let password = config
        .credentials
        .password
        .clone()
        .or_else(|| broker_url.password().map(ToString::to_string));
    if let Some(username) = username {
        mqtt_options.set_credentials(username, password.unwrap_or_default());
    }

    match broker_url.scheme() {
        "mqtt" | "tcp" => {}
        "mqtts" | "ssl" | "tls" => {
            mqtt_options.set_transport(Transport::tls_with_default_config());
        }
        scheme => return Err(ClientError::UnsupportedBrokerScheme(scheme.to_string())),
    }

    Ok(mqtt_options)
}

fn default_port_for_scheme(scheme: &str) -> u16 {
    match scheme {
        "mqtts" | "ssl" | "tls" => 8883,
        _ => 1883,
    }
}

fn reconnect_delay(reconnect_attempt: u32) -> Duration {
    Duration::from_secs(1u64 << reconnect_attempt.min(5))
}

async fn subscribe_all(
    client: &AsyncClient,
    subscriptions: &SubscriptionTopics,
) -> Result<(), ClientError> {
    for topic in subscriptions.all() {
        client.subscribe(topic, QoS::AtMostOnce).await?;
    }

    Ok(())
}

async fn publish_message(client: &AsyncClient, message: PublishMessage) -> Result<(), ClientError> {
    debug!(topic = message.topic, "mqtt publish");
    client
        .publish(message.topic, QoS::AtMostOnce, false, message.payload)
        .await?;
    Ok(())
}

fn schedule_publish(scheduled_tx: mpsc::Sender<PublishMessage>, scheduled: ScheduledPublish) {
    tokio::spawn(async move {
        tokio::time::sleep(scheduled.delay).await;
        let _ = scheduled_tx.send(scheduled.message).await;
    });
}

fn decode_payload<T>(topic: &str, payload: &[u8]) -> Result<T, ClientError>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_slice(payload).map_err(|source| ClientError::Decode {
        topic: topic.to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use bytes::BytesMut;
    use helios_host_metrics::HostMetricsSnapshot;
    use helios_state::models::Device;
    use helios_state::{LocalState, SeekRequest};
    use helios_util::types::Uuid;
    use rumqttc::mqttbytes::v4::{Packet, read};
    use rumqttc::{ConnAck, ConnectReturnCode, Publish, SubAck, SubscribeReasonCode};
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::{mpsc, watch};
    use tokio::time::timeout;

    use super::*;
    use crate::config::{MqttCredentials, MqttIdentity, ScriptConfig, ShadowEnvConfig};

    fn mqtt_config() -> MqttConfig {
        MqttConfig {
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
        }
    }

    fn local_state() -> LocalState {
        LocalState {
            authorized_apps: vec![],
            device: Device::new(Uuid::from("my-device"), None),
            status: helios_state::UpdateStatus::Done,
        }
    }

    async fn read_packet(stream: &mut TcpStream, read_buf: &mut BytesMut) -> Packet {
        loop {
            match read(read_buf, 1024 * 1024) {
                Ok(packet) => return packet,
                Err(rumqttc::mqttbytes::Error::InsufficientBytes(_)) => {
                    let bytes_read = timeout(Duration::from_secs(5), stream.read_buf(read_buf))
                        .await
                        .unwrap()
                        .unwrap();
                    assert!(
                        bytes_read > 0,
                        "mqtt test broker socket closed unexpectedly"
                    );
                }
                Err(error) => panic!("failed to read mqtt packet: {error:?}"),
            }
        }
    }

    async fn write_connack(stream: &mut TcpStream) {
        let mut buffer = BytesMut::new();
        ConnAck::new(ConnectReturnCode::Success, false)
            .write(&mut buffer)
            .unwrap();
        stream.write_all(&buffer).await.unwrap();
    }

    async fn write_suback(stream: &mut TcpStream, pkid: u16) {
        let mut buffer = BytesMut::new();
        SubAck::new(pkid, vec![SubscribeReasonCode::Success(QoS::AtMostOnce)])
            .write(&mut buffer)
            .unwrap();
        stream.write_all(&buffer).await.unwrap();
    }

    async fn write_publish(stream: &mut TcpStream, publish: Publish) {
        let mut buffer = BytesMut::new();
        publish.write(&mut buffer).unwrap();
        stream.write_all(&buffer).await.unwrap();
    }

    #[test]
    fn it_parses_tls_broker_urls() {
        let mut config = mqtt_config();
        config.broker_url = "mqtts://user:pass@example.com".to_string();
        config.credentials.username = Some("override-user".to_string());

        let options = mqtt_options_from_config(&config).unwrap();
        assert_eq!(options.broker_address(), ("example.com".to_string(), 8883));
    }

    #[test]
    fn it_routes_shadow_update_rejected_messages() {
        let subscriptions = SubscriptionTopics::new(&mqtt_config());
        let publish = Publish::new(
            subscriptions.shadow_update_rejected.clone(),
            QoS::AtMostOnce,
            serde_json::to_vec(&json!({
                "code": 409,
                "message": "version conflict"
            }))
            .unwrap(),
        );

        let message = subscriptions.decode_publish(&publish).unwrap();
        assert!(matches!(
            message,
            Some(InboundMessage::ShadowUpdateRejected(_))
        ));
    }

    #[test]
    fn it_routes_script_messages() {
        let subscriptions = SubscriptionTopics::new(&mqtt_config());
        let publish = Publish::new(
            subscriptions.script_get.clone(),
            QoS::AtMostOnce,
            serde_json::to_vec(&json!({
                "requestId": "req-1",
                "timestamp": 10,
                "fleetId": "12",
                "deviceUUID": "5000001",
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
        );

        let message = subscriptions.decode_publish(&publish).unwrap();
        assert!(matches!(message, Some(InboundMessage::Script(_))));
    }

    #[tokio::test]
    async fn it_bridges_get_script_to_update_script_end_to_end() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let broker_addr = listener.local_addr().unwrap();

        let mut config = mqtt_config();
        config.broker_url = format!("mqtt://{broker_addr}");

        let broker_config = config.clone();
        let broker_handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut read_buf = BytesMut::new();
            let mut subscription_count = 0usize;
            let mut saw_shadow_get = false;

            match read_packet(&mut stream, &mut read_buf).await {
                Packet::Connect(_) => write_connack(&mut stream).await,
                other => panic!("expected connect packet, got {other:?}"),
            }

            while subscription_count < 8 || !saw_shadow_get {
                match read_packet(&mut stream, &mut read_buf).await {
                    Packet::Subscribe(subscribe) => {
                        subscription_count += 1;
                        write_suback(&mut stream, subscribe.pkid).await;
                    }
                    Packet::Publish(publish) => {
                        assert_eq!(publish.topic, crate::topics::shadow_get(&broker_config));
                        assert!(publish.payload.is_empty());
                        assert_eq!(publish.qos, QoS::AtMostOnce);
                        assert!(!publish.retain);
                        saw_shadow_get = true;
                    }
                    other => panic!("unexpected mqtt packet before script request: {other:?}"),
                }
            }

            let script_request = Publish::new(
                crate::topics::script_get(&broker_config),
                QoS::AtMostOnce,
                serde_json::to_vec(&json!({
                    "requestId": "req-1",
                    "timestamp": 10,
                    "fleetId": "12",
                    "deviceUUID": "5000001",
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
            );
            write_publish(&mut stream, script_request).await;

            loop {
                match read_packet(&mut stream, &mut read_buf).await {
                    Packet::Publish(publish) => {
                        if publish.topic != crate::topics::script_update(&broker_config) {
                            continue;
                        }

                        assert_eq!(publish.qos, QoS::AtMostOnce);
                        assert!(!publish.retain);

                        let payload: serde_json::Value =
                            serde_json::from_slice(&publish.payload).unwrap();
                        assert_eq!(payload["requestId"], "req-1");
                        assert_eq!(payload["order"]["name"], "script");
                        assert_eq!(payload["order"]["value"]["scriptName"], "my-script");
                        assert_eq!(payload["order"]["value"]["version"], "1.0.0");
                        assert_eq!(payload["order"]["value"]["code"], 0);
                        assert_eq!(payload["order"]["value"]["msg"], "hello");
                        break;
                    }
                    Packet::PingReq => {}
                    other => panic!("unexpected mqtt packet after script request: {other:?}"),
                }
            }
        });

        let (mqtt_inbound_tx, mqtt_inbound_rx) = mpsc::channel(32);
        let (mqtt_outbound_tx, mqtt_outbound_rx) = mpsc::channel(32);
        let (seek_tx, _seek_rx) = watch::channel(SeekRequest::default());
        let (_local_state_tx, local_state_rx) = watch::channel(local_state());
        let (metrics_trigger_tx, _metrics_trigger_rx) = mpsc::channel(4);
        let (_metrics_tx, metrics_rx) = watch::channel(HostMetricsSnapshot::default());

        let client_handle = tokio::spawn(start_client(
            config.clone(),
            mqtt_inbound_tx,
            mqtt_outbound_rx,
        ));
        let runtime_handle = tokio::spawn(crate::start(
            config,
            None,
            mqtt_inbound_rx,
            seek_tx,
            local_state_rx,
            metrics_trigger_tx,
            metrics_rx,
            mqtt_outbound_tx,
        ));

        timeout(Duration::from_secs(10), broker_handle)
            .await
            .unwrap()
            .unwrap();

        client_handle.abort();
        runtime_handle.abort();
    }
}
