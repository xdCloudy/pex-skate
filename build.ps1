$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$targetDir = Join-Path $repoRoot "target\wasm32-wasip2\release"
$outputDir = Join-Path $repoRoot "dist"
$componentPath = Join-Path $targetDir "pex_skate.wasm"

$javaCommand = Get-Command java -ErrorAction SilentlyContinue
if (-not $javaCommand) {
    throw "Java 25 or newer is required to build the Fabric client companion."
}
$javaVersionOutput = (& java -version 2>&1 | Out-String)
if ($javaVersionOutput -notmatch 'version "(\d+)(?:\.|")') {
    throw "Could not determine the Java version from: $javaVersionOutput"
}
if ([int]$Matches[1] -lt 25) {
    throw "Java 25 or newer is required to build the Fabric client companion. Detected Java $($Matches[1]). Set JAVA_HOME to a JDK 25+ installation."
}

Push-Location $repoRoot
try {
    cargo build --locked --release --target wasm32-wasip2
    if ($LASTEXITCODE -ne 0) { throw "WASM plugin build failed with exit code $LASTEXITCODE" }

    New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
    Copy-Item -LiteralPath $componentPath -Destination (Join-Path $outputDir "pex-skate.wasm") -Force

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

    Copy-Item -LiteralPath (Join-Path $repoRoot "release\INSTALL.md") -Destination (Join-Path $outputDir "INSTALL.md") -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot "LICENSE") -Destination (Join-Path $outputDir "LICENSE") -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot "vendor\NOTICE.md") -Destination (Join-Path $outputDir "NOTICE.md") -Force
    $bundledNotices = @{
        "LICENSE-PEX-API-APACHE" = "vendor\pumpkin-plugin-api\LICENSE-APACHE"
        "LICENSE-PEX-API-MIT" = "vendor\pumpkin-plugin-api\LICENSE-MIT"
        "LICENSE-PEX-WIT" = "vendor\pumpkin-plugin-wit\LICENSE-PEX"
        "LICENSE-SKATE-ENGINE" = "vendor\skate-engine\LICENSE"
        "NOTICE-SKATE-ENGINE" = "vendor\skate-engine\NOTICE-mashup"
    }
    foreach ($notice in $bundledNotices.GetEnumerator()) {
        Copy-Item -LiteralPath (Join-Path $repoRoot $notice.Value) -Destination (Join-Path $outputDir $notice.Key) -Force
    }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $bundlePath = Join-Path $outputDir "PEX-Skate-26.3.zip"
    if (Test-Path -LiteralPath $bundlePath) { Remove-Item -LiteralPath $bundlePath -Force }
    $archive = [System.IO.Compression.ZipFile]::Open($bundlePath, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($fileName in @("pex-skate.wasm", "pex-skate-client-26.3.jar", "SHA256SUMS", "INSTALL.md", "LICENSE", "NOTICE.md") + @($bundledNotices.Keys)) {
            [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
                $archive,
                (Join-Path $outputDir $fileName),
                $fileName,
                [System.IO.Compression.CompressionLevel]::Optimal
            ) | Out-Null
        }
    } finally {
        $archive.Dispose()
    }
    $expectedEntries = @("pex-skate.wasm", "pex-skate-client-26.3.jar", "SHA256SUMS", "INSTALL.md", "LICENSE", "NOTICE.md") + @($bundledNotices.Keys) | Sort-Object
    $checkArchive = [System.IO.Compression.ZipFile]::OpenRead($bundlePath)
    try {
        $actualEntries = @($checkArchive.Entries | ForEach-Object FullName | Sort-Object)
    } finally {
        $checkArchive.Dispose()
    }
    if (Compare-Object $expectedEntries $actualEntries) {
        throw "Portable bundle is missing files or contains unexpected paths."
    }
} finally {
    Pop-Location
}

Write-Host "PEX Skate release files written to dist\"
