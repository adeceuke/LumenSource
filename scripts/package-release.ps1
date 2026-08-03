[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidatePattern("^[A-Fa-f0-9]{40}$")]
    [string]$CertificateThumbprint,

    [string]$TimestampUrl = "http://timestamp.digicert.com"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$RepositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$DesktopRoot = Join-Path $RepositoryRoot "apps\desktop"
$BaseConfigPath = Join-Path $DesktopRoot "src-tauri\tauri.conf.json"
$ReleaseConfigPath = Join-Path $DesktopRoot "src-tauri\tauri.release.conf.json"
$CertificatePath = "Cert:\CurrentUser\My\$CertificateThumbprint"

if (-not (Test-Path -LiteralPath $CertificatePath)) {
    throw "The release-signing certificate '$CertificateThumbprint' is not installed for the current user."
}

$ReleaseConfig = Get-Content -LiteralPath $BaseConfigPath -Raw | ConvertFrom-Json
$ReleaseConfig.bundle.windows | Add-Member -NotePropertyName certificateThumbprint -NotePropertyValue $CertificateThumbprint -Force
$ReleaseConfig.bundle.windows | Add-Member -NotePropertyName digestAlgorithm -NotePropertyValue "sha256" -Force
$ReleaseConfig.bundle.windows | Add-Member -NotePropertyName timestampUrl -NotePropertyValue $TimestampUrl -Force
$ReleaseConfig | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $ReleaseConfigPath -Encoding UTF8

try {
    Push-Location $DesktopRoot
    try {
        npm.cmd ci
        if ($LASTEXITCODE -ne 0) { throw "npm ci failed." }
        npm.cmd run tauri build -- --bundles nsis,msi --config $ReleaseConfigPath
        if ($LASTEXITCODE -ne 0) { throw "The signed Tauri package build failed." }
    }
    finally {
        Pop-Location
    }

    $Artifacts = @(
        Get-ChildItem -Path (Join-Path $RepositoryRoot "target\release\bundle") -Recurse -File |
            Where-Object { $_.Extension -in ".exe", ".msi" }
    )
    if ($Artifacts.Count -eq 0) {
        throw "No Windows installer artifacts were produced."
    }
    foreach ($Artifact in $Artifacts) {
        $Signature = Get-AuthenticodeSignature -LiteralPath $Artifact.FullName
        if ($Signature.Status -ne "Valid") {
            throw "Release artifact '$($Artifact.FullName)' is not validly signed: $($Signature.Status)."
        }
        Get-FileHash -Algorithm SHA256 -LiteralPath $Artifact.FullName
    }
}
finally {
    if (Test-Path -LiteralPath $ReleaseConfigPath) {
        Remove-Item -LiteralPath $ReleaseConfigPath -Force
    }
}
