param(
    [Parameter(Mandatory)] [string] $Out
)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$work = Join-Path ([IO.Path]::GetTempPath()) "skategm-convert-build"
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $work | Out-Null

& rustc --edition 2024 --crate-type cdylib -C opt-level=3 -C panic=abort -C target-feature=+crt-static `
    "$here/tools/asset_pipeline/refpack_native.rs" -o "$work/refpack.dll"
if ($LASTEXITCODE -ne 0) { throw 'refpack.dll build failed' }

& python -m PyInstaller --noconfirm --clean --onefile --console --name skategm-convert `
    --paths $here `
    --hidden-import numpy --hidden-import PIL.Image `
    --add-binary "$work/refpack.dll;tools/asset_pipeline" `
    --add-data "$here/tools;tools" `
    --exclude-module bpy --exclude-module mathutils --exclude-module tkinter `
    --copy-metadata numpy --copy-metadata Pillow `
    --distpath $Out --workpath "$work/build" --specpath "$work" `
    (Join-Path $here 'convert.py')
if ($LASTEXITCODE -ne 0) { throw 'PyInstaller failed' }

$licenses = Join-Path $Out 'licenses'
New-Item -ItemType Directory -Force $licenses | Out-Null
Copy-Item "$here/tools/vendor/utt/LICENSE" "$licenses/UTT.txt"
Copy-Item "$here/tools/vendor/university/LICENSE-PROJECT.md" "$licenses/CustomEngineLayer.txt"
Copy-Item "$here/tools/vendor/skate3_ui/LICENSE" "$licenses/skate3_ui.txt"
Copy-Item "$here/../LICENSE-THIRD-PARTY.md" "$licenses/README.md"
Write-Host "built $Out/skategm-convert.exe"
