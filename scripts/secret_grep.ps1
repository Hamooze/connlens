param(
  [Parameter(Mandatory = $false)]
  [string]$Root = (Join-Path $PSScriptRoot ".."),
  [Parameter(Mandatory = $false)]
  [string]$SecretsFile = "tests/fixtures/raw-secret-values.txt"
)

$ErrorActionPreference = "Stop"
& node (Join-Path $PSScriptRoot "secret-grep.mjs") $Root $SecretsFile
if ($LASTEXITCODE -ne 0) {
  throw "secret_grep failed (exit $LASTEXITCODE)"
}
