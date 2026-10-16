$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.Windows.Forms
$converter = Join-Path $PSScriptRoot "skategm-convert.exe"
if (-not (Test-Path -LiteralPath $converter -PathType Leaf)) {
    [System.Windows.Forms.MessageBox]::Show(
        "skategm-convert.exe must be beside this installer. Extract the full ISO setup ZIP before running it.",
        "PEX Skate data setup",
        [System.Windows.Forms.MessageBoxButtons]::OK,
        [System.Windows.Forms.MessageBoxIcon]::Error
    ) | Out-Null
    exit 2
}

$isoPicker = New-Object System.Windows.Forms.OpenFileDialog
$isoPicker.Title = "Select your Skate 3 Xbox 360 ISO"
$isoPicker.Filter = "Xbox 360 disc image (*.iso)|*.iso|All files (*.*)|*.*"
$isoPicker.CheckFileExists = $true
if ($isoPicker.ShowDialog() -ne [System.Windows.Forms.DialogResult]::OK) { exit 0 }
$isoPath = $isoPicker.FileName

$folderPicker = New-Object System.Windows.Forms.FolderBrowserDialog
$folderPicker.Description = "Select the PEX Skate plugin data folder (normally plugins/data/pex-skate)"
$folderPicker.UseDescriptionForTitle = $true
if ($folderPicker.ShowDialog() -ne [System.Windows.Forms.DialogResult]::OK) { exit 0 }
$dataPath = [System.IO.Path]::GetFullPath($folderPicker.SelectedPath)
$privatePath = Join-Path $dataPath "private"

if (Test-Path -LiteralPath $privatePath) {
    $answer = [System.Windows.Forms.MessageBox]::Show(
        "The selected PEX Skate data folder already has converted assets. Replace those assets with a fresh conversion? Other files in the folder will be kept.",
        "Replace converted assets?",
        [System.Windows.Forms.MessageBoxButtons]::YesNo,
        [System.Windows.Forms.MessageBoxIcon]::Warning
    )
    if ($answer -ne [System.Windows.Forms.DialogResult]::Yes) { exit 0 }
}

New-Item -ItemType Directory -Force -Path $dataPath | Out-Null
$stagePath = Join-Path ([System.IO.Path]::GetTempPath()) ("pex-skate-import-" + [guid]::NewGuid().ToString("N"))
$backupPath = Join-Path $dataPath ("private.previous-" + [guid]::NewGuid().ToString("N"))
try {
    Write-Host "Converting the selected ISO locally. This can take several minutes."
    & $converter --xex $isoPath --out $stagePath
    if ($LASTEXITCODE -ne 0) {
        throw "The Skate 3 converter exited with code $LASTEXITCODE. See the converter output above."
    }

    $required = @(
        "private/game.json",
        "private/stock/skater-collections.json",
        "private/stock/data/config/input.cfg",
        "private/skater.glb"
    )
    foreach ($relative in $required) {
        if (-not (Test-Path -LiteralPath (Join-Path $stagePath ("assets\" + $relative)) -PathType Leaf)) {
            throw "Conversion did not produce the required asset $relative. Nothing was installed."
        }
    }

    if (Test-Path -LiteralPath $privatePath) {
        Move-Item -LiteralPath $privatePath -Destination $backupPath
    }
    try {
        Move-Item -LiteralPath (Join-Path $stagePath "assets\private") -Destination $privatePath
    } catch {
        if (Test-Path -LiteralPath $backupPath) {
            Move-Item -LiteralPath $backupPath -Destination $privatePath
        }
        throw
    }
    Copy-Item -LiteralPath (Join-Path $stagePath "conversion.log") -Destination (Join-Path $dataPath "conversion.log") -Force
    if (Test-Path -LiteralPath $backupPath) {
        $resolvedBackup = (Resolve-Path -LiteralPath $backupPath).Path
        if ([System.IO.Path]::GetDirectoryName($resolvedBackup) -eq $dataPath) {
            Remove-Item -LiteralPath $resolvedBackup -Recurse -Force
        }
    }

    [System.Windows.Forms.MessageBox]::Show(
        "Skate 3 data is ready in:`r`n$dataPath`r`n`r`nRestart the PEX server before enabling Skate mode.",
        "PEX Skate data setup complete",
        [System.Windows.Forms.MessageBoxButtons]::OK,
        [System.Windows.Forms.MessageBoxIcon]::Information
    ) | Out-Null
} catch {
    [System.Windows.Forms.MessageBox]::Show(
        $_.Exception.Message,
        "PEX Skate data setup failed",
        [System.Windows.Forms.MessageBoxButtons]::OK,
        [System.Windows.Forms.MessageBoxIcon]::Error
    ) | Out-Null
    exit 1
} finally {
    if (Test-Path -LiteralPath $stagePath) {
        $resolvedStage = (Resolve-Path -LiteralPath $stagePath).Path
        $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
        if ($resolvedStage.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $resolvedStage -Recurse -Force
        }
    }
}
