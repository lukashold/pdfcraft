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
& wix build packaging/windows/pdfcraft.wxs packaging/windows/installer-ui.wxs -arch x64 -d Version=0.6.0 -d "BinDir=$stage" -d "ModelsDir=$stage/models" -d 'IconPath=assets/app-icon/pdfcraft.ico' -o $msi
if ($LASTEXITCODE -ne 0) { throw 'WiX compilation failed' }
& pwsh -NoProfile -File packaging/windows/test-msi.ps1 $msi
if ($LASTEXITCODE -ne 0) { throw 'Compiled MSI validation failed' }
function Invoke-Msi([string] $Options, [string] $Name) {
  $log = Join-Path $stage "$Name.log"
  $process = Start-Process -FilePath msiexec.exe -ArgumentList "$Options /qn /norestart /l*v `"$log`"" -Wait -PassThru -WindowStyle Hidden
  if ($process.ExitCode -notin @(0,3010)) {
    Get-Content -LiteralPath $log -Tail 100
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
  Invoke-Msi "/i `"$msi`" REMOVE=HotFolder" 'remove-companion'
  if (Get-Service PdfCraftHotFolder -ErrorAction SilentlyContinue) { throw 'Removing the component left its service installed' }
  if (-not (Test-Path -LiteralPath "$data/keep-user-tasks.txt")) { throw 'Removing the component deleted task data' }
} finally {
  Invoke-Msi "/x `"$msi`"" 'uninstall'
}
