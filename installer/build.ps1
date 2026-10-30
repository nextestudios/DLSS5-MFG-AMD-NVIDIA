# Builds dist\DLSS5-FrameGen-Setup.exe and tests it (silent install, smoke test of the installed exe, silent uninstall), then tests the portable mode.
# Usage (repo root, after cargo build --release): pwsh installer/build.ps1 -Version 1.2.0
param([Parameter(Mandatory)][string]$Version)
$ErrorActionPreference = 'Stop'

$iscc = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
if (!(Test-Path $iscc)) { choco install innosetup -y --no-progress | Out-Null }
New-Item -ItemType Directory -Force dist | Out-Null
& $iscc /Qp "/DAppVersion=$Version" "/DExe=$PWD\target\release\gpu-mods-app.exe" "/O$PWD\dist" installer\DLSS5-FrameGen.iss
if ($LASTEXITCODE) { throw "ISCC falhou ($LASTEXITCODE)" }
$setup = "$PWD\dist\DLSS5-FrameGen-Setup.exe"

$p = Start-Process $setup -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/TASKS=' -Wait -PassThru
if ($p.ExitCode) { throw "Instalação silenciosa falhou ($($p.ExitCode))" }
$app = "$env:LOCALAPPDATA\Programs\DLSS5-FrameGen"
if (!(Test-Path "$app\DLSS5-FrameGen.exe")) { throw "O exe não foi instalado em $app" }

$report = "$env:TEMP\smoke-installed.txt"
$p = Start-Process "$app\DLSS5-FrameGen.exe" -ArgumentList '--smoke-test', $report -Wait -PassThru
Get-Content $report
if ($p.ExitCode) { throw "Teste de fumaça do app instalado falhou ($($p.ExitCode))" }

$p = Start-Process "$app\unins000.exe" -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES' -Wait -PassThru
if ($p.ExitCode) { throw "Desinstalação falhou ($($p.ExitCode))" }
# The uninstaller finishes in a copy of itself: wait for the files to go.
for ($i = 0; $i -lt 30 -and (Test-Path "$app\DLSS5-FrameGen.exe"); $i++) { Start-Sleep 1 }
if (Test-Path "$app\DLSS5-FrameGen.exe") { throw "A desinstalação deixou o exe em $app" }
Write-Host "Instalador OK: $setup"

# Portable mode: portable.txt next to the exe keeps the data in a GpuMods folder beside it.
$port = "$env:TEMP\gpumods-portable-test"
Remove-Item -Recurse -Force $port -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $port | Out-Null
Copy-Item target\release\gpu-mods-app.exe "$port\DLSS5-FrameGen.exe"
Set-Content "$port\portable.txt" 'portable'
$p = Start-Process "$port\DLSS5-FrameGen.exe" -ArgumentList '--smoke-test', "$port\smoke.txt" -Wait -PassThru
if ($p.ExitCode) { Get-Content "$port\smoke.txt"; throw "Teste de fumaça da versão portátil falhou ($($p.ExitCode))" }
if (!(Test-Path "$port\GpuMods\cache")) { throw "A versão portátil não guardou os dados ao lado do exe" }
Write-Host "Versão portátil OK"
