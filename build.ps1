$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$targetDir = Join-Path $repoRoot "target\wasm32-wasip1\release"
$outputDir = Join-Path $repoRoot "dist"
$componentPath = Join-Path $targetDir "pex_skate.wasm"

if (-not (Get-Command cargo-component -ErrorAction SilentlyContinue)) {
    throw "cargo-component is required. Install it with: cargo install cargo-component"
}

Push-Location $repoRoot
try {
    cargo component build --release --target wasm32-wasip2
    if ($LASTEXITCODE -ne 0) { throw "cargo component build failed with exit code $LASTEXITCODE" }

    New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
    Copy-Item -LiteralPath $componentPath -Destination (Join-Path $outputDir "pex-skate.wasm") -Force

    $javaCommand = Get-Command java -ErrorAction SilentlyContinue
    if (-not $javaCommand) {
        throw "Java 25 or newer is required to build the Fabric client companion."
    }
    Push-Location (Join-Path $repoRoot "client")
    try {
        .\gradlew.bat build
        if ($LASTEXITCODE -ne 0) { throw "Fabric client build failed with exit code $LASTEXITCODE" }
    } finally {
        Pop-Location
    }
    Copy-Item -LiteralPath (Join-Path $repoRoot "client\build\libs\pex-skate-client-0.1.0-alpha.1.jar") -Destination (Join-Path $outputDir "pex-skate-client-26.3.jar") -Force
} finally {
    Pop-Location
}

Write-Host "PEX plugin and Fabric client companion written to dist\"
