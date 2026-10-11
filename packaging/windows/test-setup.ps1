# Read the compiled Burn manifest and embedded MSI. Never execute Setup.
param([Parameter(Mandatory)] [string] $Setup, [Parameter(Mandatory)] [string] $Msi,
      [Parameter(Mandatory)] [string] $Work, [ValidateSet('x64', 'x86', 'arm64')] [string] $Arch)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'installer-languages.ps1')
if (Test-Path -LiteralPath $Work) { throw "Inspection directory must be new: $Work" }
& wix burn extract $Setup -o (Join-Path $Work 'payload') -oba (Join-Path $Work 'ba')
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect compiled Setup' }
[xml] $manifest = Get-Content (Join-Path $Work 'ba/manifest.xml') -Raw -Encoding utf8
$root = $manifest.DocumentElement
$packages = @($root.SelectNodes('//*[local-name()="MsiPackage"]'))
if ($packages.Count -ne 1) { throw 'Setup must contain exactly one MSI' }
$properties = @($packages[0].SelectNodes('*[local-name()="MsiProperty" and @Id="TRANSFORMS"]'))
if ($properties.Count -ne 1 -or $properties[0].Id -ne 'TRANSFORMS' -or $properties[0].Value -ne '[LanguageTransform]') { throw 'Missing native MSI language transform property' }
$searches = @($root.SelectNodes('//*[local-name()="SetVariable"]'))
if ($searches.Count -ne 6) { throw 'Expected six compiled language selectors' }
$map = @{}
foreach ($search in $searches) {
  if ($search.Variable -ne 'LanguageTransform' -or $search.Type -ne 'string') { throw 'Unexpected language variable' }
  $language = $search.Value.TrimStart(':').Replace('.mst', '')
  $condition = $search.Condition
  if ($condition -cne (Get-InstallerLanguageCondition $language)) { throw "Unexpected compiled selector for $language" }
  foreach ($match in [regex]::Matches($condition, 'UserUILanguageID = (\d+)')) {
    $id = [int] $match.Groups[1].Value
    if ($map.ContainsKey($id)) { throw "Ambiguous language $id" }
    $map[$id] = $language
  }
}
# Exhaustive verification includes every regional sublanguage, English and all
# unsupported primary languages. No OS preference is changed to run this check.
for ($id = 0; $id -le 65535; $id++) {
  $selected = if ($map.ContainsKey($id)) { $map[$id] } else { 'en' }
  if ($selected -cne (Get-InstallerLanguage $id)) { throw "Incorrect fallback for LANGID $id" }
}
foreach ($culture in @('de-AT', 'fr-CA', 'es-MX', 'nl-BE', 'it-CH', 'hu-HU', 'en-AU', 'ja-JP')) {
  $native = [Globalization.CultureInfo]::GetCultureInfo($culture)
  $expected = if ($InstallerLanguages.ContainsKey($native.TwoLetterISOLanguageName)) { $native.TwoLetterISOLanguageName } else { 'en' }
  if ((Get-InstallerLanguage $native.LCID) -ne $expected) { throw "Regional locale failure: $culture" }
}
$msiFiles = @(Get-ChildItem (Join-Path $Work 'payload') -Filter *.msi -Recurse)
if ($msiFiles.Count -ne 1 -or (Get-FileHash $msiFiles[0].FullName).Hash -ne (Get-FileHash $Msi).Hash) { throw 'Embedded MSI is not the validated offline payload' }
if ($root.SelectNodes('//*[@DownloadUrl]').Count -ne 0) { throw 'Setup must not download installation payloads' }
$bytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Setup).Path)
$pe = [BitConverter]::ToInt32($bytes, 0x3c)
$machine = [BitConverter]::ToUInt16($bytes, $pe + 4)
$expectedMachine = @{ x64 = 0x8664; x86 = 0x14c; arm64 = 0xaa64 }[$Arch]
if ($machine -ne $expectedMachine) { throw "Setup machine $machine does not match $Arch" }
Write-Output 'ok: offline Setup payload, native architecture, 65,536 LANGIDs and regional/fallback selection'
