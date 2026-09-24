param([Parameter(Mandatory = $true)][string]$Setup)

$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or -not $env:RUNNER_TEMP) {
    throw 'Run this destructive upgrade test only on an isolated GitHub Actions Windows runner.'
}
$setupPath = (Resolve-Path -LiteralPath $Setup).Path
$old = Join-Path $env:LOCALAPPDATA 'Programs\x-notify-service'
$new = Join-Path $env:LOCALAPPDATA 'Programs\Hexinfo\x-notify-service'
$customOld = Join-Path $env:RUNNER_TEMP 'legacy-custom-x-notify-service'
$customNew = Join-Path $env:RUNNER_TEMP 'new-custom-x-notify-service'
foreach ($path in @($old, $new, $customOld, $customNew)) {
    if (Test-Path -LiteralPath $path) { throw "Test path already exists: $path" }
}

New-Item -ItemType Directory -Path $old, $customOld | Out-Null
Set-Content -LiteralPath (Join-Path $old 'config.toml') -Value 'port = 17321'
Set-Content -LiteralPath (Join-Path $old 'user-note.txt') -Value 'keep me'
Set-Content -LiteralPath (Join-Path $customOld 'user-note.txt') -Value 'custom keep me'
New-Item -Path 'HKCU:\Software\x-notify-service' -Force | Out-Null
Set-ItemProperty -Path 'HKCU:\Software\x-notify-service' -Name InstallDir -Value $customOld

function Invoke-Setup([string[]]$Arguments) {
    $process = Start-Process -FilePath $setupPath -ArgumentList $Arguments -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Installer exited $($process.ExitCode)" }
}

Invoke-Setup @('/S')
if (-not (Test-Path -LiteralPath (Join-Path $new 'x-notify-service.exe'))) { throw 'New executable missing' }
if ((Get-Content -LiteralPath (Join-Path $new 'config.toml') -Raw).Trim() -ne 'port = 17321') { throw 'Legacy config lost' }
if ((Get-Content -LiteralPath (Join-Path $new 'user-note.txt') -Raw).Trim() -ne 'keep me') { throw 'Unknown user file lost' }
if (Test-Path -LiteralPath $old) { throw 'Legacy default directory remains' }
$registered = (Get-ItemProperty -Path 'HKCU:\Software\Hexinfo\x-notify-service' -Name InstallDir).InstallDir
if ($registered -ne $new) { throw "Wrong registered installation path: $registered" }

# A preexisting new destination wins every name collision; the old files stay recoverable.
New-Item -ItemType Directory -Path $old | Out-Null
Set-Content -LiteralPath (Join-Path $old 'config.toml') -Value 'port = 17322'
Set-Content -LiteralPath (Join-Path $old 'user-note.txt') -Value 'old conflicting note'
Invoke-Setup @('/S')
if ((Get-Content -LiteralPath (Join-Path $new 'config.toml') -Raw).Trim() -ne 'port = 17321') { throw 'New config was overwritten' }
if ((Get-Content -LiteralPath (Join-Path $new 'user-note.txt') -Raw).Trim() -ne 'keep me') { throw 'New user file was overwritten' }
if ((Get-Content -LiteralPath (Join-Path $old 'config.toml') -Raw).Trim() -ne 'port = 17322') { throw 'Conflicting old config was lost' }
if ((Get-Content -LiteralPath (Join-Path $old 'user-note.txt') -Raw).Trim() -ne 'old conflicting note') { throw 'Conflicting old user file was lost' }

Invoke-Setup @('/S', "/D=$customNew")
if (-not (Test-Path -LiteralPath (Join-Path $customNew 'x-notify-service.exe'))) { throw 'Custom destination missing' }
if ((Get-Content -LiteralPath (Join-Path $customOld 'user-note.txt') -Raw).Trim() -ne 'custom keep me') {
    throw 'Legacy custom directory was modified'
}
Write-Host 'Windows default, conflicting, and custom-path upgrade checks passed.'
