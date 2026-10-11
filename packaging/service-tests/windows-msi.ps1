# Disposable CI machine only: build the real installer definition around a synthetic
# service executable, then test opt-in installation and component removal.
$ErrorActionPreference = 'Stop'
$stage = Join-Path $env:RUNNER_TEMP 'hot-folder-msi'
New-Item -ItemType Directory -Path "$stage/models" -Force | Out-Null
foreach ($name in 'pdfcraft.exe', 'pdfcraft-cli.exe', 'pdfcraft-hot-folder.exe') {
  Copy-Item -LiteralPath 'target/debug/examples/service_probe.exe' -Destination (Join-Path $stage $name)
}
foreach ($name in 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE') { Copy-Item -LiteralPath $name -Destination $stage }
Copy-Item -LiteralPath 'assets/app-icon/LICENSE-document.txt' -Destination $stage
Set-Content -LiteralPath "$stage/models/detection.rten" -Value 'synthetic installer fixture; never an OCR model'
Set-Content -LiteralPath "$stage/models/recognition.rten" -Value 'synthetic installer fixture; never an OCR model'
$msi = Join-Path $stage 'service-test.msi'
$upgrade = Join-Path $stage 'service-test-upgrade.msi'
$installedMsi = $msi
& wix extension add -g WixToolset.Util.wixext/5.0.2
if ($LASTEXITCODE -ne 0) { throw 'WiX extension installation failed' }
& wix build packaging/windows/pdfcraft.wxs packaging/windows/installer-ui.wxs -arch x64 -ext WixToolset.Util.wixext -d Version=0.6.0 -d "BinDir=$stage" -d "ModelsDir=$stage/models" -d 'IconPath=assets/app-icon/pdfcraft.ico' -o $msi
if ($LASTEXITCODE -ne 0) { throw 'WiX compilation failed' }
& pwsh -NoProfile -File packaging/windows/test-msi.ps1 $msi
if ($LASTEXITCODE -ne 0) { throw 'Compiled MSI validation failed' }
& wix build packaging/windows/pdfcraft.wxs packaging/windows/installer-ui.wxs -arch x64 -ext WixToolset.Util.wixext -d Version=0.6.1 -d "BinDir=$stage" -d "ModelsDir=$stage/models" -d 'IconPath=assets/app-icon/pdfcraft.ico' -o $upgrade
if ($LASTEXITCODE -ne 0) { throw 'Upgrade MSI compilation failed' }
function Invoke-Msi([string] $Options, [string] $Name) {
  $log = Join-Path $stage "$Name.log"
  $process = Start-Process -FilePath msiexec.exe -ArgumentList "$Options /qn /norestart /l*v `"$log`"" -Wait -PassThru -WindowStyle Hidden
  if ($process.ExitCode -notin @(0,3010)) {
    Select-String -LiteralPath $log -Pattern 'Return value 3|Error [0-9]|failed' -Context 12,4
    throw "MSI $Name failed: $($process.ExitCode)"
  }
}
$data = Join-Path $env:ProgramData 'PdfCraft/HotFolder'
try {
  Invoke-Msi "/i `"$msi`"" 'default'
  if (Get-Service PdfCraftHotFolder -ErrorAction SilentlyContinue) { throw 'Default install unexpectedly installed the service' }
  Invoke-Msi "/i `"$msi`" ADDLOCAL=HotFolder INSTALLHOTFOLDER=1" 'opt-in'
  $service = Get-CimInstance Win32_Service -Filter "Name='PdfCraftHotFolder'"
  if ($service.State -ne 'Running' -or $service.StartMode -ne 'Auto' -or $service.StartName -ne 'NT AUTHORITY\LocalService') {
    throw 'Installed service is not running automatically as LocalService'
  }
  for ($i = 0; $i -lt 40 -and -not (Test-Path -LiteralPath "$data/tasks.heartbeat"); $i++) { Start-Sleep -Milliseconds 250 }
  if (-not (Test-Path -LiteralPath "$data/tasks.heartbeat")) { throw 'Installed service cannot write its protected task directory' }
  Set-Content -LiteralPath "$data/keep-user-tasks.txt" -Value 'synthetic task preservation fixture'
  # No INSTALLHOTFOLDER override: a major upgrade must preserve the opt-in choice.
  Invoke-Msi "/i `"$upgrade`"" 'upgrade'
  $installedMsi = $upgrade
  $service = Get-CimInstance Win32_Service -Filter "Name='PdfCraftHotFolder'"
  if ($service.State -ne 'Running' -or $service.StartMode -ne 'Auto' -or $service.StartName -ne 'NT AUTHORITY\LocalService') {
    throw 'Upgrade did not preserve the optional automatic service and its account'
  }
  if (-not (Test-Path -LiteralPath "$data/keep-user-tasks.txt")) { throw 'Upgrade deleted task data' }
  Invoke-Msi "/i `"$installedMsi`" REMOVE=HotFolder" 'remove-companion'
  if (Get-Service PdfCraftHotFolder -ErrorAction SilentlyContinue) { throw 'Removing the component left its service installed' }
  if (-not (Test-Path -LiteralPath "$data/keep-user-tasks.txt")) { throw 'Removing the component deleted task data' }
} finally {
  Invoke-Msi "/x `"$installedMsi`"" 'uninstall'
}
