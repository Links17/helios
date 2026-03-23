use std::fs;
use std::net::{IpAddr, UdpSocket};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use helios_util::ticker;
use serde::Serialize;
use tokio::sync::{mpsc, watch};

#[derive(Debug, Clone)]
pub struct HostMetricsConfig {
    pub sample_interval: Duration,
}

impl Default for HostMetricsConfig {
    fn default() -> Self {
        Self {
            sample_interval: Duration::from_secs(300),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricsTrigger {
    Immediate,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HostMetricsSnapshot {
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

impl Default for HostMetricsSnapshot {
    fn default() -> Self {
        Self {
            timestamp_ms: now_millis(),
            memory_total_mb: 0.0,
            memory_used_mb: 0.0,
            sd_total_mb: 0.0,
            sd_used_mb: 0.0,
            flash_total_mb: 0.0,
            flash_used_mb: 0.0,
            cpu_temperature_c: 99_999.0,
            cpu_used_percent: 0.0,
            local_ip: Vec::new(),
            public_ip: Vec::new(),
            latitude: 0.0,
            longitude: 0.0,
            altitude: 0.0,
            lora_modem: None,
            lte: None,
            gps: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CpuCounters {
    idle: u64,
    total: u64,
}

pub async fn start(
    config: HostMetricsConfig,
    mut trigger_rx: mpsc::Receiver<MetricsTrigger>,
    snapshot_tx: watch::Sender<HostMetricsSnapshot>,
) {
    let mut collector = Collector::default();
    collector.publish(&snapshot_tx);

    let mut interval = ticker::delayed_interval(config.sample_interval);

    loop {
        tokio::select! {
            _ = interval.tick() => collector.publish(&snapshot_tx),
            Some(MetricsTrigger::Immediate) = trigger_rx.recv() => collector.publish(&snapshot_tx),
            else => return,
        }
    }
}

#[derive(Default)]
struct Collector {
    previous_cpu: Option<CpuCounters>,
}

impl Collector {
    fn publish(&mut self, snapshot_tx: &watch::Sender<HostMetricsSnapshot>) {
        let snapshot = self.sample();
        let _ = snapshot_tx.send(snapshot);
    }

    fn sample(&mut self) -> HostMetricsSnapshot {
        let (memory_total_mb, memory_used_mb) = read_memory_usage_mb().unwrap_or((0.0, 0.0));
        let (sd_total_mb, sd_used_mb, flash_total_mb, flash_used_mb) =
            read_storage_usage_mb().unwrap_or((0.0, 0.0, 0.0, 0.0));
        let cpu_temperature_c = read_cpu_temperature_c().unwrap_or(99_999.0);
        let cpu_used_percent = self.read_cpu_used_percent().unwrap_or(0.0);

        HostMetricsSnapshot {
            timestamp_ms: now_millis(),
            memory_total_mb,
            memory_used_mb,
            sd_total_mb,
            sd_used_mb,
            flash_total_mb,
            flash_used_mb,
            cpu_temperature_c,
            cpu_used_percent,
            local_ip: read_local_ips(),
            public_ip: Vec::new(),
            latitude: 0.0,
            longitude: 0.0,
            altitude: 0.0,
            lora_modem: None,
            lte: None,
            gps: None,
        }
    }

    fn read_cpu_used_percent(&mut self) -> Option<f64> {
        let current = read_cpu_counters()?;
        let percent = self.previous_cpu.and_then(|previous| {
            let total_delta = current.total.saturating_sub(previous.total);
            let idle_delta = current.idle.saturating_sub(previous.idle);
            (total_delta > 0).then(|| {
                let busy_delta = total_delta.saturating_sub(idle_delta);
                (busy_delta as f64 / total_delta as f64) * 100.0
            })
        });

        self.previous_cpu = Some(current);
        percent
    }
}

fn now_millis() -> i64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_millis(0));
    duration.as_millis() as i64
}

fn read_memory_usage_mb() -> Option<(f64, f64)> {
    let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kb = None;
    let mut available_kb = None;

    for line in meminfo.lines() {
        if let Some(value) = line.strip_prefix("MemTotal:") {
            total_kb = parse_meminfo_kb(value);
        }
        if let Some(value) = line.strip_prefix("MemAvailable:") {
            available_kb = parse_meminfo_kb(value);
        }
    }

    let total_mb = total_kb? as f64 / 1024.0;
    let used_mb = total_kb?.saturating_sub(available_kb.unwrap_or_default()) as f64 / 1024.0;
    Some((total_mb, used_mb))
}

fn parse_meminfo_kb(value: &str) -> Option<u64> {
    value.split_whitespace().next()?.parse().ok()
}

fn read_storage_usage_mb() -> Option<(f64, f64, f64, f64)> {
    let output = std::process::Command::new("df")
        .args(["-k", "/"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    let line = stdout.lines().nth(1)?;
    let columns: Vec<_> = line.split_whitespace().collect();
    if columns.len() < 5 {
        return None;
    }

    let total_mb = columns[1].parse::<u64>().ok()? as f64 / 1024.0;
    let used_mb = columns[2].parse::<u64>().ok()? as f64 / 1024.0;
    Some((total_mb, used_mb, total_mb, used_mb))
}

fn read_cpu_counters() -> Option<CpuCounters> {
    let stat = fs::read_to_string("/proc/stat").ok()?;
    let line = stat.lines().next()?;
    let mut values = line.split_whitespace();
    if values.next()? != "cpu" {
        return None;
    }

    let user = values.next()?.parse::<u64>().ok()?;
    let nice = values.next()?.parse::<u64>().ok()?;
    let system = values.next()?.parse::<u64>().ok()?;
    let idle = values.next()?.parse::<u64>().ok()?;
    let iowait = values
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default();
    let irq = values
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default();
    let softirq = values
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default();
    let steal = values
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default();

    let idle_total = idle + iowait;
    let total = user + nice + system + idle + iowait + irq + softirq + steal;
    Some(CpuCounters {
        idle: idle_total,
        total,
    })
}

fn read_cpu_temperature_c() -> Option<f64> {
    let thermal_dir = Path::new("/sys/class/thermal");
    let entries = fs::read_dir(thermal_dir).ok()?;

    for entry in entries.flatten() {
        let temp_path = entry.path().join("temp");
        if let Ok(value) = fs::read_to_string(temp_path)
            && let Ok(raw_value) = value.trim().parse::<f64>()
        {
            return Some(raw_value / 1000.0);
        }
    }

    None
}

fn read_local_ips() -> Vec<String> {
    let mut local_ips = Vec::new();

    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        let _ = socket.connect("8.8.8.8:80");
        if let Ok(local_addr) = socket.local_addr()
            && !is_unspecified_or_loopback(local_addr.ip())
        {
            local_ips.push(local_addr.ip().to_string());
        }
    }

    local_ips
}

fn is_unspecified_or_loopback(ip: IpAddr) -> bool {
    ip.is_loopback() || ip.is_unspecified()
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
    }

    #[test]
    fn meminfo_parser_reads_kb_values() {
        assert_eq!(parse_meminfo_kb("  123456 kB"), Some(123_456));
    }
}
