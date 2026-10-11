$ErrorActionPreference = 'Stop'

function Assert-That([bool] $Condition, [string] $Message) {
    if (-not $Condition) { throw $Message }
}

function Assert-SourceUnchanged([string] $Path, [string] $ExpectedHash, [string] $Context) {
    Assert-That (Test-Path -LiteralPath $Path -PathType Leaf) "$Context removed the original source."
    Assert-That ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -eq $ExpectedHash) "$Context changed the original source bytes."
}

function Write-Json([string] $Path, $Value) {
    # The CLI requires a JSON array even when a script has zero or one step.
    [IO.File]::WriteAllText($Path, (ConvertTo-Json -InputObject $Value -Depth 40), [Text.UTF8Encoding]::new($false))
}

function Assert-SafePath([string] $Path, [string] $AllowedRoot) {
    $absolute = [IO.Path]::GetFullPath($Path)
    $boundary = [IO.Path]::GetFullPath($AllowedRoot).TrimEnd('\')
    Assert-That ($absolute -eq $boundary -or $absolute.StartsWith($boundary + '\', [StringComparison]::OrdinalIgnoreCase)) "Unsafe test path: $absolute"
    $cursor = $absolute
    while ($cursor -and $cursor.Length -ge $boundary.Length) {
        if (Test-Path -LiteralPath $cursor) {
            Assert-That (-not ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) "Test path traverses a reparse point: $cursor"
        }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
    return $absolute
}

function Invoke-Program([string] $Exe, [string[]] $Arguments, [string] $Name, [int] $TimeoutSeconds = 120, [int[]] $SuccessCodes = @(0)) {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Exe
    $start.WorkingDirectory = $script:TestRoot
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardOutputEncoding = [Text.Encoding]::UTF8
    $start.StandardErrorEncoding = [Text.Encoding]::UTF8
    foreach ($argument in $Arguments) { [void] $start.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::Start($start)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
            $process.Kill($true)
            throw "$Name exceeded $TimeoutSeconds seconds"
        }
        $output = $stdout.GetAwaiter().GetResult()
        $errorText = $stderr.GetAwaiter().GetResult()
        [IO.File]::WriteAllText((Join-Path $script:Results "$Name.log"), $output + $errorText, [Text.UTF8Encoding]::new($false))
        Assert-That ($process.ExitCode -in $SuccessCodes) "$Name exited with code $($process.ExitCode); see $Name.log"
        return $output
    } finally { $process.Dispose() }
}

function Wait-For([string] $What, [scriptblock] $Predicate, [int] $Seconds = 120) {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $lastError = ''
    while ($timer.Elapsed.TotalSeconds -lt $Seconds) {
        try { if (& $Predicate) { return } } catch { $lastError = $_.Exception.Message }
        Start-Sleep -Milliseconds 500
    }
    throw "Timed out waiting for $What ($Seconds seconds). $lastError"
}

function Get-HotService {
    Get-CimInstance Win32_Service -Filter "Name='PdfCraftHotFolder'"
}

function Stop-HotService([int] $Seconds = 35) {
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $service = Get-HotService
    if ($service -and $service.State -ne 'Stopped') {
        $request = Invoke-CimMethod -InputObject $service -MethodName StopService
        Assert-That ($request.ReturnValue -in @(0, 5, 6)) "Service stop request failed: $($request.ReturnValue)"
        Wait-For 'service stopped' { (Get-HotService).State -eq 'Stopped' } $Seconds
    }
    return [Math]::Round($watch.Elapsed.TotalSeconds, 3)
}

function Start-HotService {
    $request = Invoke-CimMethod -InputObject (Get-HotService) -MethodName StartService
    Assert-That ($request.ReturnValue -in @(0, 10)) "Service start request failed: $($request.ReturnValue)"
    Wait-For 'service running' { (Get-HotService).State -eq 'Running' } 40
}

function Invoke-CliScript([string] $Name, [array] $Steps) {
    $path = Join-Path $script:TestRoot "$Name.json"
    Write-Json $path $Steps
    Invoke-Program $script:Cli @('run', '--script', $path) $Name 180
}

function Invoke-Tool([string] $Name, [string] $Tool, $Arguments) {
    $raw = Invoke-CliScript $Name @(@{ tool = $Tool; args = $Arguments })
    return $raw | ConvertFrom-Json
}

function New-Task([string] $Id, [string] $InputFolder, [string] $OutputFolder, [string] $Originals = 'keep') {
    return @{
        id = $Id; name = $Id; input = $InputFolder; output = $OutputFolder
        enabled = $true; interval_seconds = 1; settle_seconds = 1; originals = $Originals
        settings = @{ language = 'en'; recognize = $true; dpi = 150; auto_rotate = $false; deskew = $false; auto_crop = $false }
        output_plan = @{ steps = @(@{ format = 'pdf' }, @{ format = 'txt' }) }
    }
}

function Save-Tasks([array] $Tasks, [string] $Label) {
    [void] (Invoke-Tool $Label 'hot_folder_configure' @{ config = $script:Config; settings = @{ tasks = $Tasks } })
}

function Read-State {
    Get-Content -LiteralPath ([IO.Path]::ChangeExtension($script:Config, 'state.json')) -Raw | ConvertFrom-Json
}

function Wait-Output([string] $Directory, [string] $Stem, [int] $Seconds = 180) {
    Wait-For "searchable output $Stem" {
        $text = Join-Path $Directory "$Stem.txt"
        (Test-Path -LiteralPath (Join-Path $Directory "$Stem.pdf")) -and (Test-Path -LiteralPath $text) -and
            ((Get-Content -LiteralPath $text -Raw) -match 'searchable output')
    } $Seconds
}

function Set-PrivateDirectory([string] $Path, [string[]] $ExtraSids = @()) {
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetAccessRuleProtection($true, $false)
    foreach ($id in @('S-1-5-18', 'S-1-5-32-544') + $ExtraSids) {
        $sid = [Security.Principal.SecurityIdentifier]::new($id)
        $rule = [Security.AccessControl.FileSystemAccessRule]::new($sid, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
        $acl.AddAccessRule($rule)
    }
    Set-Acl -LiteralPath $Path -AclObject $acl
}

function Get-ProductCode([string] $Msi) {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $database = $installer.OpenDatabase($Msi, 0)
    try {
        $view = $database.OpenView('SELECT `Value` FROM `Property` WHERE `Property` = ''ProductCode''')
        [void] $view.Execute()
        return $view.Fetch().StringData(1)
    } finally {
        if ($view) { [void] $view.Close() }
        [void] [Runtime.InteropServices.Marshal]::FinalReleaseComObject($database)
        [void] [Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer)
    }
}

function New-UpgradeFixture([string] $Msi, [string] $Destination) {
    # Identical real packaged binaries; changed MSI identities exercise a genuine
    # same-version MajorUpgrade, not REINSTALL/repair. Never publish this fixture.
    Copy-Item -LiteralPath $Msi -Destination $Destination
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $database = $installer.OpenDatabase($Destination, 1)
    try {
        $product = '{' + [guid]::NewGuid().ToString().ToUpperInvariant() + '}'
        $view = $database.OpenView('UPDATE `Property` SET `Value` = ''' + $product + ''' WHERE `Property` = ''ProductCode''')
        [void] $view.Execute()
        [void] $view.Close()
        $summary = $database.SummaryInformation(20)
        $summary.Property(9) = '{' + [guid]::NewGuid().ToString().ToUpperInvariant() + '}'
        $summary.Persist()
        $database.Commit()
        return $product
    } finally {
        if ($summary) { [void] [Runtime.InteropServices.Marshal]::FinalReleaseComObject($summary) }
        [void] [Runtime.InteropServices.Marshal]::FinalReleaseComObject($database)
        [void] [Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer)
    }
}
