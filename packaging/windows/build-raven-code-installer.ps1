$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$versionText = Get-Content -LiteralPath (Join-Path $root 'Cargo.toml') -Raw
if ($versionText -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'Could not read the Raven Code version from Cargo.toml.' }
$version = $Matches[1]
$payload = Join-Path $root "dist\raven-code-$version-windows-x86_64"
if (-not (Test-Path -LiteralPath (Join-Path $payload 'RavenCode.exe'))) { & (Join-Path $root 'scripts\package-raven-code-windows.ps1') }
$outDir = Join-Path $root 'dist\installer'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$stage = Join-Path $env:TEMP 'RavenCodeInstallerPayload'
if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item -Path (Join-Path $payload '*') -Destination $stage -Recurse -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'install-raven-code.ps1') -Destination $stage
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'install-raven-code.cmd') -Destination $stage
$sedPath = Join-Path $env:TEMP 'RavenCodeInstaller.sed'
$tempTarget = Join-Path $env:TEMP "RavenCodeSetup-$version-x64.exe"
$files = Get-ChildItem -LiteralPath $stage -File -Recurse
$sourceRows = [System.Collections.Generic.List[string]]::new()
$stringRows = [System.Collections.Generic.List[string]]::new()
$i = 0
foreach ($file in $files) {
    $relative = $file.FullName.Substring($stage.Length).TrimStart('\','/')
    $sourceRows.Add("%FILE$i%=$($file.FullName)")
    $stringRows.Add("FILE$i=`"$relative`"")
    $i++
}
$sed = @(
    '[Version]','Class=IEXPRESS','SEDVersion=3','[Options]',
    'PackagePurpose=InstallApp','ShowInstallProgramWindow=1','HideExtractAnimation=0',
    'UseLongFileName=1','InsideCompressed=1','CAB_FixedSize=0','CAB_ResvCodeSigning=0',
    'RebootMode=N','InstallPrompt=','DisplayLicense=','FinishMessage=',
    "TargetName=$tempTarget",'FriendlyName=Raven Code Setup','AppLaunched=cmd.exe /c install-raven-code.cmd',
    'PostInstallCmd=<None>','AdminQuietInstCmd=','UserQuietInstCmd=','SourceFiles=SourceFiles',
    '[SourceFiles]',"SourceFiles0=$stage",'[SourceFiles0]'
) + $sourceRows + @('[Strings]') + $stringRows
Set-Content -LiteralPath $sedPath -Value $sed -Encoding ASCII
$iexpress = Join-Path $env:WINDIR 'System32\iexpress.exe'
if (-not (Test-Path -LiteralPath $iexpress)) { throw 'Windows IExpress is not available.' }
if (Test-Path -LiteralPath $tempTarget) { Remove-Item -LiteralPath $tempTarget -Force }
$process = Start-Process -FilePath $iexpress -ArgumentList @('/N','/Q',$sedPath) -Wait -PassThru
if ($process.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $tempTarget)) { throw "IExpress failed (exit $($process.ExitCode)). Review $sedPath" }
$finalTarget = Join-Path $outDir "RavenCodeSetup-$version-x64.exe"
Copy-Item -LiteralPath $tempTarget -Destination $finalTarget -Force
Write-Output "Installer created: $finalTarget"
