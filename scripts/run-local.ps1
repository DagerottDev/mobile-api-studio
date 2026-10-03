$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot '..')
try {
    npm run build --prefix apps/desktop
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    cargo build -p mobile-api-studio-script-worker
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    cargo run -p mobile-api-studio-server -- @args
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
