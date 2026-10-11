# Shared build-time language metadata. Selection uses UI language, never formatting locale.
$InstallerLanguages = Get-Content (Join-Path $PSScriptRoot '../installer-languages.json') -Raw -Encoding utf8 | ConvertFrom-Json -AsHashtable

function Get-InstallerLanguage([int] $LanguageId) {
  $primary = $LanguageId -band 0x3ff
  foreach ($entry in $InstallerLanguages.GetEnumerator()) {
    if ($entry.Value.primary -eq $primary) { return $entry.Key }
  }
  return 'en'
}

function Get-InstallerLanguageCondition([string] $Language) {
  # A LANGID has ten primary-language bits and six sublanguage bits. Burn conditions
  # have no arithmetic/bitwise operator, so enumerate those 64 possible LANGIDs.
  # This also covers regional variants not explicitly listed by .NET CultureInfo.
  $primary = $InstallerLanguages[$Language].primary
  if (-not $primary) { throw "Unknown installer language: $Language" }
  return ((0..63 | ForEach-Object { "UserUILanguageID = $($primary + 1024 * $_)" }) -join ' OR ')
}

function Get-InstallerStrings([string] $Language) {
  if (-not $InstallerLanguages.ContainsKey($Language)) { throw "Unknown installer language: $Language" }
  [xml] $xml = Get-Content (Join-Path $PSScriptRoot "locales/$Language.wxl") -Raw -Encoding utf8
  $strings = @{}
  foreach ($item in $xml.WixLocalization.String) { $strings[$item.Id] = $item.Value }
  return $strings
}
