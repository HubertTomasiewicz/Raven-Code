# Creates a portable, repo-independent Windows distribution and ZIP archive.
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$cargo = Get-Content -LiteralPath (Join-Path $repo 'Cargo.toml') -Raw
$match = [regex]::Match($cargo, '(?ms)^\[workspace\.package\]\s.*?^version\s*=\s*"([^"]+)"')
if (-not $match.Success) { throw 'Could not read workspace.package.version from Cargo.toml.' }
$version = $match.Groups[1].Value
$exe = Join-Path $repo 'target\release\raven-code.exe'
if (-not (Test-Path -LiteralPath $exe)) { throw 'Release executable is missing. Run: cargo build --release -p raven-code' }
$dist = Join-Path $repo 'dist'
$distRoot = (Resolve-Path -LiteralPath $dist).Path
$packageName = "raven-code-$version-windows-x86_64"
$package = Join-Path $distRoot $packageName
$zip = Join-Path $distRoot "$packageName.zip"
$packageFull = [IO.Path]::GetFullPath($package)
$zipFull = [IO.Path]::GetFullPath($zip)
$prefix = $distRoot.TrimEnd('\') + '\'
if (-not $packageFull.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) -or
    -not $zipFull.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Package output escaped the dist folder; refusing to overwrite files.'
}
if (Test-Path -LiteralPath $packageFull) { Remove-Item -LiteralPath $packageFull -Recurse -Force }
if (Test-Path -LiteralPath $zipFull) { Remove-Item -LiteralPath $zipFull -Force }
New-Item -ItemType Directory -Force -Path $packageFull | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $packageFull 'file-icons') | Out-Null
Copy-Item -LiteralPath $exe -Destination (Join-Path $packageFull 'RavenCode.exe')
Copy-Item -Path (Join-Path $repo 'apps\raven-code\assets\file-types\*.ico') -Destination (Join-Path $packageFull 'file-icons')
Copy-Item -LiteralPath (Join-Path $repo 'scripts\register-raven-file-icons.ps1') -Destination $packageFull
Copy-Item -LiteralPath (Join-Path $repo 'scripts\register-raven-file-icons.cmd') -Destination $packageFull
Copy-Item -LiteralPath (Join-Path $repo 'LICENSE') -Destination $packageFull
Copy-Item -LiteralPath (Join-Path $repo 'THIRD-PARTY-LICENSES.md') -Destination $packageFull
Copy-Item -LiteralPath (Join-Path $repo 'docs\raven-code\workspaces.md') -Destination (Join-Path $packageFull 'WORKSPACES.md')
$readme = @"
Raven Code $version for Windows x64 (portable)

Run RavenCode.exe. This folder can be moved anywhere; no repository or Rust installation is needed.

Optional Windows file icons:
  Double-click register-raven-file-icons.cmd
  This adds .rvenc as a Raven Code workspace type and adds Raven Code to Open with for supported text/binary types.
  It does not replace the default app for source files. Windows Explorer may need a refresh or restart to update cached icons.

Workspace files use the .rvenc extension. Open a folder and choose File > Save Workspace As to create one.
Settings and custom syntax definitions are stored per-user under %APPDATA%\RavenCode, separately from this portable folder.

See WORKSPACES.md for workspace details and THIRD-PARTY-LICENSES.md for bundled asset notices.
"@
Set-Content -LiteralPath (Join-Path $packageFull 'README.txt') -Value $readme -Encoding utf8
Compress-Archive -Path (Join-Path $packageFull '*') -DestinationPath $zipFull -CompressionLevel Optimal
Write-Host "Portable folder: $packageFull"
Write-Host "Portable ZIP:    $zipFull"
