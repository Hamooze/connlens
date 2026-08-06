param(
  [Parameter(Mandatory = $false)]
  [string]$Root = ".",
  [Parameter(Mandatory = $false)]
  [string]$SecretsFile = "tests/fixtures/raw-secret-values.txt"
)

$ErrorActionPreference = "Stop"
$rootPath = Resolve-Path -LiteralPath $Root
$secretPath = Resolve-Path -LiteralPath $SecretsFile
$failures = @()

foreach ($secret in Get-Content -LiteralPath $secretPath) {
  if ([string]::IsNullOrWhiteSpace($secret)) {
    continue
  }
  $matches = rg -n -F --glob '!tests/fixtures/**' --glob '!node_modules/**' --glob '!src-tauri/target/**' --glob '!dist/**' --glob '!package-lock.json' -- $secret $rootPath 2>$null
  if ($LASTEXITCODE -eq 0 -and $matches) {
    $failures += $matches
  }
}

if ($failures.Count -gt 0) {
  Write-Error "Raw fixture secret values were found outside allowed fixture files:`n$($failures -join "`n")"
}

Write-Output "secret_grep: pass"
