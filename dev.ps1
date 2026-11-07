# Run the app in development mode (hot reload for the UI, rebuild for Rust).
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
Set-Location $PSScriptRoot
npm run tauri dev
