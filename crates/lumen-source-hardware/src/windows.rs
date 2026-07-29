use super::{
    AcceleratorFacts, AcceleratorKind, AcceleratorUsage, CpuFacts, HardwareFacts, HardwareProbe,
    MemoryFacts, OsFacts, ProbeError, StorageFacts, UsageSnapshot,
};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

const FACTS_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$cpu = Get-CimInstance Win32_Processor -ErrorAction SilentlyContinue | Select-Object -First 1
$os = Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue
$registryCpu = Get-ItemProperty 'HKLM:\HARDWARE\DESCRIPTION\System\CentralProcessor\0'
$windows = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
Add-Type -AssemblyName Microsoft.VisualBasic
$computer = [Microsoft.VisualBasic.Devices.ComputerInfo]::new()
$memory = @(Get-CimInstance Win32_PhysicalMemory -ErrorAction SilentlyContinue | ForEach-Object {
  [pscustomobject]@{
    SMBIOSMemoryType = $_.SMBIOSMemoryType
    Speed = if ($_.ConfiguredClockSpeed) { $_.ConfiguredClockSpeed } else { $_.Speed }
  }
})
$drive = [System.IO.DriveInfo]::new($env:SystemDrive)
$gpus = @(Get-CimInstance Win32_VideoController -ErrorAction SilentlyContinue | ForEach-Object {
  [pscustomobject]@{
    Name = $_.Name
    AdapterRAM = $_.AdapterRAM
    DriverVersion = $_.DriverVersion
  }
})
[pscustomobject]@{
  Cpu = [pscustomobject]@{
    Name = if ($cpu.Name) { $cpu.Name } else { $registryCpu.ProcessorNameString }
    MaxClockSpeed = if ($cpu.MaxClockSpeed) { $cpu.MaxClockSpeed } else { $registryCpu.'~MHz' }
    NumberOfCores = $cpu.NumberOfCores
    NumberOfLogicalProcessors = if ($cpu.NumberOfLogicalProcessors) { $cpu.NumberOfLogicalProcessors } else { [Environment]::ProcessorCount }
  }
  Os = [pscustomobject]@{
    Caption = if ($os.Caption) { $os.Caption } else { $windows.ProductName }
    Version = if ($os.Version) { $os.Version } else { "$($windows.DisplayVersion) ($($windows.CurrentBuild))" }
    TotalVisibleMemorySize = if ($os.TotalVisibleMemorySize) { $os.TotalVisibleMemorySize } else { [math]::Floor($computer.TotalPhysicalMemory / 1KB) }
    FreePhysicalMemory = if ($os.FreePhysicalMemory) { $os.FreePhysicalMemory } else { [math]::Floor($computer.AvailablePhysicalMemory / 1KB) }
  }
  Memory = $memory
  Disk = [pscustomobject]@{
    DeviceID = $env:SystemDrive
    Size = $drive.TotalSize
    FreeSpace = $drive.AvailableFreeSpace
  }
  Gpus = $gpus
} | ConvertTo-Json -Compress -Depth 5
"#;

const USAGE_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$cpu = Get-CimInstance Win32_Processor -ErrorAction SilentlyContinue
$os = Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue
Add-Type -AssemblyName Microsoft.VisualBasic
$computer = [Microsoft.VisualBasic.Devices.ComputerInfo]::new()
$load = ($cpu | Measure-Object -Property LoadPercentage -Average).Average
[pscustomobject]@{
  CpuLoad = if ($null -ne $load) { $load } else { 0 }
  TotalVisibleMemorySize = if ($os.TotalVisibleMemorySize) { $os.TotalVisibleMemorySize } else { [math]::Floor($computer.TotalPhysicalMemory / 1KB) }
  FreePhysicalMemory = if ($os.FreePhysicalMemory) { $os.FreePhysicalMemory } else { [math]::Floor($computer.AvailablePhysicalMemory / 1KB) }
} | ConvertTo-Json -Compress
"#;

#[derive(Clone, Debug, Default)]
pub struct WindowsHardwareProbe {
    _private: (),
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WindowsFacts {
    cpu: WindowsCpu,
    os: WindowsOs,
    #[serde(default)]
    memory: Vec<WindowsMemory>,
    disk: WindowsDisk,
    #[serde(default)]
    gpus: Vec<WindowsGpu>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WindowsCpu {
    name: Option<String>,
    max_clock_speed: Option<u64>,
    number_of_cores: Option<usize>,
    number_of_logical_processors: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WindowsOs {
    caption: Option<String>,
    version: Option<String>,
    total_visible_memory_size: u64,
    free_physical_memory: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WindowsMemory {
    #[serde(rename = "SMBIOSMemoryType")]
    smbios_memory_type: Option<u16>,
    speed: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WindowsDisk {
    #[serde(rename = "DeviceID")]
    device_id: String,
    size: u64,
    free_space: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WindowsGpu {
    name: String,
    #[serde(rename = "AdapterRAM")]
    adapter_ram: Option<u64>,
    driver_version: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WindowsUsage {
    cpu_load: Option<f32>,
    total_visible_memory_size: u64,
    free_physical_memory: u64,
}

#[async_trait]
impl HardwareProbe for WindowsHardwareProbe {
    async fn hardware_facts(&self) -> Result<HardwareFacts, ProbeError> {
        let facts: WindowsFacts = powershell_json(FACTS_SCRIPT, "Windows CIM").await?;
        let memory = facts.memory.first();
        let total_ram_bytes = facts.os.total_visible_memory_size.saturating_mul(1024);
        let available_ram_bytes = facts.os.free_physical_memory.saturating_mul(1024);
        let mut accelerators = facts
            .gpus
            .into_iter()
            .map(|gpu| AcceleratorFacts {
                kind: accelerator_kind(&gpu.name),
                name: gpu.name,
                total_vram_bytes: gpu.adapter_ram,
                driver_version: gpu.driver_version,
            })
            .collect::<Vec<_>>();
        let nvidia = nvidia_facts().await;
        if !nvidia.is_empty() {
            accelerators.retain(|gpu| gpu.kind != AcceleratorKind::Nvidia);
            accelerators.extend(nvidia);
        }
        Ok(HardwareFacts {
            os: OsFacts {
                family: "windows".to_owned(),
                distribution: facts.os.caption,
                version: facts.os.version,
                architecture: std::env::consts::ARCH.to_owned(),
            },
            cpu: CpuFacts {
                model: facts.cpu.name.map(|name| name.trim().to_owned()),
                architecture: std::env::consts::ARCH.to_owned(),
                logical_cores: facts
                    .cpu
                    .number_of_logical_processors
                    .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from)),
                physical_cores: facts.cpu.number_of_cores,
                frequency_mhz: facts.cpu.max_clock_speed,
            },
            memory: MemoryFacts {
                kind: memory.and_then(|memory| memory_kind(memory.smbios_memory_type)),
                speed_mts: memory.and_then(|memory| memory.speed),
            },
            total_ram_bytes,
            available_ram_bytes,
            storage: StorageFacts {
                mount_point: PathBuf::from(format!("{}\\", facts.disk.device_id)),
                total_bytes: facts.disk.size,
                available_bytes: facts.disk.free_space,
            },
            accelerators,
        })
    }

    async fn usage_snapshot(&self) -> Result<UsageSnapshot, ProbeError> {
        let usage: WindowsUsage = powershell_json(USAGE_SCRIPT, "Windows CIM").await?;
        let available_ram_bytes = usage.free_physical_memory.saturating_mul(1024);
        let accelerators = nvidia_usage().await;
        Ok(UsageSnapshot {
            sampled_at_unix_ms: super::unix_time_ms(),
            cpu_utilization_percent: usage.cpu_load.unwrap_or_default().clamp(0.0, 100.0),
            used_ram_bytes: usage
                .total_visible_memory_size
                .saturating_sub(usage.free_physical_memory)
                .saturating_mul(1024),
            available_ram_bytes,
            accelerators,
        })
    }
}

async fn powershell_json<T: for<'de> Deserialize<'de>>(
    script: &'static str,
    interface: &'static str,
) -> Result<T, ProbeError> {
    let mut command = Command::new("powershell.exe");
    hide_console_window(&mut command);
    let output = command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|source| ProbeError::Interface { interface, source })?;
    if !output.status.success() {
        return Err(ProbeError::InvalidData {
            interface,
            detail: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    serde_json::from_slice(&output.stdout).map_err(|error| ProbeError::InvalidData {
        interface,
        detail: error.to_string(),
    })
}

async fn nvidia_facts() -> Vec<AcceleratorFacts> {
    let mut command = Command::new("nvidia-smi");
    hide_console_window(&mut command);
    let Ok(output) = command
        .args([
            "--query-gpu=name,memory.total,driver_version",
            "--format=csv,noheader,nounits",
        ])
        .stdin(Stdio::null())
        .output()
        .await
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(',').map(str::trim);
            Some(AcceleratorFacts {
                kind: AcceleratorKind::Nvidia,
                name: fields.next()?.to_owned(),
                total_vram_bytes: fields
                    .next()?
                    .parse::<u64>()
                    .ok()
                    .map(|mib| mib.saturating_mul(1024 * 1024)),
                driver_version: fields.next().map(str::to_owned),
            })
        })
        .collect()
}

async fn nvidia_usage() -> Vec<AcceleratorUsage> {
    let mut command = Command::new("nvidia-smi");
    hide_console_window(&mut command);
    let Ok(output) = command
        .args([
            "--query-gpu=name,utilization.gpu,memory.used",
            "--format=csv,noheader,nounits",
        ])
        .stdin(Stdio::null())
        .output()
        .await
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(',').map(str::trim);
            Some(AcceleratorUsage {
                kind: AcceleratorKind::Nvidia,
                name: fields.next()?.to_owned(),
                utilization_percent: fields.next()?.parse().ok(),
                used_vram_bytes: fields
                    .next()?
                    .parse::<u64>()
                    .ok()
                    .map(|mib| mib.saturating_mul(1024 * 1024)),
            })
        })
        .collect()
}

fn hide_console_window(command: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

fn accelerator_kind(name: &str) -> AcceleratorKind {
    let name = name.to_ascii_lowercase();
    if name.contains("nvidia") {
        AcceleratorKind::Nvidia
    } else if name.contains("amd") || name.contains("radeon") {
        AcceleratorKind::Amd
    } else if name.contains("intel") {
        AcceleratorKind::Intel
    } else {
        AcceleratorKind::Other
    }
}

fn memory_kind(code: Option<u16>) -> Option<String> {
    match code? {
        20 => Some("DDR".to_owned()),
        21 => Some("DDR2".to_owned()),
        24 => Some("DDR3".to_owned()),
        26 => Some("DDR4".to_owned()),
        27 => Some("LPDDR".to_owned()),
        28 => Some("LPDDR2".to_owned()),
        29 => Some("LPDDR3".to_owned()),
        30 => Some("LPDDR4".to_owned()),
        34 => Some("DDR5".to_owned()),
        35 => Some("LPDDR5".to_owned()),
        _ => None,
    }
}
