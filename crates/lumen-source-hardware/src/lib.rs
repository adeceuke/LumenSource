//! Linux hardware capacity detection and lightweight usage sampling.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::fs;
use tokio::process::Command;

const KIB: u64 = 1024;

/// Static and slowly-changing facts used to decide whether a model can run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HardwareFacts {
    pub os: OsFacts,
    pub cpu: CpuFacts,
    pub memory: MemoryFacts,
    pub total_ram_bytes: u64,
    pub available_ram_bytes: u64,
    pub storage: StorageFacts,
    pub accelerators: Vec<AcceleratorFacts>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsFacts {
    pub family: String,
    pub distribution: Option<String>,
    pub version: Option<String>,
    pub architecture: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuFacts {
    pub model: Option<String>,
    pub architecture: String,
    pub logical_cores: usize,
    pub physical_cores: Option<usize>,
    /// Best available reported CPU frequency. On Linux this prefers the
    /// firmware/kernel maximum and falls back to `/proc/cpuinfo`.
    pub frequency_mhz: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryFacts {
    /// Normalized memory generation, for example `DDR4` or `LPDDR5`.
    pub kind: Option<String>,
    /// Effective transfer rate. DDR memory vendors commonly label this as
    /// MHz, but MT/s is the technically accurate unit.
    pub speed_mts: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageFacts {
    pub mount_point: PathBuf,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceleratorKind {
    Nvidia,
    Amd,
    Intel,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceleratorFacts {
    pub kind: AcceleratorKind,
    pub name: String,
    pub total_vram_bytes: Option<u64>,
    pub driver_version: Option<String>,
}

/// A short-lived sample for right-sizing recommendations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageSnapshot {
    pub sampled_at_unix_ms: u64,
    pub cpu_utilization_percent: f32,
    pub used_ram_bytes: u64,
    pub available_ram_bytes: u64,
    pub accelerators: Vec<AcceleratorUsage>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AcceleratorUsage {
    pub kind: AcceleratorKind,
    pub name: String,
    pub utilization_percent: Option<f32>,
    pub used_vram_bytes: Option<u64>,
}

#[derive(Debug, Error)]
pub enum ProbeError {
    #[error("cannot read {interface}: {source}")]
    Interface {
        interface: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid data from {interface}: {detail}")]
    InvalidData {
        interface: &'static str,
        detail: String,
    },
}

#[async_trait]
pub trait HardwareProbe: Send + Sync {
    async fn hardware_facts(&self) -> Result<HardwareFacts, ProbeError>;
    async fn usage_snapshot(&self) -> Result<UsageSnapshot, ProbeError>;
}

#[cfg(target_os = "linux")]
pub type PlatformHardwareProbe = LinuxHardwareProbe;

#[cfg(not(target_os = "linux"))]
#[derive(Clone, Debug, Default)]
pub struct PlatformHardwareProbe;

#[cfg(not(target_os = "linux"))]
#[async_trait]
impl HardwareProbe for PlatformHardwareProbe {
    async fn hardware_facts(&self) -> Result<HardwareFacts, ProbeError> {
        Err(ProbeError::InvalidData {
            interface: "platform adapter",
            detail: "hardware detection is not implemented for this operating system".to_owned(),
        })
    }

    async fn usage_snapshot(&self) -> Result<UsageSnapshot, ProbeError> {
        Err(ProbeError::InvalidData {
            interface: "platform adapter",
            detail: "hardware usage sampling is not implemented for this operating system"
                .to_owned(),
        })
    }
}

/// Linux probe restricted to `/proc`, `/sys`, `df`, and optional vendor GPU tools.
#[derive(Clone, Debug)]
pub struct LinuxHardwareProbe {
    storage_path: PathBuf,
    cpu_sample_interval: Duration,
}

impl Default for LinuxHardwareProbe {
    fn default() -> Self {
        Self {
            storage_path: PathBuf::from("/"),
            cpu_sample_interval: Duration::from_millis(100),
        }
    }
}

impl LinuxHardwareProbe {
    pub fn new(storage_path: impl Into<PathBuf>) -> Self {
        Self {
            storage_path: storage_path.into(),
            ..Self::default()
        }
    }

    pub fn with_cpu_sample_interval(mut self, interval: Duration) -> Self {
        self.cpu_sample_interval = interval;
        self
    }
}

#[async_trait]
impl HardwareProbe for LinuxHardwareProbe {
    async fn hardware_facts(&self) -> Result<HardwareFacts, ProbeError> {
        let (cpuinfo, cpu_frequency, meminfo, memory_hardware, os_release, storage, accelerators) = tokio::join!(
            read_interface("/proc/cpuinfo", "/proc/cpuinfo"),
            read_optional("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq"),
            read_interface("/proc/meminfo", "/proc/meminfo"),
            detect_memory_hardware(),
            read_optional("/etc/os-release"),
            storage_facts(&self.storage_path),
            detect_accelerators(),
        );
        let memory = parse_meminfo(&meminfo?)?;
        let mut cpu = parse_cpuinfo(&cpuinfo?);
        if let Some(frequency) = cpu_frequency.as_deref().and_then(parse_frequency_khz) {
            cpu.frequency_mhz = Some(frequency);
        }

        Ok(HardwareFacts {
            os: parse_os_release(os_release.as_deref()),
            cpu,
            memory: memory_hardware,
            total_ram_bytes: memory.total,
            available_ram_bytes: memory.available,
            storage: storage?,
            // Missing or broken GPU tooling is deliberately a CPU-only result.
            accelerators,
        })
    }

    async fn usage_snapshot(&self) -> Result<UsageSnapshot, ProbeError> {
        let first = read_interface("/proc/stat", "/proc/stat").await?;
        tokio::time::sleep(self.cpu_sample_interval).await;
        let (second, meminfo, accelerators) = tokio::join!(
            read_interface("/proc/stat", "/proc/stat"),
            read_interface("/proc/meminfo", "/proc/meminfo"),
            sample_accelerators(),
        );
        let memory = parse_meminfo(&meminfo?)?;

        Ok(UsageSnapshot {
            sampled_at_unix_ms: unix_time_ms(),
            cpu_utilization_percent: cpu_utilization(&first, &second?)?,
            used_ram_bytes: memory.total.saturating_sub(memory.available),
            available_ram_bytes: memory.available,
            accelerators,
        })
    }
}

async fn read_interface(
    path: impl AsRef<Path>,
    interface: &'static str,
) -> Result<String, ProbeError> {
    fs::read_to_string(path)
        .await
        .map_err(|source| ProbeError::Interface { interface, source })
}

async fn read_optional(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).await.ok()
}

#[derive(Clone, Copy)]
struct Memory {
    total: u64,
    available: u64,
}

fn parse_meminfo(input: &str) -> Result<Memory, ProbeError> {
    let value = |key: &str| {
        input.lines().find_map(|line| {
            let (name, rest) = line.split_once(':')?;
            if name == key {
                rest.split_whitespace().next()?.parse::<u64>().ok()
            } else {
                None
            }
        })
    };
    let total = value("MemTotal").ok_or_else(|| ProbeError::InvalidData {
        interface: "/proc/meminfo",
        detail: "MemTotal is absent".to_owned(),
    })?;
    let available = value("MemAvailable")
        .or_else(|| value("MemFree"))
        .ok_or_else(|| ProbeError::InvalidData {
            interface: "/proc/meminfo",
            detail: "MemAvailable and MemFree are absent".to_owned(),
        })?;
    Ok(Memory {
        total: total.saturating_mul(KIB),
        available: available.saturating_mul(KIB),
    })
}

fn parse_cpuinfo(input: &str) -> CpuFacts {
    let logical_cores = input
        .lines()
        .filter(|line| line.starts_with("processor"))
        .count()
        .max(1);
    let field = |key: &str| {
        input.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            (name.trim() == key).then(|| value.trim().to_owned())
        })
    };
    let physical_cores = field("cpu cores").and_then(|value| value.parse().ok());
    let frequency_mhz = field("cpu MHz")
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| value.round() as u64);

    CpuFacts {
        model: field("model name")
            .or_else(|| field("Hardware"))
            .or_else(|| field("Processor")),
        architecture: std::env::consts::ARCH.to_owned(),
        logical_cores,
        physical_cores,
        frequency_mhz,
    }
}

fn parse_frequency_khz(input: &str) -> Option<u64> {
    input
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .map(|value| value / 1_000)
        .filter(|value| *value > 0)
}

async fn detect_memory_hardware() -> MemoryFacts {
    let (sysfs, dmidecode) = tokio::join!(read_edac_memory(), read_dmidecode_memory());
    MemoryFacts {
        kind: sysfs.kind.or(dmidecode.kind),
        speed_mts: sysfs.speed_mts.or(dmidecode.speed_mts),
    }
}

async fn read_edac_memory() -> MemoryFacts {
    let mut result = MemoryFacts::default();
    let mut controllers = match fs::read_dir("/sys/devices/system/edac/mc").await {
        Ok(entries) => entries,
        Err(_) => return result,
    };

    while let Ok(Some(controller)) = controllers.next_entry().await {
        let mut dimms = match fs::read_dir(controller.path()).await {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        while let Ok(Some(dimm)) = dimms.next_entry().await {
            if !dimm.file_name().to_string_lossy().starts_with("dimm") {
                continue;
            }
            if result.kind.is_none() {
                result.kind = read_optional(dimm.path().join("dimm_mem_type"))
                    .await
                    .as_deref()
                    .and_then(normalize_memory_kind);
            }
            if result.speed_mts.is_none() {
                result.speed_mts = read_optional(dimm.path().join("dimm_speed"))
                    .await
                    .as_deref()
                    .and_then(parse_memory_speed);
            }
            if result.kind.is_some() && result.speed_mts.is_some() {
                return result;
            }
        }
    }
    result
}

async fn read_dmidecode_memory() -> MemoryFacts {
    let output = match Command::new("dmidecode")
        .args(["--type", "17"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await
    {
        Ok(output) if output.status.success() => output,
        _ => return MemoryFacts::default(),
    };
    parse_dmidecode_memory(&String::from_utf8_lossy(&output.stdout))
}

fn parse_dmidecode_memory(input: &str) -> MemoryFacts {
    let kind = input.lines().find_map(|line| {
        line.trim()
            .strip_prefix("Type:")
            .and_then(normalize_memory_kind)
    });
    let configured_speed = input.lines().find_map(|line| {
        line.trim()
            .strip_prefix("Configured Memory Speed:")
            .and_then(parse_memory_speed)
    });
    let speed = configured_speed.or_else(|| {
        input.lines().find_map(|line| {
            line.trim()
                .strip_prefix("Speed:")
                .and_then(parse_memory_speed)
        })
    });
    MemoryFacts {
        kind,
        speed_mts: speed,
    }
}

fn normalize_memory_kind(input: &str) -> Option<String> {
    let normalized = input.trim().to_ascii_uppercase().replace([' ', '_'], "-");
    if normalized.is_empty() || normalized.contains("UNKNOWN") {
        return None;
    }
    for generation in ["5", "4", "3", "2"] {
        if normalized.contains(&format!("LPDDR{generation}"))
            || normalized.contains(&format!("LOW-POWER-DDR{generation}"))
        {
            return Some(format!("LPDDR{generation}"));
        }
        if normalized.contains(&format!("DDR{generation}")) {
            return Some(format!("DDR{generation}"));
        }
    }
    None
}

fn parse_memory_speed(input: &str) -> Option<u64> {
    input
        .split_whitespace()
        .find_map(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
}

fn parse_os_release(input: Option<&str>) -> OsFacts {
    let field = |key: &str| {
        input.and_then(|text| {
            text.lines().find_map(|line| {
                let (name, value) = line.split_once('=')?;
                (name == key).then(|| value.trim().trim_matches('"').to_owned())
            })
        })
    };
    OsFacts {
        family: "linux".to_owned(),
        distribution: field("ID"),
        version: field("VERSION_ID"),
        architecture: std::env::consts::ARCH.to_owned(),
    }
}

async fn storage_facts(path: &Path) -> Result<StorageFacts, ProbeError> {
    let output = Command::new("df")
        .args(["-Pk", "--"])
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await
        .map_err(|source| ProbeError::Interface {
            interface: "df",
            source,
        })?;
    if !output.status.success() {
        return Err(ProbeError::InvalidData {
            interface: "df",
            detail: format!("exited with {}", output.status),
        });
    }
    parse_df(&String::from_utf8_lossy(&output.stdout), path)
}

fn parse_df(input: &str, path: &Path) -> Result<StorageFacts, ProbeError> {
    let columns: Vec<&str> = input
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.split_whitespace().collect())
        .unwrap_or_default();
    let parse_column = |index: usize| {
        columns
            .get(index)
            .and_then(|value| value.parse::<u64>().ok())
            .map(|value| value.saturating_mul(KIB))
    };
    let total_bytes = parse_column(1).ok_or_else(|| ProbeError::InvalidData {
        interface: "df",
        detail: "total block count is absent".to_owned(),
    })?;
    let available_bytes = parse_column(3).ok_or_else(|| ProbeError::InvalidData {
        interface: "df",
        detail: "available block count is absent".to_owned(),
    })?;
    Ok(StorageFacts {
        mount_point: path.to_owned(),
        total_bytes,
        available_bytes,
    })
}

#[derive(Clone, Copy)]
struct CpuTicks {
    idle: u64,
    total: u64,
}

fn parse_cpu_ticks(input: &str) -> Option<CpuTicks> {
    let line = input.lines().find(|line| line.starts_with("cpu "))?;
    let ticks: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|value| value.parse().ok())
        .collect();
    let total = ticks.iter().copied().fold(0_u64, u64::saturating_add);
    let idle = ticks
        .first()
        .and_then(|_| ticks.get(3))
        .copied()
        .unwrap_or_default()
        .saturating_add(ticks.get(4).copied().unwrap_or_default());
    Some(CpuTicks { idle, total })
}

fn cpu_utilization(first: &str, second: &str) -> Result<f32, ProbeError> {
    let before = parse_cpu_ticks(first);
    let after = parse_cpu_ticks(second);
    match (before, after) {
        (Some(before), Some(after)) => {
            let total = after.total.saturating_sub(before.total);
            let idle = after.idle.saturating_sub(before.idle);
            if total == 0 {
                Ok(0.0)
            } else {
                Ok(100.0 * (total.saturating_sub(idle) as f32 / total as f32))
            }
        }
        _ => Err(ProbeError::InvalidData {
            interface: "/proc/stat",
            detail: "aggregate CPU row is absent".to_owned(),
        }),
    }
}

async fn detect_accelerators() -> Vec<AcceleratorFacts> {
    let mut devices = nvidia_facts().await.unwrap_or_default();
    if command_succeeds("rocminfo", &[]).await {
        devices.extend(
            sysfs_accelerators()
                .await
                .into_iter()
                .filter(|device| device.kind == AcceleratorKind::Amd),
        );
    }
    devices
}

async fn command_succeeds(program: &str, args: &[&str]) -> bool {
    matches!(
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await,
        Ok(status) if status.success()
    )
}

async fn nvidia_facts() -> Option<Vec<AcceleratorFacts>> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total,driver_version",
            "--format=csv,noheader,nounits",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let devices: Vec<_> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut columns = line.split(',').map(str::trim);
            Some(AcceleratorFacts {
                kind: AcceleratorKind::Nvidia,
                name: columns.next()?.to_owned(),
                total_vram_bytes: columns
                    .next()
                    .and_then(|value| value.parse::<u64>().ok())
                    .map(|mib| mib.saturating_mul(1024 * 1024)),
                driver_version: columns.next().map(str::to_owned),
            })
        })
        .collect();
    (!devices.is_empty()).then_some(devices)
}

async fn sysfs_accelerators() -> Vec<AcceleratorFacts> {
    let mut devices = Vec::new();
    let mut entries = match fs::read_dir("/sys/class/drm").await {
        Ok(entries) => entries,
        Err(_) => return devices,
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let vendor = read_optional(entry.path().join("device/vendor")).await;
        let kind = match vendor.as_deref().map(str::trim) {
            Some("0x10de") => AcceleratorKind::Nvidia,
            Some("0x1002") => AcceleratorKind::Amd,
            Some("0x8086") => AcceleratorKind::Intel,
            Some(_) => AcceleratorKind::Other,
            None => continue,
        };
        devices.push(AcceleratorFacts {
            kind,
            name,
            total_vram_bytes: read_sysfs_vram(&entry.path()).await,
            driver_version: None,
        });
    }
    devices
}

async fn read_sysfs_vram(card_path: &Path) -> Option<u64> {
    let text = read_optional(card_path.join("device/mem_info_vram_total")).await?;
    text.trim().parse().ok()
}

async fn sample_accelerators() -> Vec<AcceleratorUsage> {
    let output = match Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,utilization.gpu,memory.used",
            "--format=csv,noheader,nounits",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await
    {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut columns = line.split(',').map(str::trim);
            Some(AcceleratorUsage {
                kind: AcceleratorKind::Nvidia,
                name: columns.next()?.to_owned(),
                utilization_percent: columns.next().and_then(|value| value.parse().ok()),
                used_vram_bytes: columns
                    .next()
                    .and_then(|value| value.parse::<u64>().ok())
                    .map(|mib| mib.saturating_mul(1024 * 1024)),
            })
        })
        .collect()
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_golden_linux_profile() {
        let memory = parse_meminfo(
            "MemTotal:       32768000 kB\nMemFree: 100 kB\nMemAvailable: 24576000 kB\n",
        );
        assert!(matches!(
            memory,
            Ok(Memory {
                total: 33_554_432_000,
                available: 25_165_824_000
            })
        ));

        let cpu = parse_cpuinfo(
            "processor : 0\nmodel name : Golden Lake\ncpu cores : 8\ncpu MHz : 3199.75\nprocessor : 1\n",
        );
        assert_eq!(cpu.model.as_deref(), Some("Golden Lake"));
        assert_eq!(cpu.logical_cores, 2);
        assert_eq!(cpu.physical_cores, Some(8));
        assert_eq!(cpu.frequency_mhz, Some(3200));
    }

    #[test]
    fn parses_memory_type_and_configured_transfer_rate() {
        let memory = parse_dmidecode_memory(
            "Memory Device\n\tType: DDR5\n\tSpeed: 5600 MT/s\n\tConfigured Memory Speed: 5200 MT/s\n",
        );
        assert_eq!(memory.kind.as_deref(), Some("DDR5"));
        assert_eq!(memory.speed_mts, Some(5200));
        assert_eq!(
            normalize_memory_kind("Low-Power-DDR3-RAM").as_deref(),
            Some("LPDDR3")
        );
    }

    #[test]
    fn computes_cpu_usage_from_golden_samples() {
        let usage = cpu_utilization("cpu  10 0 10 80 0\n", "cpu  30 0 20 150 0\n");
        assert!(matches!(usage, Ok(value) if (value - 30.0).abs() < 0.001));
    }

    #[test]
    fn parses_storage_without_panicking_on_device_names() {
        let result = parse_df(
            "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/nvme0n1p2 1000 250 750 25% /\n",
            Path::new("/"),
        );
        assert!(matches!(
            result,
            Ok(StorageFacts {
                total_bytes: 1_024_000,
                available_bytes: 768_000,
                ..
            })
        ));
    }
}
