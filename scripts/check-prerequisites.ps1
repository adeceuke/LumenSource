[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$Failed = $false

function Test-NativeCommand {
    param([Parameter(Mandatory)][string]$Name)

    $resolved = Get-Command $Name -ErrorAction SilentlyContinue
    if (-not $resolved) {
        Write-Error "missing  $Name" -ErrorAction Continue
        $script:Failed = $true
        return
    }
    $version = (& $Name --version 2>&1 | Select-Object -First 1)
    Write-Host ("ok  {0,-12} {1}" -f $Name, $version)
}

if (-not [Environment]::Is64BitOperatingSystem) {
    Write-Error "Lumen Source currently requires 64-bit Windows." -ErrorAction Continue
    $Failed = $true
}
else {
    Write-Host "ok  platform     64-bit Windows"
}

Test-NativeCommand cargo
Test-NativeCommand rustc
Test-NativeCommand rustfmt
Test-NativeCommand cargo-clippy
Test-NativeCommand node
Test-NativeCommand npm

$RustHost = if (Get-Command rustc -ErrorAction SilentlyContinue) {
    (& rustc -vV | Select-String "^host:").Line
}
if ($RustHost -and $RustHost -notmatch "pc-windows-msvc") {
    Write-Error "Rust must use an MSVC Windows host toolchain; detected '$RustHost'." -ErrorAction Continue
    $Failed = $true
}

$Linker = Get-Command link.exe -ErrorAction SilentlyContinue
if (-not $Linker) {
    Write-Error "missing  MSVC link.exe. Run this from a Visual Studio Developer PowerShell, or install Visual Studio 2022 Build Tools with 'Desktop development with C++'." -ErrorAction Continue
    $Failed = $true
}
else {
    Write-Host "ok  linker       $($Linker.Source)"
}

$WebView2 = Get-ItemProperty `
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F1E7E50D-5E14-44A1-A17F-3D7B9D3B7C28}" `
    -ErrorAction SilentlyContinue
if ($WebView2) {
    Write-Host "ok  WebView2     $($WebView2.pv)"
}
else {
    Write-Warning "WebView2 Runtime was not found in the machine-wide registry. Current Windows releases normally include it; install the Evergreen WebView2 Runtime if packaging or startup fails."
}

if ($Failed) {
    throw "One or more Windows development prerequisites are missing."
}
Write-Host "All required command-line prerequisites are available."
