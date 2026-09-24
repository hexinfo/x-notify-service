param(
    [Parameter(Mandatory = $true)][string]$Setup,
    [Parameter(Mandatory = $true)][string]$ServiceExe
)

$ErrorActionPreference = 'Stop'
$app = 'x-notify-service'
$newDir = Join-Path $env:LOCALAPPDATA "Programs\Hexinfo\$app"
$oldDir = Join-Path $env:LOCALAPPDATA "Programs\$app"
$dataDir = Join-Path $env:LOCALAPPDATA $app
$newDataDir = Join-Path $env:LOCALAPPDATA "Hexinfo\$app"
$oldKey = "HKCU:\Software\$app"
$newKey = "HKCU:\Software\Hexinfo\$app"
$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$app"
$logSentinel = Join-Path $dataDir 'logs\upgrade-sentinel.txt'

function Invoke-Installer([string]$path, [string[]]$arguments) {
    $process = Start-Process -FilePath $path -ArgumentList $arguments -PassThru
    try {
        if (-not $process.WaitForExit(60000)) {
            $process.Kill()
            throw "Timed out: $path"
        }
        if ($process.ExitCode -ne 0) { throw "Exit $($process.ExitCode): $path" }
    } finally {
        $process.Dispose()
    }
}

function Assert([bool]$condition, [string]$message) {
    if (-not $condition) { throw $message }
}

# Run on an isolated Windows user/runner: this test installs and uninstalls the app.
Assert (-not (Test-Path $newDir)) "New installation already exists: $newDir"
Assert (-not (Test-Path $oldDir)) "Old installation already exists: $oldDir"

try {
    Invoke-Installer $Setup @('/S')
    Assert (Test-Path (Join-Path $newDir "$app.exe")) 'Fresh install missing executable'
    Assert ((Get-ItemProperty $newKey).InstallDir -eq $newDir) 'New install registry path is wrong'
    Assert ((Get-ItemProperty $uninstallKey).InstallLocation -eq $newDir) 'Uninstall registry path is wrong'

    # A second install at the new location must preserve an edited config.
    $newConfig = Join-Path $newDir 'config.toml'
    Set-Content -Path $newConfig -Value '# edited new config' -Encoding utf8
    Invoke-Installer $Setup @('/S')
    Assert ((Get-Content $newConfig -Raw).Contains('# edited new config')) 'Existing new config was overwritten'

    Invoke-Installer (Join-Path $newDir 'uninstall.exe') @('/S')
    New-Item -ItemType Directory -Path $oldDir -Force | Out-Null
    Copy-Item $ServiceExe (Join-Path $oldDir "$app.exe")
    Set-Content (Join-Path $oldDir 'config.toml') '# old installer config'
    New-Item -ItemType Directory -Path (Split-Path $logSentinel) -Force | Out-Null
    Set-Content $logSentinel 'keep old logs'
    Set-Content (Join-Path $dataDir 'config.toml') '# old user config'
    New-Item -Path $oldKey -Force | Out-Null
    Set-ItemProperty -Path $oldKey -Name InstallDir -Value $oldDir

    Invoke-Installer $Setup @('/S')
    Assert (Test-Path (Join-Path $newDir "$app.exe")) 'Upgrade missing new executable'
    Assert ((Get-Content $newConfig -Raw).Contains('# edited new config')) 'Upgrade overwrote existing new config'
    Assert (-not (Test-Path $oldDir)) 'Old program directory was not removed'
    Assert (-not (Test-Path (Join-Path $dataDir 'config.toml'))) 'Old config was not removed'
    Assert (-not (Test-Path $dataDir)) 'Old config/log/data directory was not removed'
    Assert (-not (Test-Path $oldKey)) 'Old install registry key was not removed'
    Assert ((Get-ItemProperty $newKey).InstallDir -eq $newDir) 'Upgrade did not register new path'
    $newPort = Join-Path $newDataDir 'port'
    for ($attempt = 0; $attempt -lt 50 -and -not (Test-Path $newPort); $attempt++) {
        Start-Sleep -Milliseconds 200
    }
    Assert (Test-Path $newPort) 'New port/data path did not survive old-data cleanup'

    # Compile a deterministic old uninstaller stub to exercise both exit paths.
    Invoke-Installer (Join-Path $newDir 'uninstall.exe') @('/S')
    $stubSource = Join-Path $env:TEMP 'hexinfo-old-uninstall-stub.cs'
    $stubMarker = Join-Path $env:TEMP 'hexinfo-old-uninstall-ran.txt'
    $stubExe = Join-Path $oldDir 'uninstall.exe'
    New-Item -ItemType Directory -Path $oldDir -Force | Out-Null
    Set-Content -Path $stubSource -Value @'
using System;
using System.IO;
class Program {
    static int Main() {
        File.WriteAllText(Environment.GetEnvironmentVariable("HEXINFO_TEST_OLD_UNINSTALL_MARKER"), "called");
        return int.Parse(Environment.GetEnvironmentVariable("HEXINFO_TEST_OLD_UNINSTALL_EXIT"));
    }
}
'@
    $compiler = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
    & $compiler /nologo /target:exe "/out:$stubExe" $stubSource
    Assert ($LASTEXITCODE -eq 0) 'Could not compile old uninstaller stub'
    $env:HEXINFO_TEST_OLD_UNINSTALL_MARKER = $stubMarker
    $env:HEXINFO_TEST_OLD_UNINSTALL_EXIT = '7'
    $rejected = $false
    try { Invoke-Installer $Setup @('/S') } catch { $rejected = $true }
    Assert $rejected 'Installer accepted a failing old uninstaller'
    Assert (Test-Path $stubMarker) 'Old uninstaller branch was not executed'
    Assert (-not (Test-Path (Join-Path $newDir "$app.exe"))) 'New version installed after old uninstall failed'
    Remove-Item $stubMarker
    $env:HEXINFO_TEST_OLD_UNINSTALL_EXIT = '0'
    Invoke-Installer $Setup @('/S')
    Assert (Test-Path $stubMarker) 'Successful old uninstaller branch was not executed'
    Assert (-not (Test-Path $oldDir)) 'Old program directory remained after successful stub uninstall'
    Assert (Test-Path (Join-Path $newDir "$app.exe")) 'New version missing after successful stub uninstall'

    # The directory page can select the old default path for a new installation.
    Invoke-Installer (Join-Path $newDir 'uninstall.exe') @('/S')
    Invoke-Installer $Setup @('/S', "/D=$oldDir")
    Assert (Test-Path (Join-Path $oldDir "$app.exe")) 'Installer deleted its selected install directory'
    Assert ((Get-ItemProperty $newKey).InstallDir -eq $oldDir) 'Custom install path was not registered'
    Invoke-Installer (Join-Path $oldDir 'uninstall.exe') @('/S')

    $nestedDir = Join-Path $oldDir 'custom\nested'
    Invoke-Installer $Setup @('/S', "/D=$nestedDir")
    Assert (Test-Path (Join-Path $nestedDir "$app.exe")) 'Installer deleted a nested selected install directory'
    Assert ((Get-ItemProperty $newKey).InstallDir -eq $nestedDir) 'Nested install path was not registered'
} finally {
    if (Test-Path (Join-Path $newDir 'uninstall.exe')) {
        Invoke-Installer (Join-Path $newDir 'uninstall.exe') @('/S')
    }
    if ($nestedDir -and (Test-Path (Join-Path $nestedDir 'uninstall.exe'))) {
        Invoke-Installer (Join-Path $nestedDir 'uninstall.exe') @('/S')
    }
    Remove-Item Env:HEXINFO_TEST_OLD_UNINSTALL_MARKER -ErrorAction SilentlyContinue
    Remove-Item Env:HEXINFO_TEST_OLD_UNINSTALL_EXIT -ErrorAction SilentlyContinue
    if ($stubSource) { Remove-Item $stubSource -ErrorAction SilentlyContinue }
    if ($stubMarker) { Remove-Item $stubMarker -ErrorAction SilentlyContinue }
    Remove-Item $oldDir -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item $oldKey -Recurse -Force -ErrorAction SilentlyContinue
}
