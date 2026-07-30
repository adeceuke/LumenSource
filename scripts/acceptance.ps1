[CmdletBinding()]
param(
    [string]$OutputDirectory = (Join-Path $PWD "acceptance-results")
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$RepositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$ResolvedOutput = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $ResolvedOutput -Force | Out-Null

Push-Location $RepositoryRoot
try {
    powershell -ExecutionPolicy Bypass -File scripts\container.ps1 check
    if ($LASTEXITCODE -ne 0) { throw "The automated Windows acceptance check failed." }

    $Commit = (git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw "Could not identify the tested commit." }
    $Timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $ReportPath = Join-Path $ResolvedOutput "windows-$Timestamp.md"
    $Os = Get-CimInstance Win32_OperatingSystem
    $Cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
    $Gpu = @(Get-CimInstance Win32_VideoController | ForEach-Object { $_.Name }) -join ", "

    @"
# Windows 1.0 acceptance evidence

- Commit: `$Commit`
- Recorded: `$(Get-Date -Format o)`
- OS: `$($Os.Caption) $($Os.Version) build $($Os.BuildNumber)`
- CPU: `$($Cpu.Name)`
- RAM bytes: `$($Os.TotalVisibleMemorySize * 1024)`
- GPU: `$Gpu`
- Automated suite: PASS
- Package path/hash/signature: PENDING
- Tester: PENDING

## Required cases

| Case | Result | Evidence / defect |
| --- | --- | --- |
| WIN-PKG-001 | PENDING | |
| WIN-INSTALL-001 | PENDING | |
| WIN-OLLAMA-001 | PENDING | |
| WIN-OLLAMA-002 | PENDING | |
| WIN-STATE-001 | PENDING | |
| WIN-UPGRADE-001 | PENDING | |
| WIN-RECOVERY-001 | PENDING | |
| WIN-UNINSTALL-001 | PENDING | |
| WIN-UNINSTALL-002 | PENDING | |
| EXT-OLLAMA-001 | PENDING | |
| EXT-VLLM-001 | PENDING | |
| REMOTE-OLLAMA-001 | PENDING | |
| RESOURCE-001 | PENDING | |
| SHARE-001 | PENDING | |
| SHARE-002 | PENDING | |
| SHARE-003 | PENDING | |
| LOW-DISK-001 | PENDING | |
| LOW-MEMORY-001 | PENDING | |
| A11Y-KEYBOARD-001 | PENDING | |
| A11Y-SCREENREADER-001 | PENDING | |
| A11Y-CONTRAST-001 | PENDING | |
| COMPREHENSION-001 | PENDING | |
"@ | Set-Content -LiteralPath $ReportPath -Encoding UTF8

    Write-Host "Created $ReportPath"
}
finally {
    Pop-Location
}
