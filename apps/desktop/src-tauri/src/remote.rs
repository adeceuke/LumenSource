use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use lumen_source_hardware::{
    AcceleratorFacts, AcceleratorKind, AcceleratorUsage, CpuFacts, HardwareFacts, MemoryFacts,
    OsFacts, StorageFacts, UsageSnapshot,
};
use lumen_source_runtime::{OllamaRuntime, Runtime};
use serde::{Deserialize, Serialize};
#[cfg(any(unix, windows))]
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::net::UnixListener;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

const SSH_TIMEOUT: Duration = Duration::from_secs(15);
const REMOTE_OLLAMA_DISCOVERY_COMMAND: &str = r#"PATH="$HOME/.local/bin:$HOME/bin:$HOME/.ollama/bin:$HOME/.nix-profile/bin:/usr/local/bin:/usr/bin:/bin:/snap/bin:/nix/var/nix/profiles/default/bin${PATH:+:$PATH}"; export PATH
if command -v ollama >/dev/null 2>&1; then
    command -v ollama
elif [ -n "${SHELL:-}" ] && [ -x "$SHELL" ] && "$SHELL" -lc 'command -v ollama >/dev/null 2>&1'; then
    "$SHELL" -lc 'command -v ollama'
elif [ -n "${SHELL:-}" ] && [ -x "$SHELL" ] && "$SHELL" -ic 'command -v ollama >/dev/null 2>&1'; then
    "$SHELL" -ic 'command -v ollama'
else
    exit 127
fi"#;
const REMOTE_OLLAMA_START_COMMAND: &str = r#"PATH="$HOME/.local/bin:$HOME/bin:$HOME/.ollama/bin:$HOME/.nix-profile/bin:/usr/local/bin:/usr/bin:/bin:/snap/bin:/nix/var/nix/profiles/default/bin${PATH:+:$PATH}"; export PATH
OLLAMA_BIN=$(command -v ollama 2>/dev/null)
if [ -n "$OLLAMA_BIN" ] && [ -x "$OLLAMA_BIN" ]; then
    nohup "$OLLAMA_BIN" serve </dev/null >/dev/null 2>&1 &
elif [ -n "${SHELL:-}" ] && [ -x "$SHELL" ] && "$SHELL" -lc 'command -v ollama >/dev/null 2>&1'; then
    nohup "$SHELL" -lc 'ollama serve' </dev/null >/dev/null 2>&1 &
elif [ -n "${SHELL:-}" ] && [ -x "$SHELL" ] && "$SHELL" -ic 'command -v ollama >/dev/null 2>&1'; then
    nohup "$SHELL" -ic 'ollama serve' </dev/null >/dev/null 2>&1 &
else
    exit 127
fi
"#;
const REMOTE_HARDWARE_COMMAND: &str = r#"LC_ALL=C; export LC_ALL
lumen_section() {
    printf '\n[LUMEN_SOURCE:%s]\n' "$1"
}
lumen_section ARCHITECTURE
uname -m
lumen_section OS_RELEASE
cat /etc/os-release 2>/dev/null || true
lumen_section CPUINFO
cat /proc/cpuinfo
lumen_section CPU_FREQUENCY
cat /sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq 2>/dev/null || true
lumen_section MEMINFO
cat /proc/meminfo
lumen_section MEMORY_TYPE
for lumen_file in /sys/devices/system/edac/mc/mc*/dimm*/dimm_mem_type; do
    if [ -r "$lumen_file" ]; then cat "$lumen_file"; break; fi
done
lumen_section MEMORY_SPEED
for lumen_file in /sys/devices/system/edac/mc/mc*/dimm*/dimm_speed; do
    if [ -r "$lumen_file" ]; then cat "$lumen_file"; break; fi
done
lumen_section DMIDECODE
if command -v dmidecode >/dev/null 2>&1; then
    dmidecode --type 17 2>/dev/null || true
fi
lumen_section STORAGE
df -Pk -- "$HOME"
lumen_section NVIDIA
if command -v nvidia-smi >/dev/null 2>&1; then
    nvidia-smi --query-gpu=name,memory.total,driver_version --format=csv,noheader,nounits 2>/dev/null || true
fi
lumen_section AMD
if command -v rocminfo >/dev/null 2>&1 && rocminfo >/dev/null 2>&1; then
    for lumen_card in /sys/class/drm/card[0-9]*; do
        [ -r "$lumen_card/device/vendor" ] || continue
        lumen_vendor=$(cat "$lumen_card/device/vendor" 2>/dev/null)
        [ "$lumen_vendor" = "0x1002" ] || continue
        lumen_vram=
        if [ -r "$lumen_card/device/mem_info_vram_total" ]; then
            lumen_vram=$(cat "$lumen_card/device/mem_info_vram_total" 2>/dev/null)
        fi
        printf '%s|%s\n' "${lumen_card##*/}" "$lumen_vram"
    done
fi
"#;
const REMOTE_USAGE_COMMAND: &str = r#"LC_ALL=C; export LC_ALL
lumen_section() {
    printf '\n[LUMEN_SOURCE:%s]\n' "$1"
}
lumen_section CPU_BEFORE
head -n 1 /proc/stat
sleep 0.1
lumen_section CPU_AFTER
head -n 1 /proc/stat
lumen_section MEMINFO
cat /proc/meminfo
lumen_section NVIDIA
if command -v nvidia-smi >/dev/null 2>&1; then
    nvidia-smi --query-gpu=name,utilization.gpu,memory.used --format=csv,noheader,nounits 2>/dev/null || true
fi
lumen_section AMD
for lumen_card in /sys/class/drm/card[0-9]*; do
    [ -r "$lumen_card/device/vendor" ] || continue
    lumen_vendor=$(cat "$lumen_card/device/vendor" 2>/dev/null)
    [ "$lumen_vendor" = "0x1002" ] || continue
    lumen_busy=
    lumen_vram=
    if [ -r "$lumen_card/device/gpu_busy_percent" ]; then
        lumen_busy=$(cat "$lumen_card/device/gpu_busy_percent" 2>/dev/null)
    fi
    if [ -r "$lumen_card/device/mem_info_vram_used" ]; then
        lumen_vram=$(cat "$lumen_card/device/mem_info_vram_used" 2>/dev/null)
    fi
    printf '%s|%s|%s\n' "${lumen_card##*/}" "$lumen_busy" "$lumen_vram"
done
"#;
const REMOTE_WINDOWS_DETECTION_SCRIPT: &str =
    "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); Write-Output Windows";
const REMOTE_WINDOWS_HARDWARE_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$cpu = Get-CimInstance Win32_Processor -ErrorAction SilentlyContinue | Select-Object -First 1
$os = Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue
$registryCpu = Get-ItemProperty 'HKLM:\HARDWARE\DESCRIPTION\System\CentralProcessor\0'
$windows = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
Add-Type -AssemblyName Microsoft.VisualBasic
$computer = [Microsoft.VisualBasic.Devices.ComputerInfo]::new()
$memory = Get-CimInstance Win32_PhysicalMemory -ErrorAction SilentlyContinue | Select-Object -First 1
$drive = [IO.DriveInfo]::new($env:SystemDrive)
$architecture = switch ($env:PROCESSOR_ARCHITECTURE) {
  'AMD64' { 'x86_64' }
  'ARM64' { 'aarch64' }
  default { $env:PROCESSOR_ARCHITECTURE.ToLowerInvariant() }
}
$gpus = @(Get-CimInstance Win32_VideoController -ErrorAction SilentlyContinue | ForEach-Object {
  $name = [string]$_.Name
  $kind = if ($name -match 'NVIDIA') { 'nvidia' } elseif ($name -match 'AMD|Radeon') { 'amd' } elseif ($name -match 'Intel') { 'intel' } else { 'other' }
  [pscustomobject]@{
    kind = $kind
    name = $name
    total_vram_bytes = $_.AdapterRAM
    driver_version = $_.DriverVersion
  }
})
$nvidiaSmi = Get-Command nvidia-smi.exe -ErrorAction SilentlyContinue
if ($nvidiaSmi) {
  $nvidiaGpus = @(& $nvidiaSmi.Source --query-gpu=name,memory.total,driver_version --format=csv,noheader,nounits | ForEach-Object {
    $fields = $_ -split ','
    [pscustomobject]@{
      kind = 'nvidia'
      name = $fields[0].Trim()
      total_vram_bytes = [uint64]$fields[1].Trim() * 1MB
      driver_version = $fields[2].Trim()
    }
  })
  $gpus = @($gpus | Where-Object kind -ne 'nvidia') + $nvidiaGpus
}
$memoryKind = switch ([int]$memory.SMBIOSMemoryType) {
  20 { 'DDR' }; 21 { 'DDR2' }; 24 { 'DDR3' }; 26 { 'DDR4' }
  27 { 'LPDDR' }; 28 { 'LPDDR2' }; 29 { 'LPDDR3' }; 30 { 'LPDDR4' }
  34 { 'DDR5' }; 35 { 'LPDDR5' }; default { $null }
}
[pscustomobject]@{
  os = [pscustomobject]@{
    family = 'windows'
    distribution = if ($os.Caption) { $os.Caption } else { $windows.ProductName }
    version = if ($os.Version) { $os.Version } else { "$($windows.DisplayVersion) ($($windows.CurrentBuild))" }
    architecture = $architecture
  }
  cpu = [pscustomobject]@{
    model = if ($cpu.Name) { $cpu.Name } else { $registryCpu.ProcessorNameString }
    architecture = $architecture
    logical_cores = if ($cpu.NumberOfLogicalProcessors) { $cpu.NumberOfLogicalProcessors } else { [Environment]::ProcessorCount }
    physical_cores = $cpu.NumberOfCores
    frequency_mhz = if ($cpu.MaxClockSpeed) { $cpu.MaxClockSpeed } else { $registryCpu.'~MHz' }
  }
  memory = [pscustomobject]@{
    kind = $memoryKind
    speed_mts = if ($memory.ConfiguredClockSpeed) { $memory.ConfiguredClockSpeed } else { $memory.Speed }
  }
  total_ram_bytes = if ($os.TotalVisibleMemorySize) { [uint64]$os.TotalVisibleMemorySize * 1KB } else { $computer.TotalPhysicalMemory }
  available_ram_bytes = if ($os.FreePhysicalMemory) { [uint64]$os.FreePhysicalMemory * 1KB } else { $computer.AvailablePhysicalMemory }
  storage = [pscustomobject]@{
    mount_point = "$($env:SystemDrive)\"
    total_bytes = $drive.TotalSize
    available_bytes = $drive.AvailableFreeSpace
  }
  accelerators = $gpus
} | ConvertTo-Json -Compress -Depth 5
"#;
const REMOTE_WINDOWS_USAGE_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$cpu = Get-CimInstance Win32_Processor -ErrorAction SilentlyContinue
$os = Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue
Add-Type -AssemblyName Microsoft.VisualBasic
$computer = [Microsoft.VisualBasic.Devices.ComputerInfo]::new()
$load = ($cpu | Measure-Object -Property LoadPercentage -Average).Average
$accelerators = @()
$nvidiaSmi = Get-Command nvidia-smi.exe -ErrorAction SilentlyContinue
if ($nvidiaSmi) {
  $accelerators = @(& $nvidiaSmi.Source --query-gpu=name,utilization.gpu,memory.used --format=csv,noheader,nounits | ForEach-Object {
    $fields = $_ -split ','
    [pscustomobject]@{
      kind = 'nvidia'
      name = $fields[0].Trim()
      utilization_percent = [float]$fields[1].Trim()
      used_vram_bytes = [uint64]$fields[2].Trim() * 1MB
    }
  })
}
[pscustomobject]@{
  sampled_at_unix_ms = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
  cpu_utilization_percent = if ($null -ne $load) { $load } else { 0 }
  used_ram_bytes = if ($os.TotalVisibleMemorySize) { [uint64]($os.TotalVisibleMemorySize - $os.FreePhysicalMemory) * 1KB } else { $computer.TotalPhysicalMemory - $computer.AvailablePhysicalMemory }
  available_ram_bytes = if ($os.FreePhysicalMemory) { [uint64]$os.FreePhysicalMemory * 1KB } else { $computer.AvailablePhysicalMemory }
  accelerators = $accelerators
} | ConvertTo-Json -Compress
"#;
const REMOTE_WINDOWS_OLLAMA_DISCOVERY_SCRIPT: &str = r#"
$ollama = Get-Command ollama.exe -ErrorAction SilentlyContinue
if (-not $ollama) {
  $candidate = Join-Path $env:LOCALAPPDATA 'Programs\Ollama\ollama.exe'
  if (Test-Path -LiteralPath $candidate -PathType Leaf) { $ollama = Get-Item $candidate }
}
if (-not $ollama) { exit 127 }
if ($ollama.Source) { $ollama.Source } else { $ollama.FullName }
"#;
const REMOTE_WINDOWS_OLLAMA_START_SCRIPT: &str = r#"
$ollama = Get-Command ollama.exe -ErrorAction SilentlyContinue
if (-not $ollama) {
  $candidate = Join-Path $env:LOCALAPPDATA 'Programs\Ollama\ollama.exe'
  if (Test-Path -LiteralPath $candidate -PathType Leaf) { $ollama = Get-Item $candidate }
}
if (-not $ollama) { exit 127 }
$path = if ($ollama.Source) { $ollama.Source } else { $ollama.FullName }
Start-Process -FilePath $path -ArgumentList 'serve' -WindowStyle Hidden
"#;
#[cfg(unix)]
const ASKPASS_SOCKET_ENV: &str = "LUMEN_SOURCE_ASKPASS_SOCKET";
#[cfg(windows)]
const ASKPASS_PIPE_ENV: &str = "LUMEN_SOURCE_ASKPASS_PIPE";
const ASKPASS_MARKER_ENV: &str = "LUMEN_SOURCE_ASKPASS_HELPER";
const ASKPASS_MAX_REQUESTS: usize = 4;
#[cfg(unix)]
static ASKPASS_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RemoteAuthentication {
    #[default]
    Key,
    Password,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTargetConfig {
    pub name: String,
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub authentication: RemoteAuthentication,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
}

impl RemoteTargetConfig {
    pub fn normalized(mut self) -> Self {
        self.name = self.name.trim().to_owned();
        self.host = self.host.trim().to_owned();
        self.username = self.username.trim().to_owned();
        self.identity_file = self
            .identity_file
            .take()
            .map(|path| path.trim().to_owned())
            .filter(|path| !path.is_empty());
        self
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_optional_label("Target name", &self.name, 80)?;
        validate_component("Host", &self.host, 255, "-.:_[]")?;
        validate_component("Username", &self.username, 64, "-._")?;
        if self.port == 0 {
            return Err("SSH port must be between 1 and 65535".to_owned());
        }
        if self.authentication == RemoteAuthentication::Password && self.identity_file.is_some() {
            return Err(
                "Choose either password authentication or an SSH identity file, not both"
                    .to_owned(),
            );
        }
        if let Some(path) = self.identity_file.as_deref().map(str::trim) {
            if path.is_empty() {
                return Err("Identity file cannot be blank when provided".to_owned());
            }
            if path.contains(['\n', '\r', '\0']) {
                return Err("Identity file contains unsupported characters".to_owned());
            }
            if !Path::new(path).is_file() {
                return Err(format!("SSH identity file does not exist: {path}"));
            }
        }
        Ok(())
    }

    pub fn target_id(&self) -> String {
        format!("ssh:{}@{}:{}", self.username, self.host, self.port)
    }

    pub fn display_name(&self) -> String {
        let name = self.name.trim();
        if name.is_empty() {
            self.host.trim().to_owned()
        } else {
            name.to_owned()
        }
    }

    fn destination(&self) -> String {
        format!("{}@{}", self.username, self.host)
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTargetProfile {
    pub target_id: String,
    pub target_name: String,
    pub config: RemoteTargetConfig,
}

impl From<RemoteTargetConfig> for RemoteTargetProfile {
    fn from(config: RemoteTargetConfig) -> Self {
        Self {
            target_id: config.target_id(),
            target_name: config.display_name(),
            config,
        }
    }
}

fn default_ssh_port() -> u16 {
    22
}

struct AskpassBroker {
    #[cfg(unix)]
    socket_path: PathBuf,
    #[cfg(windows)]
    pipe_name: String,
    #[cfg(unix)]
    task: tokio::task::JoinHandle<()>,
    #[cfg(windows)]
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl AskpassBroker {
    #[cfg(unix)]
    async fn start(password: Arc<Zeroizing<String>>) -> Result<Self, String> {
        use std::os::unix::fs::PermissionsExt;

        let root = dirs::runtime_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("lumen-source")
            .join("askpass");
        std::fs::create_dir_all(&root)
            .map_err(|error| format!("Could not create the SSH password channel: {error}"))?;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("Could not secure the SSH password channel: {error}"))?;
        let sequence = ASKPASS_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let socket_path = root.join(format!("{}-{sequence}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path)
            .map_err(|error| format!("Could not open the SSH password channel: {error}"))?;
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Could not secure the SSH password socket: {error}"))?;
        let task = tokio::spawn(async move {
            for _ in 0..ASKPASS_MAX_REQUESTS {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                if stream.write_all(password.as_bytes()).await.is_err()
                    || stream.write_all(b"\n").await.is_err()
                {
                    break;
                }
                let _ = stream.shutdown().await;
            }
        });
        Ok(Self { socket_path, task })
    }

    #[cfg(windows)]
    async fn start(password: Arc<Zeroizing<String>>) -> Result<Self, String> {
        use tokio::net::windows::named_pipe::ServerOptions;

        let pipe_name = format!(r"\\.\pipe\lumen-source-askpass-{}", uuid::Uuid::new_v4());
        let mut options = ServerOptions::new();
        options
            .access_inbound(false)
            .access_outbound(true)
            .max_instances(ASKPASS_MAX_REQUESTS)
            .reject_remote_clients(true);
        let mut servers = Vec::with_capacity(ASKPASS_MAX_REQUESTS);
        for index in 0..ASKPASS_MAX_REQUESTS {
            options.first_pipe_instance(index == 0);
            servers.push(
                options
                    .create(&pipe_name)
                    .map_err(|error| format!("Could not open the SSH password pipe: {error}"))?,
            );
        }
        let tasks = servers
            .into_iter()
            .map(|mut server| {
                let password = Arc::clone(&password);
                tokio::spawn(async move {
                    if server.connect().await.is_ok() {
                        let _ = server.write_all(password.as_bytes()).await;
                        let _ = server.write_all(b"\n").await;
                        let _ = server.shutdown().await;
                    }
                })
            })
            .collect();
        Ok(Self { pipe_name, tasks })
    }

    #[cfg(unix)]
    fn configure(&self, command: &mut Command) -> Result<(), String> {
        let executable = std::env::current_exe()
            .map_err(|error| format!("Could not locate the SSH password helper: {error}"))?;
        command
            .env("SSH_ASKPASS", executable)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env("DISPLAY", "lumen-source-askpass")
            .env(ASKPASS_MARKER_ENV, "1")
            .env(ASKPASS_SOCKET_ENV, &self.socket_path);
        Ok(())
    }

    #[cfg(windows)]
    fn configure(&self, command: &mut Command) -> Result<(), String> {
        let executable = std::env::current_exe()
            .map_err(|error| format!("Could not locate the SSH password helper: {error}"))?;
        command
            .env("SSH_ASKPASS", executable)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env("DISPLAY", "lumen-source-askpass")
            .env(ASKPASS_MARKER_ENV, "1")
            .env(ASKPASS_PIPE_ENV, &self.pipe_name);
        Ok(())
    }
}

impl Drop for AskpassBroker {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            self.task.abort();
            let _ = std::fs::remove_file(&self.socket_path);
        }
        #[cfg(windows)]
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// Runs before Tauri initialization when OpenSSH invokes this executable as its
/// one-time askpass helper. The password travels over a mode-0600 Unix socket,
/// never through arguments or environment values.
pub fn run_askpass_helper_if_requested() -> Option<i32> {
    #[cfg(unix)]
    {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;

        if std::env::var_os(ASKPASS_MARKER_ENV).as_deref() != Some(std::ffi::OsStr::new("1")) {
            return None;
        }
        let result = (|| -> Result<(), ()> {
            let path = std::env::var_os(ASKPASS_SOCKET_ENV).ok_or(())?;
            let mut stream = UnixStream::connect(path).map_err(|_| ())?;
            let mut password = Zeroizing::new(Vec::new());
            stream.read_to_end(&mut password).map_err(|_| ())?;
            std::io::stdout().write_all(&password).map_err(|_| ())?;
            std::io::stdout().flush().map_err(|_| ())
        })();
        Some(if result.is_ok() { 0 } else { 1 })
    }
    #[cfg(windows)]
    {
        use std::io::{Read, Write};

        if std::env::var_os(ASKPASS_MARKER_ENV).as_deref() != Some(std::ffi::OsStr::new("1")) {
            return None;
        }
        let result = (|| -> Result<(), ()> {
            let path = std::env::var_os(ASKPASS_PIPE_ENV).ok_or(())?;
            let mut pipe = std::fs::File::open(path).map_err(|_| ())?;
            let mut password = Zeroizing::new(Vec::new());
            pipe.read_to_end(&mut password).map_err(|_| ())?;
            std::io::stdout().write_all(&password).map_err(|_| ())?;
            std::io::stdout().flush().map_err(|_| ())
        })();
        Some(if result.is_ok() { 0 } else { 1 })
    }
    #[cfg(not(any(unix, windows)))]
    None
}

fn validate_label(label: &str, value: &str, max: usize) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{label} is required"));
    }
    if value.len() > max || value.contains(['\n', '\r', '\0']) {
        return Err(format!("{label} is not valid"));
    }
    Ok(())
}

fn validate_optional_label(label: &str, value: &str, max: usize) -> Result<(), String> {
    let value = value.trim();
    if value.len() > max || value.contains(['\n', '\r', '\0']) {
        return Err(format!("{label} is not valid"));
    }
    Ok(())
}

fn validate_component(label: &str, value: &str, max: usize, extra: &str) -> Result<(), String> {
    validate_label(label, value, max)?;
    if value.starts_with('-')
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || extra.contains(character))
    {
        return Err(format!("{label} contains unsupported characters"));
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCheck {
    pub id: String,
    pub status: String,
    pub message_key: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConnectionReport {
    pub target_id: String,
    pub target_name: String,
    pub can_continue: bool,
    pub checks: Vec<RemoteCheck>,
}

pub struct RemoteSession {
    pub config: RemoteTargetConfig,
    pub runtime: Arc<OllamaRuntime>,
    pub hardware: HardwareFacts,
    _tunnel: Mutex<Child>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RemoteOperatingSystem {
    Linux,
    Windows,
}

impl RemoteSession {
    pub fn target_id(&self) -> String {
        self.config.target_id()
    }

    pub async fn healthy(&self) -> bool {
        self.runtime.health().await.is_ok()
    }
}

pub struct RemoteConnectionAttempt {
    pub report: RemoteConnectionReport,
    pub session: Option<Arc<RemoteSession>>,
}

pub async fn probe_hardware(
    config: &RemoteTargetConfig,
    password: Option<Zeroizing<String>>,
) -> Result<HardwareFacts, String> {
    config.validate()?;
    let password = password.filter(|value| !value.is_empty()).map(Arc::new);
    match detect_remote_os(config, password.as_ref()).await? {
        RemoteOperatingSystem::Linux => {
            let output = ssh_probe(config, REMOTE_HARDWARE_COMMAND, password.as_ref()).await?;
            parse_remote_hardware(&output)
        }
        RemoteOperatingSystem::Windows => {
            let command = windows_remote_command(REMOTE_WINDOWS_HARDWARE_SCRIPT);
            let output = ssh_probe(config, &command, password.as_ref()).await?;
            serde_json::from_str(&output)
                .map_err(|error| format!("Could not interpret Windows hardware data: {error}"))
        }
    }
}

pub async fn probe_usage(
    config: &RemoteTargetConfig,
    password: Option<Zeroizing<String>>,
) -> Result<UsageSnapshot, String> {
    config.validate()?;
    let password = password.filter(|value| !value.is_empty()).map(Arc::new);
    match detect_remote_os(config, password.as_ref()).await? {
        RemoteOperatingSystem::Linux => {
            let output = ssh_probe(config, REMOTE_USAGE_COMMAND, password.as_ref()).await?;
            parse_remote_usage(&output)
        }
        RemoteOperatingSystem::Windows => {
            let command = windows_remote_command(REMOTE_WINDOWS_USAGE_SCRIPT);
            let output = ssh_probe(config, &command, password.as_ref()).await?;
            serde_json::from_str(&output)
                .map_err(|error| format!("Could not interpret Windows usage data: {error}"))
        }
    }
}

pub async fn connect(
    config: RemoteTargetConfig,
    password: Option<Zeroizing<String>>,
) -> Result<RemoteConnectionAttempt, String> {
    config.validate()?;
    let password = match config.authentication {
        RemoteAuthentication::Key => None,
        RemoteAuthentication::Password => {
            let password = password
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "Enter the SSH password for this connection".to_owned())?;
            Some(Arc::new(password))
        }
    };
    let target_id = config.target_id();
    let target_name = config.display_name();
    let mut checks = Vec::new();

    let remote_os = match detect_remote_os(&config, password.as_ref()).await {
        Ok(remote_os) => {
            let name = match remote_os {
                RemoteOperatingSystem::Linux => "Linux",
                RemoteOperatingSystem::Windows => "Windows",
            };
            checks.push(check(
                "connection",
                "pass",
                "connection.connected",
                &format!("Connected securely and confirmed a {name} target."),
                None,
            ));
            remote_os
        }
        Err(detail) => {
            let guidance = ssh_connection_guidance(&detail, &config);
            checks.push(check(
                "connection",
                "fail",
                "connection.failed",
                &detail,
                Some(&guidance),
            ));
            return Ok(attempt(target_id, target_name, checks, None));
        }
    };

    let hardware_command = match remote_os {
        RemoteOperatingSystem::Linux => REMOTE_HARDWARE_COMMAND.to_owned(),
        RemoteOperatingSystem::Windows => windows_remote_command(REMOTE_WINDOWS_HARDWARE_SCRIPT),
    };
    let hardware = match ssh_probe(&config, &hardware_command, password.as_ref()).await {
        Ok(output) => match parse_hardware_for_os(remote_os, &output) {
            Ok(hardware) => hardware,
            Err(detail) => {
                checks.push(check(
                    "hardware",
                    "fail",
                    "hardware.invalidResponse",
                    &format!("Could not interpret the target's hardware data. {detail}"),
                    Some(remote_hardware_guidance(remote_os)),
                ));
                return Ok(attempt(target_id, target_name, checks, None));
            }
        },
        Err(detail) => {
            checks.push(check(
                "hardware",
                "fail",
                "hardware.probeFailed",
                &format!("Could not inspect the target hardware. {detail}"),
                Some(remote_hardware_guidance(remote_os)),
            ));
            return Ok(attempt(target_id, target_name, checks, None));
        }
    };
    checks.push(check(
        "hardware",
        "pass",
        "hardware.detected",
        &hardware_summary(&hardware),
        None,
    ));

    let discovery_command = match remote_os {
        RemoteOperatingSystem::Linux => REMOTE_OLLAMA_DISCOVERY_COMMAND.to_owned(),
        RemoteOperatingSystem::Windows => {
            windows_remote_command(REMOTE_WINDOWS_OLLAMA_DISCOVERY_SCRIPT)
        }
    };
    if let Err(detail) = ssh_probe(&config, &discovery_command, password.as_ref()).await {
        checks.push(check(
            "ollama",
            "fail",
            "ollama.notFound",
            &format!("Ollama was not found for the remote SSH user. {detail}"),
            Some(remote_ollama_guidance(remote_os)),
        ));
        return Ok(attempt(target_id, target_name, checks, None));
    }

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| format!("Could not allocate a local tunnel port: {error}"))?;
    let local_port = listener
        .local_addr()
        .map_err(|error| format!("Could not inspect the local tunnel port: {error}"))?
        .port();
    drop(listener);

    let (mut command, askpass) = ssh_command(&config, password.as_ref()).await?;
    command
        .arg("-N")
        .arg("-T")
        .arg("-o")
        .arg("ExitOnForwardFailure=yes")
        .arg("-o")
        .arg("ServerAliveInterval=15")
        .arg("-o")
        .arg("ServerAliveCountMax=3")
        .arg("-L")
        .arg(format!("127.0.0.1:{local_port}:127.0.0.1:11434"))
        .arg(config.destination())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| ssh_launch_error(&error))?;
    let runtime = Arc::new(
        OllamaRuntime::new(&format!("http://127.0.0.1:{local_port}"))
            .map_err(|error| error.to_string())?,
    );

    let mut healthy = wait_for_remote_runtime(&mut child, runtime.as_ref(), 20).await?;
    let mut started_ollama = false;
    let tunnel_running = child
        .try_wait()
        .map_err(|error| format!("Could not inspect the SSH tunnel: {error}"))?
        .is_none();
    if !healthy && tunnel_running {
        let start_command = match remote_os {
            RemoteOperatingSystem::Linux => REMOTE_OLLAMA_START_COMMAND.to_owned(),
            RemoteOperatingSystem::Windows => {
                windows_remote_command(REMOTE_WINDOWS_OLLAMA_START_SCRIPT)
            }
        };
        match ssh_probe(&config, &start_command, password.as_ref()).await {
            Ok(_) => {
                started_ollama = true;
                healthy = wait_for_remote_runtime(&mut child, runtime.as_ref(), 100).await?;
            }
            Err(detail) => {
                drop(askpass);
                let _ = child.kill().await;
                checks.push(check(
                    "ollama",
                    "fail",
                    "ollama.startFailed",
                    &format!(
                        "Ollama is installed, but LumenSource could not start `ollama serve`. {detail}"
                    ),
                    Some(remote_ollama_start_guidance(remote_os)),
                ));
                return Ok(attempt(target_id, target_name, checks, None));
            }
        }
    }
    drop(askpass);
    if !healthy {
        let _ = child.kill().await;
        checks.push(check(
            "ollama",
            "fail",
            if started_ollama {
                "ollama.startedUnreachable"
            } else {
                "ollama.tunnelClosed"
            },
            if started_ollama {
                "LumenSource started `ollama serve`, but its loopback API did not become reachable on the target."
            } else {
                "Ollama is installed, but the SSH tunnel closed before its loopback API became reachable."
            },
            Some(remote_ollama_start_guidance(remote_os)),
        ));
        return Ok(attempt(target_id, target_name, checks, None));
    }

    checks.push(check(
        "ollama",
        "pass",
        if started_ollama {
            "ollama.started"
        } else {
            "ollama.reachable"
        },
        if started_ollama {
            "Ollama was installed but stopped. LumenSource started `ollama serve`, and its loopback API is reachable through the encrypted SSH tunnel."
        } else {
            "Ollama is installed and its loopback API is reachable through the encrypted SSH tunnel."
        },
        None,
    ));
    let session = Arc::new(RemoteSession {
        config,
        runtime,
        hardware,
        _tunnel: Mutex::new(child),
    });
    Ok(attempt(target_id, target_name, checks, Some(session)))
}

async fn wait_for_remote_runtime(
    tunnel: &mut Child,
    runtime: &OllamaRuntime,
    attempts: usize,
) -> Result<bool, String> {
    for _ in 0..attempts {
        if tunnel
            .try_wait()
            .map_err(|error| format!("Could not inspect the SSH tunnel: {error}"))?
            .is_some()
        {
            return Ok(false);
        }
        if runtime.health().await.is_ok() {
            return Ok(true);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(false)
}

fn parse_remote_hardware(output: &str) -> Result<HardwareFacts, String> {
    const KIB: u64 = 1024;
    let sections = parse_sections(output);
    let section = |name: &str| {
        sections
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| format!("The `{name}` section is missing."))
    };
    let architecture = section("ARCHITECTURE")?.trim();
    if architecture.is_empty() {
        return Err("The target architecture is missing.".to_owned());
    }
    let cpuinfo = section("CPUINFO")?;
    let logical_cores = cpuinfo
        .lines()
        .filter(|line| {
            line.split_once(':')
                .is_some_and(|(name, _)| name.trim() == "processor")
        })
        .count()
        .max(1);
    let cpu_field = |key: &str| {
        cpuinfo.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            (name.trim() == key).then(|| value.trim().to_owned())
        })
    };
    let kernel_frequency = sections
        .get("CPU_FREQUENCY")
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|khz| khz / 1_000)
        .filter(|mhz| *mhz > 0);
    let reported_frequency = cpu_field("cpu MHz")
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| value.round() as u64);

    let meminfo = section("MEMINFO")?;
    let memory_value = |key: &str| {
        meminfo.lines().find_map(|line| {
            let (name, rest) = line.split_once(':')?;
            if name != key {
                return None;
            }
            rest.split_whitespace().next()?.parse::<u64>().ok()
        })
    };
    let total_ram_bytes = memory_value("MemTotal")
        .ok_or_else(|| "The target did not report MemTotal.".to_owned())?
        .saturating_mul(KIB);
    let available_ram_bytes = memory_value("MemAvailable")
        .or_else(|| memory_value("MemFree"))
        .ok_or_else(|| "The target did not report available memory.".to_owned())?
        .saturating_mul(KIB);

    let os_release = sections.get("OS_RELEASE").map(String::as_str);
    let os_field = |key: &str| {
        os_release.and_then(|text| {
            text.lines().find_map(|line| {
                let (name, value) = line.split_once('=')?;
                (name == key).then(|| value.trim().trim_matches('"').to_owned())
            })
        })
    };

    let storage = parse_remote_storage(section("STORAGE")?)?;
    let memory_kind = sections
        .get("MEMORY_TYPE")
        .and_then(|value| normalize_memory_kind(value))
        .or_else(|| {
            sections
                .get("DMIDECODE")
                .and_then(|value| parse_dmidecode_kind(value))
        });
    let memory_speed_mts = sections
        .get("MEMORY_SPEED")
        .and_then(|value| parse_memory_speed(value))
        .or_else(|| {
            sections
                .get("DMIDECODE")
                .and_then(|value| parse_dmidecode_speed(value))
        });
    let mut accelerators = sections
        .get("NVIDIA")
        .map(|value| parse_nvidia_hardware(value))
        .unwrap_or_default();
    if let Some(amd) = sections.get("AMD") {
        accelerators.extend(parse_amd_hardware(amd));
    }

    Ok(HardwareFacts {
        os: OsFacts {
            family: "linux".to_owned(),
            distribution: os_field("ID"),
            version: os_field("VERSION_ID"),
            architecture: architecture.to_owned(),
        },
        cpu: CpuFacts {
            model: cpu_field("model name")
                .or_else(|| cpu_field("Hardware"))
                .or_else(|| cpu_field("Processor")),
            architecture: architecture.to_owned(),
            logical_cores,
            physical_cores: cpu_field("cpu cores").and_then(|value| value.parse().ok()),
            frequency_mhz: kernel_frequency.or(reported_frequency),
        },
        memory: MemoryFacts {
            kind: memory_kind,
            speed_mts: memory_speed_mts,
        },
        total_ram_bytes,
        available_ram_bytes,
        storage,
        accelerators,
    })
}

fn parse_remote_usage(output: &str) -> Result<UsageSnapshot, String> {
    const KIB: u64 = 1024;
    let sections = parse_sections(output);
    let section = |name: &str| {
        sections
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| format!("The `{name}` section is missing."))
    };
    let meminfo = section("MEMINFO")?;
    let memory_value = |key: &str| {
        meminfo.lines().find_map(|line| {
            let (name, rest) = line.split_once(':')?;
            (name == key)
                .then(|| rest.split_whitespace().next()?.parse::<u64>().ok())
                .flatten()
        })
    };
    let total_ram_bytes = memory_value("MemTotal")
        .ok_or_else(|| "The target did not report MemTotal.".to_owned())?
        .saturating_mul(KIB);
    let available_ram_bytes = memory_value("MemAvailable")
        .or_else(|| memory_value("MemFree"))
        .ok_or_else(|| "The target did not report available memory.".to_owned())?
        .saturating_mul(KIB);
    let mut accelerators = sections
        .get("NVIDIA")
        .map(|value| parse_nvidia_usage(value))
        .unwrap_or_default();
    if let Some(amd) = sections.get("AMD") {
        accelerators.extend(parse_amd_usage(amd));
    }

    Ok(UsageSnapshot {
        sampled_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX),
        cpu_utilization_percent: parse_remote_cpu_utilization(
            section("CPU_BEFORE")?,
            section("CPU_AFTER")?,
        )?,
        used_ram_bytes: total_ram_bytes.saturating_sub(available_ram_bytes),
        available_ram_bytes,
        accelerators,
    })
}

#[derive(Clone, Copy)]
struct RemoteCpuTicks {
    idle: u64,
    total: u64,
}

fn parse_remote_cpu_ticks(input: &str) -> Option<RemoteCpuTicks> {
    let line = input.lines().find(|line| line.starts_with("cpu "))?;
    let ticks = line
        .split_whitespace()
        .skip(1)
        .filter_map(|value| value.parse::<u64>().ok())
        .collect::<Vec<_>>();
    let total = ticks.iter().copied().fold(0_u64, u64::saturating_add);
    let idle = ticks
        .get(3)
        .copied()
        .unwrap_or_default()
        .saturating_add(ticks.get(4).copied().unwrap_or_default());
    Some(RemoteCpuTicks { idle, total })
}

fn parse_remote_cpu_utilization(first: &str, second: &str) -> Result<f32, String> {
    let before = parse_remote_cpu_ticks(first)
        .ok_or_else(|| "The target CPU sample is invalid.".to_owned())?;
    let after = parse_remote_cpu_ticks(second)
        .ok_or_else(|| "The target CPU sample is invalid.".to_owned())?;
    let total = after.total.saturating_sub(before.total);
    let idle = after.idle.saturating_sub(before.idle);
    Ok(if total == 0 {
        0.0
    } else {
        100.0 * (total.saturating_sub(idle) as f32 / total as f32)
    })
}

fn parse_nvidia_usage(input: &str) -> Vec<AcceleratorUsage> {
    input
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

fn parse_amd_usage(input: &str) -> Vec<AcceleratorUsage> {
    input
        .lines()
        .filter_map(|line| {
            let mut columns = line.split('|').map(str::trim);
            Some(AcceleratorUsage {
                kind: AcceleratorKind::Amd,
                name: columns.next()?.to_owned(),
                utilization_percent: columns.next().and_then(|value| value.parse().ok()),
                used_vram_bytes: columns.next().and_then(|value| value.parse().ok()),
            })
        })
        .collect()
}

fn parse_sections(output: &str) -> BTreeMap<String, String> {
    let mut sections: BTreeMap<String, String> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in output.lines() {
        if let Some(name) = line
            .strip_prefix("[LUMEN_SOURCE:")
            .and_then(|value| value.strip_suffix(']'))
        {
            current = Some(name.to_owned());
            sections.entry(name.to_owned()).or_default();
        } else if let Some(name) = current.as_ref() {
            let value = sections.entry(name.clone()).or_default();
            if !value.is_empty() {
                value.push('\n');
            }
            value.push_str(line);
        }
    }
    sections
}

fn parse_remote_storage(input: &str) -> Result<StorageFacts, String> {
    const KIB: u64 = 1024;
    let columns = input
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .unwrap_or_default();
    let blocks = |index: usize| {
        columns
            .get(index)
            .and_then(|value| value.parse::<u64>().ok())
            .map(|value| value.saturating_mul(KIB))
    };
    Ok(StorageFacts {
        mount_point: PathBuf::from(columns.get(5).copied().unwrap_or("/")),
        total_bytes: blocks(1)
            .ok_or_else(|| "The target storage capacity is missing.".to_owned())?,
        available_bytes: blocks(3)
            .ok_or_else(|| "The target free storage value is missing.".to_owned())?,
    })
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

fn parse_dmidecode_kind(input: &str) -> Option<String> {
    input.lines().find_map(|line| {
        line.trim()
            .strip_prefix("Type:")
            .and_then(normalize_memory_kind)
    })
}

fn parse_dmidecode_speed(input: &str) -> Option<u64> {
    let find = |label: &str| {
        input
            .lines()
            .find_map(|line| line.trim().strip_prefix(label).and_then(parse_memory_speed))
    };
    find("Configured Memory Speed:").or_else(|| find("Speed:"))
}

fn parse_nvidia_hardware(input: &str) -> Vec<AcceleratorFacts> {
    input
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
        .collect()
}

fn parse_amd_hardware(input: &str) -> Vec<AcceleratorFacts> {
    input
        .lines()
        .filter_map(|line| {
            let (name, vram) = line.split_once('|')?;
            Some(AcceleratorFacts {
                kind: AcceleratorKind::Amd,
                name: name.to_owned(),
                total_vram_bytes: vram.trim().parse().ok(),
                driver_version: None,
            })
        })
        .collect()
}

fn hardware_summary(hardware: &HardwareFacts) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let accelerator = hardware
        .accelerators
        .first()
        .map(|device| format!(", {}", device.name))
        .unwrap_or_else(|| ", CPU-only".to_owned());
    format!(
        "Detected {} logical CPU cores, {:.1} GiB RAM{accelerator}, and {:.1} GiB free storage.",
        hardware.cpu.logical_cores,
        hardware.total_ram_bytes as f64 / GIB,
        hardware.storage.available_bytes as f64 / GIB,
    )
}

async fn detect_remote_os(
    config: &RemoteTargetConfig,
    password: Option<&Arc<Zeroizing<String>>>,
) -> Result<RemoteOperatingSystem, String> {
    let linux_probe = ssh_probe(config, "uname -s", password).await;
    if linux_probe
        .as_deref()
        .is_ok_and(|output| remote_os_output_contains(output, "Linux"))
    {
        return Ok(RemoteOperatingSystem::Linux);
    }
    let command = windows_remote_command(REMOTE_WINDOWS_DETECTION_SCRIPT);
    let windows_probe = ssh_probe(config, &command, password).await;
    if windows_probe
        .as_deref()
        .is_ok_and(|output| remote_os_output_contains(output, "Windows"))
    {
        return Ok(RemoteOperatingSystem::Windows);
    }

    match (linux_probe, windows_probe) {
        (Err(linux_error), Err(windows_error)) => Err(format!(
            "Could not identify the remote operating system because the SSH probes failed. Linux probe: {linux_error} Windows probe: {windows_error}"
        )),
        (linux_result, windows_result) => {
            let linux_output = linux_result.unwrap_or_default();
            let windows_output = windows_result.unwrap_or_default();
            Err(format!(
                "The SSH connection succeeded, but the target did not identify itself as Linux or Windows (Linux probe: `{}`, Windows probe: `{}`).",
                compact_output(&linux_output),
                compact_output(&windows_output),
            ))
        }
    }
}

fn remote_os_output_contains(output: &str, expected: &str) -> bool {
    output
        .lines()
        .any(|line| line.trim().eq_ignore_ascii_case(expected))
}

fn windows_remote_command(script: &str) -> String {
    let utf16 = script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    format!(
        "powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand {}",
        STANDARD.encode(utf16)
    )
}

fn parse_hardware_for_os(os: RemoteOperatingSystem, output: &str) -> Result<HardwareFacts, String> {
    match os {
        RemoteOperatingSystem::Linux => parse_remote_hardware(output),
        RemoteOperatingSystem::Windows => serde_json::from_str(output)
            .map_err(|error| format!("Could not interpret Windows hardware data: {error}")),
    }
}

fn remote_hardware_guidance(os: RemoteOperatingSystem) -> &'static str {
    match os {
        RemoteOperatingSystem::Linux => {
            "Confirm `/proc/cpuinfo`, `/proc/meminfo`, `uname`, and `df` are available to the SSH user, then retry."
        }
        RemoteOperatingSystem::Windows => {
            "Confirm Windows PowerShell, registry access, and the system drive are available to the OpenSSH user, then retry."
        }
    }
}

fn remote_ollama_guidance(os: RemoteOperatingSystem) -> &'static str {
    match os {
        RemoteOperatingSystem::Linux => {
            "Install Ollama using the publisher's Linux instructions, then confirm `ssh <target> ollama --version` works."
        }
        RemoteOperatingSystem::Windows => {
            "Install Ollama for the Windows SSH user, then confirm `ollama.exe --version` works in that user's PowerShell session."
        }
    }
}

fn remote_ollama_start_guidance(os: RemoteOperatingSystem) -> &'static str {
    match os {
        RemoteOperatingSystem::Linux => {
            "Start Ollama on the target and confirm `curl http://127.0.0.1:11434/api/tags` succeeds. A system-managed installation can be started with `sudo systemctl enable --now ollama`."
        }
        RemoteOperatingSystem::Windows => {
            "Start Ollama for the SSH user and confirm `Invoke-WebRequest http://127.0.0.1:11434/api/tags` succeeds in PowerShell."
        }
    }
}

async fn ssh_command(
    config: &RemoteTargetConfig,
    password: Option<&Arc<Zeroizing<String>>>,
) -> Result<(Command, Option<AskpassBroker>), String> {
    let mut command = Command::new("ssh");
    hide_console_window(&mut command);
    command
        .arg("-p")
        .arg(config.port.to_string())
        .arg("-o")
        .arg("ConnectTimeout=10")
        .arg("-o")
        .arg("StrictHostKeyChecking=accept-new");
    let askpass = match config.authentication {
        RemoteAuthentication::Key => {
            command.arg("-o").arg("BatchMode=yes");
            if let Some(identity_file) = config.identity_file.as_deref() {
                command
                    .arg("-o")
                    .arg("IdentitiesOnly=yes")
                    .arg("-i")
                    .arg(identity_file.trim());
            }
            None
        }
        RemoteAuthentication::Password => {
            let password = password
                .cloned()
                .ok_or_else(|| "Enter the SSH password for this connection".to_owned())?;
            command
                .arg("-o")
                .arg("BatchMode=no")
                .arg("-o")
                .arg("PreferredAuthentications=password,keyboard-interactive")
                .arg("-o")
                .arg("PubkeyAuthentication=no")
                .arg("-o")
                .arg("PasswordAuthentication=yes")
                .arg("-o")
                .arg("KbdInteractiveAuthentication=yes")
                .arg("-o")
                .arg("NumberOfPasswordPrompts=1");
            let broker = AskpassBroker::start(password).await?;
            broker.configure(&mut command)?;
            Some(broker)
        }
    };
    Ok((command, askpass))
}

#[cfg(target_os = "windows")]
fn hide_console_window(command: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn hide_console_window(_command: &mut Command) {}

async fn ssh_probe(
    config: &RemoteTargetConfig,
    probe: &str,
    password: Option<&Arc<Zeroizing<String>>>,
) -> Result<String, String> {
    let (mut command, askpass) = ssh_command(config, password).await?;
    command
        .arg(config.destination())
        .arg(probe)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = tokio::time::timeout(SSH_TIMEOUT, command.output())
        .await
        .map_err(|_| "The SSH connection timed out.".to_owned())?
        .map_err(|error| ssh_launch_error(&error))?;
    drop(askpass);
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(if detail.is_empty() {
        match output.status.code() {
            Some(code) => format!("SSH exited with status {code}."),
            None => "The SSH process was terminated before the probe completed.".to_owned(),
        }
    } else {
        format!("SSH reported: {}", compact_output(&detail))
    })
}

fn ssh_launch_error(error: &std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        "The OpenSSH client (`ssh`) is not installed on this machine.".to_owned()
    } else {
        format!("Could not start the OpenSSH client: {error}")
    }
}

fn compact_output(value: &str) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.chars().take(500).collect()
}

fn ssh_connection_guidance(detail: &str, config: &RemoteTargetConfig) -> String {
    let normalized = detail.to_ascii_lowercase();
    let destination = config.destination();
    let batch_test = format!(
        "`ssh -o BatchMode=yes -p {} {} true`",
        config.port, destination
    );
    if normalized.contains("permission denied") {
        if config.authentication == RemoteAuthentication::Password {
            return format!(
                "SSH reached the target, but password authentication was rejected. Verify the username and password with `ssh -p {} {destination}` and retry. The password is used only for this connection and is not saved by LumenSource.",
                config.port
            );
        }
        let agent_note = if std::env::var_os("SSH_AUTH_SOCK").is_some()
            || config.identity_file.is_some()
        {
            "Ensure the correct key is loaded in the SSH agent visible to LumenSource; if you selected an encrypted key, load it into the agent first."
        } else {
            "No SSH agent or identity file is configured for LumenSource. Load your key with `ssh-add`, launch LumenSource from that environment, or select the identity file."
        };
        return format!(
            "SSH reached the target, but non-interactive authentication failed. LumenSource cannot use a terminal password or key-passphrase prompt. {agent_note} Test the same mode with {batch_test}. If normal `ssh {destination}` works only after asking for a password, configure public-key authentication first (for example with `ssh-copy-id -p {} {destination}`). If your terminal uses an SSH config alias, enter that alias as the host instead of its IP address.",
            config.port
        );
    }
    if normalized.contains("host key verification failed")
        || normalized.contains("no host key is known")
    {
        return format!(
            "The target host key conflicts with a previously trusted identity or could not be saved. Verify the target fingerprint before changing trust. Inspect the saved key with `ssh-keygen -F {}` and connect with `ssh -p {} {destination}` for full OpenSSH diagnostics.",
            config.host,
            config.port
        );
    }
    if normalized.contains("connection refused")
        || normalized.contains("connection timed out")
        || normalized.contains("no route to host")
        || normalized.contains("network is unreachable")
    {
        return "Install and start OpenSSH Server on the target (for Ubuntu: `sudo apt install openssh-server && sudo systemctl enable --now ssh`), then verify the host address, SSH port, and firewall.".to_owned();
    }
    format!(
        "Verify the address and port with `ssh -p {} {destination}`. LumenSource uses strict host-key checking and non-interactive authentication through your existing SSH agent, SSH configuration, or selected identity file. The equivalent non-interactive test is {batch_test}.",
        config.port
    )
}

fn check(
    id: &str,
    status: &str,
    message_key: &str,
    detail: &str,
    guidance: Option<&str>,
) -> RemoteCheck {
    RemoteCheck {
        id: id.to_owned(),
        status: status.to_owned(),
        message_key: message_key.to_owned(),
        detail: detail.to_owned(),
        guidance: guidance.map(str::to_owned),
    }
}

fn attempt(
    target_id: String,
    target_name: String,
    checks: Vec<RemoteCheck>,
    session: Option<Arc<RemoteSession>>,
) -> RemoteConnectionAttempt {
    RemoteConnectionAttempt {
        report: RemoteConnectionReport {
            target_id,
            target_name,
            can_continue: session.is_some(),
            checks,
        },
        session,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_config() -> RemoteTargetConfig {
        RemoteTargetConfig {
            name: "GPU host".to_owned(),
            host: "model-host.example".to_owned(),
            port: 22,
            username: "lumen_source".to_owned(),
            authentication: RemoteAuthentication::Key,
            identity_file: None,
        }
    }

    #[test]
    fn windows_remote_commands_are_encoded_for_the_target_shell() {
        let command = windows_remote_command("Write-Output Windows");
        assert!(command.starts_with("powershell.exe "));
        assert!(command.contains("-EncodedCommand "));
        assert!(!command.contains("Write-Output"));
    }

    #[tokio::test]
    async fn ssh_silently_accepts_new_hosts_but_rejects_changed_keys() -> Result<(), String> {
        let config = valid_config();
        let (command, broker) = ssh_command(&config, None).await?;
        let arguments = command
            .as_std()
            .get_args()
            .map(|argument| argument.to_string_lossy())
            .collect::<Vec<_>>();
        assert!(arguments
            .iter()
            .any(|argument| argument == "StrictHostKeyChecking=accept-new"));
        assert!(broker.is_none());
        Ok(())
    }

    #[test]
    fn recognizes_remote_os_with_login_banner_output() {
        assert!(remote_os_output_contains(
            "Welcome to Ubuntu 24.04 LTS\nLinux\n",
            "Linux"
        ));
        assert!(remote_os_output_contains(
            "notice\r\nWindows\r\n",
            "Windows"
        ));
        assert!(!remote_os_output_contains("GNU/Linux", "Linux"));
    }

    #[test]
    fn parses_normalized_windows_remote_hardware() -> Result<(), String> {
        let facts = parse_hardware_for_os(
            RemoteOperatingSystem::Windows,
            r#"{
              "os":{"family":"windows","distribution":"Windows 11","version":"24H2","architecture":"x86_64"},
              "cpu":{"model":"Test CPU","architecture":"x86_64","logical_cores":8,"physical_cores":4,"frequency_mhz":3200},
              "memory":{"kind":"DDR5","speed_mts":5600},
              "total_ram_bytes":17179869184,
              "available_ram_bytes":8589934592,
              "storage":{"mount_point":"C:\\","total_bytes":1000,"available_bytes":500},
              "accelerators":[]
            }"#,
        )?;
        assert_eq!(facts.os.family, "windows");
        assert_eq!(facts.cpu.logical_cores, 8);
        assert_eq!(facts.storage.mount_point, PathBuf::from("C:\\"));
        Ok(())
    }

    #[test]
    fn validates_and_identifies_a_remote_target() {
        let config = valid_config();
        assert_eq!(config.validate(), Ok(()));
        assert_eq!(config.target_id(), "ssh:lumen_source@model-host.example:22");
    }

    #[test]
    fn target_name_is_optional_and_falls_back_to_the_host() {
        let mut config = valid_config();
        config.name = "  ".to_owned();
        assert_eq!(config.validate(), Ok(()));
        assert_eq!(config.display_name(), "model-host.example");
    }

    #[test]
    fn normalizes_saved_profile_fields_and_uses_the_host_as_its_label() {
        let mut config = valid_config();
        config.name = "  ".to_owned();
        config.host = "  10.0.0.8 ".to_owned();
        config.username = " lumen_source ".to_owned();
        config.identity_file = Some("  ".to_owned());
        let profile = RemoteTargetProfile::from(config.normalized());
        assert_eq!(profile.target_name, "10.0.0.8");
        assert_eq!(profile.target_id, "ssh:lumen_source@10.0.0.8:22");
        assert_eq!(profile.config.identity_file, None);
    }

    #[test]
    fn rejects_values_that_could_become_ssh_options() {
        let mut config = valid_config();
        config.host = "-oProxyCommand=bad".to_owned();
        assert!(config.validate().is_err());
        config = valid_config();
        config.username = "name with spaces".to_owned();
        assert!(config.validate().is_err());
    }

    #[test]
    fn reports_a_missing_identity_file_before_launching_ssh() {
        let mut config = valid_config();
        config.identity_file = Some("/definitely/missing/lumen-source-key".to_owned());
        assert!(config.validate().is_err());
    }

    #[test]
    fn permission_denied_guidance_explains_non_interactive_authentication() {
        let config = valid_config();
        let guidance = ssh_connection_guidance(
            "SSH reported: Permission denied (publickey,password).",
            &config,
        );
        assert!(guidance.contains("cannot use a terminal password"));
        assert!(guidance.contains("BatchMode=yes"));
        assert!(guidance.contains("ssh-copy-id"));
    }

    #[test]
    fn password_failure_guidance_does_not_suggest_batch_mode() {
        let mut config = valid_config();
        config.authentication = RemoteAuthentication::Password;
        let guidance = ssh_connection_guidance(
            "SSH reported: Permission denied (publickey,password).",
            &config,
        );
        assert!(guidance.contains("password authentication was rejected"));
        assert!(guidance.contains("is not saved"));
        assert!(!guidance.contains("BatchMode=yes"));
    }

    #[test]
    fn remote_ollama_commands_include_non_interactive_login_and_interactive_paths() {
        for command in [REMOTE_OLLAMA_DISCOVERY_COMMAND, REMOTE_OLLAMA_START_COMMAND] {
            assert!(command.contains("$HOME/.local/bin"));
            assert!(command.contains("/usr/local/bin"));
            assert!(command.contains("/snap/bin"));
            assert!(command.contains("$HOME/.nix-profile/bin"));
        }
        assert!(REMOTE_OLLAMA_DISCOVERY_COMMAND.contains("\"$SHELL\" -lc"));
        assert!(REMOTE_OLLAMA_DISCOVERY_COMMAND.contains("\"$SHELL\" -ic"));
        assert!(REMOTE_OLLAMA_START_COMMAND.contains("\"$SHELL\" -lc"));
        assert!(REMOTE_OLLAMA_START_COMMAND.contains("\"$SHELL\" -ic"));
    }

    #[test]
    fn remote_ollama_start_is_detached_from_ssh_standard_streams() {
        assert!(REMOTE_OLLAMA_START_COMMAND.contains("nohup"));
        assert!(REMOTE_OLLAMA_START_COMMAND.contains("serve"));
        assert!(REMOTE_OLLAMA_START_COMMAND.contains("</dev/null"));
        assert!(REMOTE_OLLAMA_START_COMMAND.contains(">/dev/null 2>&1 &"));
    }

    #[test]
    fn parses_normalized_remote_linux_hardware() -> Result<(), String> {
        let facts = parse_remote_hardware(
            r#"
[LUMEN_SOURCE:ARCHITECTURE]
x86_64
[LUMEN_SOURCE:OS_RELEASE]
ID=ubuntu
VERSION_ID="24.04"
[LUMEN_SOURCE:CPUINFO]
processor : 0
model name : Test CPU
cpu cores : 2
cpu MHz : 2400.000
processor : 1
[LUMEN_SOURCE:CPU_FREQUENCY]
3600000
[LUMEN_SOURCE:MEMINFO]
MemTotal:       32768000 kB
MemAvailable:   24576000 kB
[LUMEN_SOURCE:MEMORY_TYPE]
DDR5
[LUMEN_SOURCE:MEMORY_SPEED]
5600
[LUMEN_SOURCE:DMIDECODE]
[LUMEN_SOURCE:STORAGE]
Filesystem 1024-blocks Used Available Capacity Mounted on
/dev/test 104857600 10485760 94371840 10% /home
[LUMEN_SOURCE:NVIDIA]
NVIDIA Test GPU, 12288, 555.42
[LUMEN_SOURCE:AMD]
"#,
        )?;

        assert_eq!(facts.os.family, "linux");
        assert_eq!(facts.os.distribution.as_deref(), Some("ubuntu"));
        assert_eq!(facts.os.version.as_deref(), Some("24.04"));
        assert_eq!(facts.os.architecture, "x86_64");
        assert_eq!(facts.cpu.model.as_deref(), Some("Test CPU"));
        assert_eq!(facts.cpu.logical_cores, 2);
        assert_eq!(facts.cpu.physical_cores, Some(2));
        assert_eq!(facts.cpu.frequency_mhz, Some(3600));
        assert_eq!(facts.total_ram_bytes, 32_768_000 * 1024);
        assert_eq!(facts.available_ram_bytes, 24_576_000 * 1024);
        assert_eq!(facts.memory.kind.as_deref(), Some("DDR5"));
        assert_eq!(facts.memory.speed_mts, Some(5600));
        assert_eq!(facts.storage.mount_point, PathBuf::from("/home"));
        assert_eq!(facts.storage.available_bytes, 94_371_840 * 1024);
        assert_eq!(facts.accelerators.len(), 1);
        assert_eq!(facts.accelerators[0].kind, AcceleratorKind::Nvidia);
        assert_eq!(
            facts.accelerators[0].total_vram_bytes,
            Some(12_288 * 1024 * 1024)
        );
        Ok(())
    }

    #[test]
    fn remote_hardware_requires_capacity_sections() {
        let error = parse_remote_hardware(
            "[LUMEN_SOURCE:ARCHITECTURE]\nx86_64\n[LUMEN_SOURCE:CPUINFO]\nprocessor: 0",
        );
        assert!(error.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn remote_hardware_probe_is_valid_posix_shell_syntax() -> Result<(), String> {
        let status = std::process::Command::new("sh")
            .args(["-n", "-c", REMOTE_HARDWARE_COMMAND])
            .status()
            .map_err(|error| error.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("The remote hardware probe exited with {status}"))
        }
    }

    #[test]
    fn parses_remote_hardware_usage() -> Result<(), String> {
        let usage = parse_remote_usage(
            r#"
[LUMEN_SOURCE:CPU_BEFORE]
cpu  100 0 50 850 0 0 0 0
[LUMEN_SOURCE:CPU_AFTER]
cpu  140 0 60 900 0 0 0 0
[LUMEN_SOURCE:MEMINFO]
MemTotal:       16000000 kB
MemAvailable:   6000000 kB
[LUMEN_SOURCE:NVIDIA]
NVIDIA Test GPU, 42, 2048
[LUMEN_SOURCE:AMD]
"#,
        )?;
        assert!((usage.cpu_utilization_percent - 50.0).abs() < f32::EPSILON);
        assert_eq!(usage.used_ram_bytes, 10_000_000 * 1024);
        assert_eq!(usage.available_ram_bytes, 6_000_000 * 1024);
        assert_eq!(usage.accelerators.len(), 1);
        assert_eq!(usage.accelerators[0].utilization_percent, Some(42.0));
        assert_eq!(
            usage.accelerators[0].used_vram_bytes,
            Some(2_048 * 1024 * 1024)
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn remote_usage_probe_is_valid_posix_shell_syntax() -> Result<(), String> {
        let status = std::process::Command::new("sh")
            .args(["-n", "-c", REMOTE_USAGE_COMMAND])
            .status()
            .map_err(|error| error.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("The remote usage probe exited with {status}"))
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn askpass_broker_uses_a_private_one_time_socket() -> Result<(), String> {
        use std::os::unix::fs::PermissionsExt;
        use tokio::io::AsyncReadExt;

        let password = Arc::new(Zeroizing::new("test-only-password".to_owned()));
        let broker = AskpassBroker::start(password).await?;
        let mode = std::fs::metadata(&broker.socket_path)
            .map_err(|error| error.to_string())?
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let path = broker.socket_path.clone();
        let mut stream = tokio::net::UnixStream::connect(&path)
            .await
            .map_err(|error| error.to_string())?;
        let mut received = Zeroizing::new(Vec::new());
        stream
            .read_to_end(&mut received)
            .await
            .map_err(|error| error.to_string())?;
        assert_eq!(received.as_slice(), b"test-only-password\n");
        drop(broker);
        assert!(!path.exists());
        Ok(())
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn askpass_broker_uses_a_private_local_named_pipe() -> Result<(), String> {
        use tokio::io::AsyncReadExt;
        use tokio::net::windows::named_pipe::ClientOptions;

        let password = Arc::new(Zeroizing::new("test-only-password".to_owned()));
        let broker = AskpassBroker::start(password).await?;
        assert!(broker
            .pipe_name
            .starts_with(r"\\.\pipe\lumen-source-askpass-"));
        for _ in 0..ASKPASS_MAX_REQUESTS {
            let mut options = ClientOptions::new();
            options.write(false);
            let mut pipe = options
                .open(&broker.pipe_name)
                .map_err(|error| error.to_string())?;
            let mut received = Zeroizing::new(Vec::new());
            pipe.read_to_end(&mut received)
                .await
                .map_err(|error| error.to_string())?;
            assert_eq!(received.as_slice(), b"test-only-password\n");
        }
        Ok(())
    }

    #[cfg(any(unix, windows))]
    #[tokio::test]
    async fn password_is_not_exposed_in_ssh_arguments_or_environment() -> Result<(), String> {
        let mut config = valid_config();
        config.authentication = RemoteAuthentication::Password;
        let secret = "test-password-that-must-not-leak";
        let password = Arc::new(Zeroizing::new(secret.to_owned()));
        let (command, broker) = ssh_command(&config, Some(&password)).await?;
        let command = command.as_std();
        assert!(command
            .get_args()
            .all(|argument| !argument.to_string_lossy().contains(secret)));
        assert!(command
            .get_envs()
            .all(|(_, value)| value.is_none_or(|value| !value.to_string_lossy().contains(secret))));
        assert!(broker.is_some());
        Ok(())
    }
}
