use std::net::{IpAddr, UdpSocket};
use std::process::Command;

#[cfg(target_os = "linux")]
use crate::ReservedMetricsConfig;
use crate::{CoreHostMetrics, ReservedHostMetrics};

#[derive(Debug, Default)]
pub struct Sampler {
    #[cfg(target_os = "linux")]
    config: ReservedMetricsConfig,
    #[cfg(target_os = "linux")]
    previous_cpu: Option<CpuCounters>,
}

impl Sampler {
    pub fn new(config: crate::ReservedMetricsConfig) -> Self {
        #[cfg(not(target_os = "linux"))]
        let _ = config;

        Self {
            #[cfg(target_os = "linux")]
            config,
            #[cfg(target_os = "linux")]
            previous_cpu: None,
        }
    }

    pub fn sample(&mut self) -> (CoreHostMetrics, ReservedHostMetrics) {
        (self.sample_core(), self.sample_reserved())
    }

    fn sample_core(&mut self) -> CoreHostMetrics {
        #[cfg(target_os = "linux")]
        {
            sample_linux_core(&mut self.previous_cpu)
        }

        #[cfg(target_os = "macos")]
        {
            sample_macos_core()
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            sample_fallback_core()
        }
    }

    fn sample_reserved(&self) -> ReservedHostMetrics {
        #[cfg(target_os = "linux")]
        {
            sample_linux_reserved(&self.config)
        }

        #[cfg(not(target_os = "linux"))]
        {
            ReservedHostMetrics::default()
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy)]
struct CpuCounters {
    idle: u64,
    total: u64,
}

#[cfg(target_os = "linux")]
fn sample_linux_core(previous_cpu: &mut Option<CpuCounters>) -> CoreHostMetrics {
    let mut metrics = CoreHostMetrics::default();
    let (sd_total_mb, sd_used_mb, flash_total_mb, flash_used_mb) =
        read_storage_usage_mb().unwrap_or((0.0, 0.0, 0.0, 0.0));

    let (memory_total_mb, memory_used_mb) = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|meminfo| parse_linux_meminfo_usage_mb(&meminfo))
        .unwrap_or((0.0, 0.0));

    metrics.memory_total_mb = memory_total_mb;
    metrics.memory_used_mb = memory_used_mb;
    metrics.sd_total_mb = sd_total_mb;
    metrics.sd_used_mb = sd_used_mb;
    metrics.flash_total_mb = flash_total_mb;
    metrics.flash_used_mb = flash_used_mb;
    metrics.cpu_used_percent = std::fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|stat| parse_linux_cpu_counters(&stat))
        .and_then(|current| {
            let percent = previous_cpu.and_then(|previous| {
                let total_delta = current.total.saturating_sub(previous.total);
                let idle_delta = current.idle.saturating_sub(previous.idle);
                (total_delta > 0).then(|| {
                    let busy_delta = total_delta.saturating_sub(idle_delta);
                    (busy_delta as f64 / total_delta as f64) * 100.0
                })
            });

            *previous_cpu = Some(current);
            percent
        })
        .unwrap_or(0.0);
    metrics.local_ip = read_local_ips();
    metrics
}

#[cfg(target_os = "linux")]
fn sample_linux_reserved(config: &ReservedMetricsConfig) -> ReservedHostMetrics {
    let mut metrics = ReservedHostMetrics::default();
    if config.cpu_temperature_enable {
        metrics.cpu_temperature_c = read_linux_cpu_temperature_c().unwrap_or(99_999.0);
    }
    metrics
}

#[cfg(target_os = "macos")]
fn sample_macos_core() -> CoreHostMetrics {
    let mut metrics = CoreHostMetrics::default();
    let (sd_total_mb, sd_used_mb, flash_total_mb, flash_used_mb) =
        read_storage_usage_mb().unwrap_or((0.0, 0.0, 0.0, 0.0));

    metrics.memory_total_mb = read_macos_memory_total_mb().unwrap_or(0.0);
    metrics.memory_used_mb = read_macos_memory_used_mb().unwrap_or(0.0);
    metrics.sd_total_mb = sd_total_mb;
    metrics.sd_used_mb = sd_used_mb;
    metrics.flash_total_mb = flash_total_mb;
    metrics.flash_used_mb = flash_used_mb;
    metrics.cpu_used_percent = read_macos_cpu_used_percent().unwrap_or(0.0);
    metrics.local_ip = read_local_ips();
    metrics
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn sample_fallback_core() -> CoreHostMetrics {
    let mut metrics = CoreHostMetrics::default();
    let (sd_total_mb, sd_used_mb, flash_total_mb, flash_used_mb) =
        read_storage_usage_mb().unwrap_or((0.0, 0.0, 0.0, 0.0));

    metrics.sd_total_mb = sd_total_mb;
    metrics.sd_used_mb = sd_used_mb;
    metrics.flash_total_mb = flash_total_mb;
    metrics.flash_used_mb = flash_used_mb;
    metrics.local_ip = read_local_ips();
    metrics
}

fn read_storage_usage_mb() -> Option<(f64, f64, f64, f64)> {
    let output = Command::new("df").args(["-k", "/"]).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    parse_df_usage_mb(&stdout)
}

fn parse_df_usage_mb(stdout: &str) -> Option<(f64, f64, f64, f64)> {
    let line = stdout.lines().find(|line| !line.trim().is_empty())?;
    let data_line = if line.starts_with("Filesystem") {
        stdout.lines().find(|candidate| {
            !candidate.trim().is_empty() && !candidate.trim_start().starts_with("Filesystem")
        })?
    } else {
        line
    };

    let columns: Vec<_> = data_line.split_whitespace().collect();
    if columns.len() < 5 {
        return None;
    }

    let total_mb = kb_to_mb(columns[1].parse::<u64>().ok()?);
    let used_mb = kb_to_mb(columns[2].parse::<u64>().ok()?);
    Some((total_mb, used_mb, total_mb, used_mb))
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

#[cfg(target_os = "linux")]
fn parse_linux_meminfo_usage_mb(meminfo: &str) -> Option<(f64, f64)> {
    let mut total_kb = None;
    let mut available_kb = None;

    for line in meminfo.lines() {
        if let Some(value) = line.strip_prefix("MemTotal:") {
            total_kb = parse_kb_value(value);
        }
        if let Some(value) = line.strip_prefix("MemAvailable:") {
            available_kb = parse_kb_value(value);
        }
    }

    let total_kb = total_kb?;
    let used_kb = total_kb.saturating_sub(available_kb.unwrap_or_default());
    Some((kb_to_mb(total_kb), kb_to_mb(used_kb)))
}

#[cfg(target_os = "linux")]
fn parse_linux_cpu_counters(stat: &str) -> Option<CpuCounters> {
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

    Some(CpuCounters {
        idle: idle + iowait,
        total: user + nice + system + idle + iowait + irq + softirq + steal,
    })
}

#[cfg(target_os = "linux")]
fn read_linux_cpu_temperature_c() -> Option<f64> {
    let thermal_dir = std::path::Path::new("/sys/class/thermal");
    let entries = std::fs::read_dir(thermal_dir).ok()?;

    for entry in entries.flatten() {
        let temp_path = entry.path().join("temp");
        if let Ok(value) = std::fs::read_to_string(temp_path)
            && let Ok(raw_value) = value.trim().parse::<f64>()
        {
            return Some(raw_value / 1000.0);
        }
    }

    None
}

#[cfg(target_os = "macos")]
fn read_macos_memory_total_mb() -> Option<f64> {
    let output = Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let bytes = String::from_utf8(output.stdout).ok()?;
    let total_bytes = bytes.trim().parse::<u64>().ok()?;
    Some(bytes_to_mb(total_bytes))
}

#[cfg(target_os = "macos")]
fn read_macos_memory_used_mb() -> Option<f64> {
    let output = Command::new("vm_stat").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    parse_macos_vm_stat_usage_mb(&stdout)
}

#[cfg(target_os = "macos")]
fn read_macos_cpu_used_percent() -> Option<f64> {
    let output = Command::new("top")
        .args(["-l", "1", "-n", "0"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    parse_macos_cpu_usage_percent(&stdout)
}

#[cfg(target_os = "macos")]
fn parse_macos_vm_stat_usage_mb(vm_stat: &str) -> Option<f64> {
    let page_size_bytes = vm_stat
        .lines()
        .find_map(|line| {
            let prefix = "Mach Virtual Memory Statistics: (page size of ";
            let suffix = " bytes)";
            let page_size = line
                .trim()
                .strip_prefix(prefix)?
                .strip_suffix(suffix)?
                .trim();
            page_size.parse::<u64>().ok()
        })
        .unwrap_or(4096);

    let mut active_pages = 0_u64;
    let mut wired_pages = 0_u64;
    let mut compressed_pages = 0_u64;

    for line in vm_stat.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("Pages active:") {
            active_pages = parse_count_value(value).unwrap_or_default();
        } else if let Some(value) = trimmed.strip_prefix("Pages wired down:") {
            wired_pages = parse_count_value(value).unwrap_or_default();
        } else if let Some(value) = trimmed.strip_prefix("Pages occupied by compressor:") {
            compressed_pages = parse_count_value(value).unwrap_or_default();
        }
    }

    let used_bytes = (active_pages + wired_pages + compressed_pages) * page_size_bytes;
    Some(bytes_to_mb(used_bytes))
}

#[cfg(target_os = "macos")]
fn parse_macos_cpu_usage_percent(top_output: &str) -> Option<f64> {
    let line = top_output
        .lines()
        .find(|line| line.trim_start().starts_with("CPU usage:"))?;

    let idle_percent = line.split(',').find_map(|component| {
        let component = component.trim();
        let idle_value = component
            .strip_prefix("CPU usage: ")
            .unwrap_or(component)
            .strip_suffix("% idle")?
            .trim();
        idle_value.parse::<f64>().ok()
    })?;

    Some((100.0 - idle_percent).clamp(0.0, 100.0))
}

#[cfg(target_os = "linux")]
fn parse_kb_value(value: &str) -> Option<u64> {
    value.split_whitespace().next()?.parse().ok()
}

fn kb_to_mb(value_kb: u64) -> f64 {
    value_kb as f64 / 1000.0
}

fn bytes_to_mb(value_bytes: u64) -> f64 {
    value_bytes as f64 / 1_000_000.0
}

#[cfg(target_os = "macos")]
fn parse_count_value(value: &str) -> Option<u64> {
    value
        .trim()
        .trim_end_matches('.')
        .replace('.', "")
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn df_parser_reads_storage_usage() {
        let parsed = parse_df_usage_mb(
            "Filesystem  1024-blocks     Used Available Capacity iused ifree %iused  Mounted on\n/dev/disk3s1    100000000 20000000 80000000    20%  10000 50000   17%   /\n",
        )
        .unwrap();

        assert_eq!(parsed, (100_000.0, 20_000.0, 100_000.0, 20_000.0));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_meminfo_parser_reads_usage() {
        let parsed = parse_linux_meminfo_usage_mb(
            "MemTotal:       2048000 kB\nMemAvailable:   1024000 kB\n",
        )
        .unwrap();

        assert_eq!(parsed, (2048.0, 1024.0));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_cpu_parser_reads_counters() {
        let counters = parse_linux_cpu_counters("cpu  10 20 30 40 50 60 70 80\n").unwrap();
        assert_eq!(counters.idle, 90);
        assert_eq!(counters.total, 360);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_vm_stat_parser_reads_usage() {
        let parsed = parse_macos_vm_stat_usage_mb(
            "Mach Virtual Memory Statistics: (page size of 4096 bytes)\nPages active:                               1000.\nPages wired down:                           500.\nPages occupied by compressor:               250.\n",
        )
        .unwrap();

        assert!((parsed - 7.168).abs() < 0.001);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_cpu_parser_reads_used_percent() {
        let parsed = parse_macos_cpu_usage_percent(
            "Processes: 123 total\nCPU usage: 7.38% user, 9.15% sys, 83.47% idle\n",
        )
        .unwrap();

        assert!((parsed - 16.53).abs() < 0.001);
    }
}
