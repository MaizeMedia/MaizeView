# Launch MaizeView for Playwright CDP testing (Windows / WebView2).
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$E2eDir = Join-Path $Root "e2e"
$DataDir = Join-Path $E2eDir ".data"
$EnvFile = Join-Path $E2eDir ".env"

New-Item -ItemType Directory -Force -Path $DataDir | Out-Null

if (Test-Path $EnvFile) {
  Get-Content $EnvFile | ForEach-Object {
    $line = $_.Trim()
    if (-not $line -or $line.StartsWith("#")) { return }
    $idx = $line.IndexOf("=")
    if ($idx -le 0) { return }
    $key = $line.Substring(0, $idx).Trim()
    $val = $line.Substring($idx + 1).Trim().Trim('"').Trim("'")
    if (-not (Test-Path "Env:$key")) { Set-Item -Path "Env:$key" -Value $val }
  }
}

if (-not $env:MAIZEVIEW_DB_PATH) {
  $env:MAIZEVIEW_DB_PATH = Join-Path $DataDir "maizeview.db"
}
if (-not $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS) {
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9222"
}

Write-Host "E2E DB: $($env:MAIZEVIEW_DB_PATH)"
Write-Host "CDP:    http://127.0.0.1:9222"
if ($env:MAIZEVIEW_TEST_LIB) {
  Write-Host "Sandbox: $($env:MAIZEVIEW_TEST_LIB)"
} else {
  Write-Host "Sandbox: (unset - copy e2e/.env.example to e2e/.env)"
}

Set-Location $Root
npm run tauri dev
