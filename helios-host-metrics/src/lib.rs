mod extensions;
mod model;
mod platform;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use helios_util::ticker;
use tokio::sync::{mpsc, watch};
use tracing::debug;

pub use extensions::ReservedMetricsEnricher;
pub use model::{CoreHostMetrics, HostMetricsSnapshot, ReservedHostMetrics};

#[derive(Debug, Clone)]
pub struct HostMetricsConfig {
    pub sample_interval: Duration,
    pub reserved: ReservedMetricsConfig,
}

impl Default for HostMetricsConfig {
    fn default() -> Self {
        Self {
            sample_interval: Duration::from_secs(300),
            reserved: ReservedMetricsConfig::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReservedMetricsConfig {
    pub cpu_temperature_enable: bool,
    pub public_ip: PublicIpConfig,
}

impl Default for ReservedMetricsConfig {
    fn default() -> Self {
        Self {
            cpu_temperature_enable: true,
            public_ip: PublicIpConfig::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PublicIpConfig {
    pub endpoint: Option<String>,
    pub timeout: Duration,
}

impl Default for PublicIpConfig {
    fn default() -> Self {
        Self {
            endpoint: None,
            timeout: Duration::from_secs(2),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricsTrigger {
    Immediate,
}

impl Default for HostMetricsSnapshot {
    fn default() -> Self {
        Self::from_parts(
            now_millis(),
            CoreHostMetrics::default(),
            ReservedHostMetrics::default(),
        )
    }
}

pub async fn start(
    config: HostMetricsConfig,
    mut trigger_rx: mpsc::Receiver<MetricsTrigger>,
    snapshot_tx: watch::Sender<HostMetricsSnapshot>,
) {
    let mut collector = Collector::new(config.reserved);
    collector.publish(&snapshot_tx).await;

    let mut interval = ticker::delayed_interval(config.sample_interval);

    loop {
        tokio::select! {
            _ = interval.tick() => collector.publish(&snapshot_tx).await,
            Some(MetricsTrigger::Immediate) = trigger_rx.recv() => collector.publish(&snapshot_tx).await,
            else => return,
        }
    }
}

#[derive(Debug)]
struct Collector {
    sampler: platform::Sampler,
    enricher: ReservedMetricsEnricher,
}

impl Collector {
    fn new(config: ReservedMetricsConfig) -> Self {
        Self {
            sampler: platform::Sampler::new(config.clone()),
            enricher: ReservedMetricsEnricher::new(config),
        }
    }
}

impl Collector {
    async fn publish(&mut self, snapshot_tx: &watch::Sender<HostMetricsSnapshot>) {
        let snapshot = self.sample().await;
        let _ = snapshot_tx.send(snapshot);
    }

    async fn sample(&mut self) -> HostMetricsSnapshot {
        let (core, mut reserved) = self.sampler.sample();
        self.enricher.enrich(&mut reserved).await;
        debug!(?reserved.public_ip, "sampled reserved host metrics extensions");
        HostMetricsSnapshot::from_parts(now_millis(), core, reserved)
    }
}

fn now_millis() -> i64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_millis(0));
    duration.as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn it_samples_immediately_when_triggered() {
        let (trigger_tx, trigger_rx) = mpsc::channel(4);
        let (snapshot_tx, mut snapshot_rx) = watch::channel(HostMetricsSnapshot::default());

        let handle = tokio::spawn(start(
            HostMetricsConfig {
                sample_interval: Duration::from_secs(3600),
                reserved: ReservedMetricsConfig::default(),
            },
            trigger_rx,
            snapshot_tx,
        ));

        let initial_timestamp = snapshot_rx.borrow().timestamp_ms;
        trigger_tx.send(MetricsTrigger::Immediate).await.unwrap();

        tokio::time::timeout(Duration::from_secs(1), snapshot_rx.changed())
            .await
            .unwrap()
            .unwrap();

        assert!(snapshot_rx.borrow().timestamp_ms >= initial_timestamp);

        drop(trigger_tx);
        handle.abort();
        let _ = handle.await;
    }

    #[test]
    fn default_snapshot_uses_protocol_safe_values() {
        let snapshot = HostMetricsSnapshot::default();
        assert_eq!(snapshot.cpu_temperature_c, 99_999.0);
        assert!(snapshot.public_ip.is_empty());
        assert_eq!(snapshot.latitude, 0.0);
        assert_eq!(snapshot.longitude, 0.0);
        assert_eq!(snapshot.altitude, 0.0);
        assert!(snapshot.lora_modem.is_none());
        assert!(snapshot.lte.is_none());
        assert!(snapshot.gps.is_none());
    }
}
