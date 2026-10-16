param(
    [Parameter(Mandatory)] [string] $Out
)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$repository = (Resolve-Path (Join-Path $here '..\..')).Path
$work = Join-Path ([IO.Path]::GetTempPath()) "skategm-convert-build"
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
$workFull = [IO.Path]::GetFullPath($work)
if (-not $workFull.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to remove build work outside the temporary directory: $workFull"
}
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $work | Out-Null
$sourceStage = Join-Path $work 'source'
New-Item -ItemType Directory -Force $sourceStage | Out-Null
Copy-Item -Path (Join-Path $here '*') -Destination $sourceStage -Recurse -Force
Get-ChildItem -LiteralPath $sourceStage -Directory -Filter '__pycache__' -Recurse | Remove-Item -Recurse -Force
Get-ChildItem -LiteralPath $sourceStage -File -Filter '*.pyc' -Recurse | Remove-Item -Force

Push-Location $repository
try {
    & cargo build --locked --release --bin pex-skate-map-check
    if ($LASTEXITCODE -ne 0) { throw 'PEX Skate map validator build failed' }
} finally {
    Pop-Location
}
$mapChecker = Join-Path $repository 'target\release\pex-skate-map-check.exe'
if (-not (Test-Path -LiteralPath $mapChecker -PathType Leaf)) {
    throw 'PEX Skate map validator executable was not created'
}

& rustc --edition 2024 --crate-type cdylib -C opt-level=3 -C panic=abort -C target-feature=+crt-static `
    "$here/tools/asset_pipeline/refpack_native.rs" -o "$work/refpack.dll"
if ($LASTEXITCODE -ne 0) { throw 'refpack.dll build failed' }

& python -m PyInstaller --noconfirm --clean --onefile --console --name skategm-convert `
    --paths $here `
    --hidden-import numpy --hidden-import PIL.Image `
    --add-binary "$work/refpack.dll;tools/asset_pipeline" `
    --add-binary "$mapChecker;tools" `
    --add-data "$(Join-Path $sourceStage 'tools');tools" `
    --exclude-module bpy --exclude-module mathutils --exclude-module tkinter `
    --copy-metadata numpy --copy-metadata Pillow `
    --distpath $Out --workpath "$work/build" --specpath "$work" `
    (Join-Path $here 'convert.py')
if ($LASTEXITCODE -ne 0) { throw 'PyInstaller failed' }

New-Item -ItemType Directory -Force -Path $Out | Out-Null
Compress-Archive -Path (Join-Path $sourceStage '*') -DestinationPath (Join-Path $Out 'skategm-exporter-source.zip') -Force

$licenses = Join-Path $Out 'licenses'
New-Item -ItemType Directory -Force $licenses | Out-Null
Copy-Item "$here/tools/vendor/utt/LICENSE" "$licenses/UTT.txt"
Copy-Item "$here/tools/vendor/university/LICENSE-PROJECT.md" "$licenses/CustomEngineLayer.txt"
Copy-Item "$here/tools/vendor/skate3_ui/LICENSE" "$licenses/skate3_ui.txt"
Copy-Item "$here/LICENSE-THIRD-PARTY.md" "$licenses/README.md"
Write-Host "built $Out/skategm-convert.exe"
