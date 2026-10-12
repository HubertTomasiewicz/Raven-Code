$ErrorActionPreference = 'Stop'
$installRoot = Join-Path $env:LOCALAPPDATA 'Programs\RavenApp\RavenCode'
$startMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
$sourceRoot = $PSScriptRoot
New-Item -ItemType Directory -Force -Path $installRoot | Out-Null
Copy-Item -Path (Join-Path $sourceRoot '*') -Destination $installRoot -Recurse -Force
$exe = Join-Path $installRoot 'RavenCode.exe'
if (-not (Test-Path -LiteralPath $exe)) { throw "Installed executable not found: $exe" }
$register = Join-Path $installRoot 'register-raven-file-icons.cmd'
if (-not (Test-Path -LiteralPath $register)) { throw 'File association setup script was not installed.' }
$proc = Start-Process -FilePath "$env:ComSpec" -ArgumentList @('/d','/c',('"{0}" /quiet' -f $register)) -Wait -PassThru -WindowStyle Hidden
if ($proc.ExitCode -ne 0) { throw "Raven Code file registration failed with exit code $($proc.ExitCode)." }
New-Item -ItemType Directory -Force -Path $startMenu | Out-Null
$shortcutPath = Join-Path $startMenu 'Raven Code.lnk'
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($shortcutPath)
$shortcut.TargetPath = $exe
$shortcut.WorkingDirectory = $installRoot
$shortcut.IconLocation = "$exe,0"
$shortcut.Description = 'Raven Code'
$shortcut.Save()
Write-Output "Installed Raven Code to $installRoot"
Write-Output "Created Start Menu shortcut: $shortcutPath"
