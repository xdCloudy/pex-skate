$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$targetDir = Join-Path $repoRoot "target\wasm32-wasip2\release"
$outputDir = Join-Path $repoRoot "dist"
$componentPath = Join-Path $targetDir "pex_skate.wasm"

Push-Location $repoRoot
try {
    cargo build --locked --release --target wasm32-wasip2
    if ($LASTEXITCODE -ne 0) { throw "WASM plugin build failed with exit code $LASTEXITCODE" }

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
    $clientJar = Get-ChildItem -LiteralPath (Join-Path $repoRoot "client\build\libs") -Filter "pex-skate-client-*.jar" |
        Where-Object { $_.Name -notlike "*-sources.jar" } |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1
    if (-not $clientJar) { throw "Fabric client build did not produce a client jar." }
    Copy-Item -LiteralPath $clientJar.FullName -Destination (Join-Path $outputDir "pex-skate-client-26.3.jar") -Force
    $checksums = @("dist/pex-skate.wasm", "dist/pex-skate-client-26.3.jar") | ForEach-Object {
        $file = Join-Path $repoRoot $_
        $hash = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $([System.IO.Path]::GetFileName($file))"
    }
    Set-Content -LiteralPath (Join-Path $outputDir "SHA256SUMS") -Value $checksums -Encoding ascii
} finally {
    Pop-Location
}

Write-Host "PEX plugin and Fabric client companion written to dist\"
