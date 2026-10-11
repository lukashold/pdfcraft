# Safe local checks only. Never run the installer acceptance body here.
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/common.ps1"
foreach ($file in Get-ChildItem -LiteralPath $PSScriptRoot -Filter '*.ps1') {
    $tokens = $null; $errors = $null
    [void] [Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref] $tokens, [ref] $errors)
    Assert-That (-not $errors) ("PowerShell syntax: " + ($errors -join '; '))
}
$boundary = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
[void] (Assert-SafePath (Join-Path $boundary 'packaging/server-tests') $boundary)
$rejected = $false
try { [void] (Assert-SafePath ($boundary + '-sibling/file') $boundary) } catch { $rejected = $true }
Assert-That $rejected 'Sibling-prefix confinement must reject paths outside the test root.'

$keys = @('GITHUB_EVENT_NAME', 'INSTALLER_RUN_ID', 'EXPECTED_SHA', 'GITHUB_SHA', 'INCLUDE_2019',
    'CONFIRM_2019', 'ARTIFACT_NAME', 'GITHUB_API_URL', 'GITHUB_REPOSITORY', 'GITHUB_OUTPUT', 'GH_TOKEN')
$saved = @{}
foreach ($key in $keys) { $saved[$key] = [Environment]::GetEnvironmentVariable($key) }
$output = [IO.Path]::GetTempFileName()
$previousCase = Get-Variable -Name PdfcraftServerProvenanceCase -Scope Global -ErrorAction SilentlyContinue
try {
    $env:GITHUB_EVENT_NAME = 'workflow_dispatch'; $env:INSTALLER_RUN_ID = '123'
    $env:EXPECTED_SHA = 'a' * 40; $env:GITHUB_SHA = $env:EXPECTED_SHA
    $env:INCLUDE_2019 = 'false'; $env:CONFIRM_2019 = ''
    $env:ARTIFACT_NAME = 'pdfcraft-preview-windows-x64'
    $env:GITHUB_API_URL = 'https://api.invalid'; $env:GITHUB_REPOSITORY = 'test/pdfcraft'
    $env:GITHUB_OUTPUT = $output; $env:GH_TOKEN = 'synthetic-not-a-token'
    $global:PdfcraftServerProvenanceCase = 'good'
    function Invoke-RestMethod($Uri, $Headers) {
        if ($Uri -like '*/artifacts?*') {
            $sha = if ($global:PdfcraftServerProvenanceCase -eq 'artifact-sha') { 'b' * 40 } else { $env:EXPECTED_SHA }
            return @{ total_count = 1; artifacts = @(@{ name = $env:ARTIFACT_NAME; expired = $false; id = 42; workflow_run = @{ head_sha = $sha } }) }
        }
        if ($Uri -like '*/workflows/*') {
            return @{ path = if ($global:PdfcraftServerProvenanceCase -eq 'workflow') { '.github/workflows/other.yml' } else { '.github/workflows/preview-installers.yml' } }
        }
        return @{
            status = 'completed'; conclusion = if ($global:PdfcraftServerProvenanceCase -eq 'failed') { 'failure' } else { 'success' }
            head_sha = $env:EXPECTED_SHA; workflow_id = 7
            head_repository = @{ full_name = if ($global:PdfcraftServerProvenanceCase -eq 'fork') { 'other/pdfcraft' } else { $env:GITHUB_REPOSITORY } }
            event = if ($global:PdfcraftServerProvenanceCase -eq 'pull-request') { 'pull_request' } else { 'workflow_dispatch' }
        }
    }
    & "$PSScriptRoot/verify-provenance.ps1" | Out-Null
    Assert-That ((Get-Content -LiteralPath $output -Raw) -match 'artifact_id=42') 'Matching trusted artifact should pass.'
    foreach ($invalid in @('failed', 'fork', 'workflow', 'pull-request', 'artifact-sha')) {
        $global:PdfcraftServerProvenanceCase = $invalid; $rejected = $false
        try { & "$PSScriptRoot/verify-provenance.ps1" | Out-Null } catch { $rejected = $true }
        Assert-That $rejected "Unsafe artifact accepted: $invalid"
    }
    $global:PdfcraftServerProvenanceCase = 'good'; $env:INCLUDE_2019 = 'true'; $rejected = $false
    try { & "$PSScriptRoot/verify-provenance.ps1" | Out-Null } catch { $rejected = $true }
    Assert-That $rejected 'Unconfirmed Server 2019 dispatch must fail.'
    $env:INCLUDE_2019 = 'false'; $env:GITHUB_EVENT_NAME = 'pull_request'; $rejected = $false
    try { & "$PSScriptRoot/verify-provenance.ps1" | Out-Null } catch { $rejected = $true }
    Assert-That $rejected 'Automatic parent event must fail.'
} finally {
    Remove-Item -LiteralPath $output -Force
    foreach ($key in $keys) { [Environment]::SetEnvironmentVariable($key, $saved[$key]) }
    Remove-Item Function:Invoke-RestMethod
    if ($previousCase) { $global:PdfcraftServerProvenanceCase = $previousCase.Value }
    else { Remove-Variable -Name PdfcraftServerProvenanceCase -Scope Global }
}
Write-Output 'ok: parser, safe-path confinement and eight provenance scenarios; no service or installer executed'
