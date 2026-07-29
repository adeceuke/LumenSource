[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet("check", "package", "run", "help")]
    [string]$Command = "help",

    [Parameter(Position = 1, ValueFromRemainingArguments)]
    [string[]]$Arguments
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$RepositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$DesktopRoot = Join-Path $RepositoryRoot "apps\desktop"

function Invoke-Native {
    param(
        [Parameter(Mandatory)]
        [string]$FilePath,
        [Parameter(ValueFromRemainingArguments)]
        [string[]]$NativeArguments
    )

    & $FilePath @NativeArguments
    if ($LASTEXITCODE -ne 0) {
        throw "'$FilePath $($NativeArguments -join ' ')' failed with exit code $LASTEXITCODE."
    }
}

function Show-Usage {
    @"
Usage: powershell -ExecutionPolicy Bypass -File scripts/container.ps1 <command> [arguments]

Commands:
  check             Run formatting, linting, Rust tests, and frontend build.
  package           Build Windows MSI and NSIS installer artifacts.
  run <command...>  Run an arbitrary command from the repository root.
  help              Show this help.

This script is the Windows-native counterpart to container.sh. Windows Tauri
packages require the MSVC toolchain and WebView2, so this workflow intentionally
runs on the host rather than in the Ubuntu container.
"@
}

Push-Location $RepositoryRoot
try {
    switch ($Command) {
        "check" {
            Invoke-Native cargo fmt --all --check
            [string[]]$ClippyArguments = @(
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings"
            )
            Invoke-Native cargo @ClippyArguments
            Invoke-Native cargo test --workspace
            Push-Location $DesktopRoot
            try {
                Invoke-Native npm ci
                Invoke-Native npm run typecheck
                Invoke-Native npm run build
            }
            finally {
                Pop-Location
            }
        }
        "package" {
            Push-Location $DesktopRoot
            try {
                Invoke-Native npm ci
                [string[]]$TauriArguments = @(
                    "run",
                    "tauri",
                    "build",
                    "--",
                    "--bundles",
                    "nsis,msi"
                )
                Invoke-Native npm @TauriArguments
            }
            finally {
                Pop-Location
            }
            Write-Host "Packages are available under target\release\bundle\."
        }
        "run" {
            if (-not $Arguments -or $Arguments.Count -eq 0) {
                throw "The run command requires a command to execute."
            }
            [string[]]$RunArguments = if ($Arguments.Count -gt 1) {
                @($Arguments[1..($Arguments.Count - 1)])
            }
            else {
                @()
            }
            Invoke-Native ($Arguments[0]) @RunArguments
        }
        "help" {
            Show-Usage
        }
    }
}
finally {
    Pop-Location
}
