param(
  [string]$ExePath = "",
  [string]$ShortcutName = "ConnLens"
)

$repoRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($ExePath)) {
  $ExePath = Join-Path $repoRoot "src-tauri\target\release\connlens.exe"
}

$resolvedExe = Resolve-Path -LiteralPath $ExePath -ErrorAction Stop
$iconPath = Join-Path $repoRoot "src-tauri\icons\icon.ico"
$resolvedIcon = Resolve-Path -LiteralPath $iconPath -ErrorAction Stop
$programsDir = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
New-Item -ItemType Directory -Force -Path $programsDir | Out-Null

$shortcutPath = Join-Path $programsDir "$ShortcutName.lnk"
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($shortcutPath)
$shortcut.TargetPath = $resolvedExe.Path
$shortcut.WorkingDirectory = Split-Path -Parent $resolvedExe.Path
$shortcut.IconLocation = "$($resolvedIcon.Path),0"
$shortcut.Description = "ConnLens local developer connection visibility"
$shortcut.Save()

Write-Output "Registered Start Menu shortcut: $shortcutPath"
