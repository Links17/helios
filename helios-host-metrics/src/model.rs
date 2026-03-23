use serde::Serialize;

#[derive(Debug, Clone, PartialEq)]
pub struct CoreHostMetrics {
    pub memory_total_mb: f64,
    pub memory_used_mb: f64,
    pub sd_total_mb: f64,
    pub sd_used_mb: f64,
    pub flash_total_mb: f64,
    pub flash_used_mb: f64,
    pub cpu_used_percent: f64,
    pub local_ip: Vec<String>,
}

impl Default for CoreHostMetrics {
    fn default() -> Self {
        Self {
            memory_total_mb: 0.0,
            memory_used_mb: 0.0,
            sd_total_mb: 0.0,
            sd_used_mb: 0.0,
            flash_total_mb: 0.0,
            flash_used_mb: 0.0,
            cpu_used_percent: 0.0,
            local_ip: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReservedHostMetrics {
    pub cpu_temperature_c: f64,
    pub public_ip: Vec<String>,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f64,
    pub lora_modem: Option<String>,
    pub lte: Option<u8>,
    pub gps: Option<u8>,
}

impl Default for ReservedHostMetrics {
    fn default() -> Self {
        Self {
            cpu_temperature_c: 99_999.0,
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

impl HostMetricsSnapshot {
    pub fn from_parts(
        timestamp_ms: i64,
        core: CoreHostMetrics,
        reserved: ReservedHostMetrics,
    ) -> Self {
        Self {
            timestamp_ms,
            memory_total_mb: core.memory_total_mb,
            memory_used_mb: core.memory_used_mb,
            sd_total_mb: core.sd_total_mb,
            sd_used_mb: core.sd_used_mb,
            flash_total_mb: core.flash_total_mb,
            flash_used_mb: core.flash_used_mb,
            cpu_temperature_c: reserved.cpu_temperature_c,
            cpu_used_percent: core.cpu_used_percent,
            local_ip: core.local_ip,
            public_ip: reserved.public_ip,
            latitude: reserved.latitude,
            longitude: reserved.longitude,
            altitude: reserved.altitude,
            lora_modem: reserved.lora_modem,
            lte: reserved.lte,
            gps: reserved.gps,
        }
    }
}
