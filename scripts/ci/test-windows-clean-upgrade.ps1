param(
    [Parameter(Mandatory = $true)][string]$Setup,
    [Parameter(Mandatory = $true)][string]$ServiceExe
)

$ErrorActionPreference = 'Stop'
$app = 'x-notify-service'
$newDir = Join-Path $env:LOCALAPPDATA "Programs\Hexinfo\$app"
$oldDir = Join-Path $env:LOCALAPPDATA "Programs\$app"
$dataDir = Join-Path $env:LOCALAPPDATA $app
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

    # The directory page can select the old default path for a new installation.
    Invoke-Installer (Join-Path $newDir 'uninstall.exe') @('/S')
    Invoke-Installer $Setup @('/S', "/D=$oldDir")
    Assert (Test-Path (Join-Path $oldDir "$app.exe")) 'Installer deleted its selected install directory'
    Assert ((Get-ItemProperty $newKey).InstallDir -eq $oldDir) 'Custom install path was not registered'
} finally {
    if (Test-Path (Join-Path $newDir 'uninstall.exe')) {
        Invoke-Installer (Join-Path $newDir 'uninstall.exe') @('/S')
    }
    if (Test-Path (Join-Path $oldDir 'uninstall.exe')) {
        Invoke-Installer (Join-Path $oldDir 'uninstall.exe') @('/S')
    }
    Remove-Item $oldDir -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item $oldKey -Recurse -Force -ErrorAction SilentlyContinue
}
