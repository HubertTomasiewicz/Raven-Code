# Per-user Raven Code file icon and Open With setup. No administrator rights
# are needed; existing default apps are preserved except for .rvenc.
$ErrorActionPreference = 'Stop'
$scriptDir = (Resolve-Path $PSScriptRoot).Path
$repo = (Resolve-Path (Join-Path $scriptDir '..')).Path
$exeCandidates = @(
    (Join-Path $scriptDir 'RavenCode.exe'),
    (Join-Path $scriptDir 'raven-code.exe'),
    (Join-Path $repo 'target\release\raven-code.exe'),
    (Join-Path $repo 'target\debug\raven-code.exe')
)
$exe = $exeCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $exe) { throw 'RavenCode.exe was not found beside this script, and no repository release/debug build was found.' }
$exe = (Resolve-Path -LiteralPath $exe).Path
$iconDir = Join-Path $env:LOCALAPPDATA 'RavenCode\file-icons'
New-Item -ItemType Directory -Force -Path $iconDir | Out-Null
foreach ($name in @('workspace','code','binary','hash')) {
    $iconCandidates = @(
        (Join-Path $scriptDir "file-icons\$name.ico"),
        (Join-Path $repo "apps\raven-code\assets\file-types\$name.ico")
    )
    $source = $iconCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if (-not (Test-Path -LiteralPath $source)) { throw "Missing file icon: $source" }
    Copy-Item -LiteralPath $source -Destination (Join-Path $iconDir "$name.ico") -Force
}
$classes = 'HKCU:\Software\Classes'
$exeCommand = '"{0}" "%1"' -f $exe
$groups = @{
    'RavenCode.Workspace' = @{ Label = 'Raven Code Workspace'; Icon = 'workspace.ico'; Extensions = @('.rvenc') }
    'RavenCode.Text' = @{ Label = 'Raven Code Text File'; Icon = 'code.ico'; Extensions = @('.txt','.rs','.toml','.py','.js','.ts','.md','.json','.yaml','.yml','.xml','.sql','.html','.css','.sh','.ps1','.c','.cpp','.h','.hpp','.cs','.java','.go','.lua','.php','.csv','.tsv') }
    'RavenCode.Binary' = @{ Label = 'Raven Code Binary File'; Icon = 'binary.ico'; Extensions = @('.bin','.pdb','.pak','.wad','.dat','.db') }
}
foreach ($progId in $groups.Keys) {
    $group = $groups[$progId]
    $progPath = Join-Path $classes $progId
    New-Item -Path $progPath -Force | Out-Null
    Set-Item -Path $progPath -Value $group.Label
    $iconPath = Join-Path $iconDir $group.Icon
    New-Item -Path (Join-Path $progPath 'DefaultIcon') -Force | Out-Null
    Set-Item -Path (Join-Path $progPath 'DefaultIcon') -Value $iconPath
    $commandPath = Join-Path $progPath 'shell\open\command'
    New-Item -Path $commandPath -Force | Out-Null
    Set-Item -Path $commandPath -Value $exeCommand
    foreach ($extension in $group.Extensions) {
        $extensionPath = Join-Path $classes $extension
        New-Item -Path $extensionPath -Force | Out-Null
        if ($extension -eq '.rvenc') { Set-Item -Path $extensionPath -Value $progId }
        $openWith = Join-Path $extensionPath 'OpenWithProgids'
        New-Item -Path $openWith -Force | Out-Null
        New-ItemProperty -Path $openWith -Name $progId -PropertyType String -Value '' -Force | Out-Null
        $systemIcon = Join-Path (Join-Path $classes "SystemFileAssociations\$extension") 'DefaultIcon'
        New-Item -Path $systemIcon -Force | Out-Null
        Set-Item -Path $systemIcon -Value $iconPath
    }
}
if (-not ('RavenCode.ShellNotify' -as [type])) {
    Add-Type -Namespace RavenCode -Name ShellNotify -MemberDefinition @'
        [System.Runtime.InteropServices.DllImport("shell32.dll")]
        public static extern void SHChangeNotify(uint eventId, uint flags, System.IntPtr item1, System.IntPtr item2);
'@
}
[RavenCode.ShellNotify]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
Write-Host "Raven Code icons and Open With entries registered for this Windows account. Windows Explorer was notified to refresh its file associations."
Write-Host "Executable: $exe"
