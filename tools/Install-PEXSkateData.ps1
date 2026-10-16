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
$folderPicker.Description = "Select the server PEX Skate data folder, or cancel for client-only model setup."
$folderPicker.UseDescriptionForTitle = $true
$dataPath = if ($folderPicker.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
    [System.IO.Path]::GetFullPath($folderPicker.SelectedPath)
} else {
    $null
}
$privatePath = if ($dataPath) { Join-Path $dataPath "private" } else { $null }

$clientConfigPath = $null
$clientFolderPicker = New-Object System.Windows.Forms.FolderBrowserDialog
$clientFolderPicker.Description = "Optional: select this Minecraft instance's config folder to install the local skater model. Cancel to skip."
$clientFolderPicker.UseDescriptionForTitle = $true
$defaultClientConfig = Join-Path $env:APPDATA ".minecraft\config"
if (Test-Path -LiteralPath $defaultClientConfig -PathType Container) {
    $clientFolderPicker.SelectedPath = $defaultClientConfig
}
if ($clientFolderPicker.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
    $clientConfigPath = [System.IO.Path]::GetFullPath($clientFolderPicker.SelectedPath)
}
if (-not $dataPath -and -not $clientConfigPath) {
    [System.Windows.Forms.MessageBox]::Show(
        "Select a server data folder or a local Minecraft config folder.",
        "PEX Skate data setup",
        [System.Windows.Forms.MessageBoxButtons]::OK,
        [System.Windows.Forms.MessageBoxIcon]::Information
    ) | Out-Null
    exit 0
}
$clientModelPath = if ($clientConfigPath) {
    Join-Path $clientConfigPath "pex-skate\private\skater.glb"
} else {
    $null
}

if ($privatePath -and (Test-Path -LiteralPath $privatePath)) {
    $answer = [System.Windows.Forms.MessageBox]::Show(
        "The selected PEX Skate data folder already has converted assets. Replace those assets with a fresh conversion? Other files in the folder will be kept.",
        "Replace converted assets?",
        [System.Windows.Forms.MessageBoxButtons]::YesNo,
        [System.Windows.Forms.MessageBoxIcon]::Warning
    )
    if ($answer -ne [System.Windows.Forms.DialogResult]::Yes) { exit 0 }
}

$stagePath = Join-Path ([System.IO.Path]::GetTempPath()) ("pex-skate-import-" + [guid]::NewGuid().ToString("N"))
$backupPath = $null
if ($dataPath) {
    New-Item -ItemType Directory -Force -Path $dataPath | Out-Null
    $backupPath = Join-Path $dataPath ("private.previous-" + [guid]::NewGuid().ToString("N"))
}
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

    $serverDataMessage = "Server data not installed (client-only setup)."
    $modelSourcePath = Join-Path $stagePath "assets\private\skater.glb"
    if ($dataPath) {
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
        $serverDataMessage = "Server data installed: $dataPath"
        $modelSourcePath = Join-Path $privatePath "skater.glb"
    }

    $clientModelMessage = "Client model not installed. To use the Skate 3 character mesh, run this setup on each client and choose its Minecraft config folder."
    if ($clientModelPath) {
        $replaceClientModel = $true
        if (Test-Path -LiteralPath $clientModelPath -PathType Leaf) {
            $answer = [System.Windows.Forms.MessageBox]::Show(
                "Replace the existing local PEX Skate model?",
                "Replace client model?",
                [System.Windows.Forms.MessageBoxButtons]::YesNo,
                [System.Windows.Forms.MessageBoxIcon]::Warning
            )
            $replaceClientModel = $answer -eq [System.Windows.Forms.DialogResult]::Yes
        }
        if ($replaceClientModel) {
            try {
                $clientModelDirectory = Split-Path -Parent $clientModelPath
                New-Item -ItemType Directory -Force -Path $clientModelDirectory | Out-Null
                $temporaryModelPath = $null
                $temporaryModelPath = Join-Path $clientModelDirectory (".skater-" + [guid]::NewGuid().ToString("N") + ".tmp")
                Copy-Item -LiteralPath $modelSourcePath -Destination $temporaryModelPath
                Move-Item -LiteralPath $temporaryModelPath -Destination $clientModelPath -Force
                $clientModelMessage = "Client model installed: $clientModelPath"
            } catch {
                $clientModelMessage = "Converted data is ready, but the client model copy failed: $($_.Exception.Message)"
            } finally {
                if ($temporaryModelPath -and (Test-Path -LiteralPath $temporaryModelPath)) {
                    Remove-Item -LiteralPath $temporaryModelPath -Force
                }
            }
        } else {
            $clientModelMessage = "Existing client model kept."
        }
    }

    $restartMessage = if ($dataPath) { "Restart the PEX server before enabling Skate mode." } else { "Restart Minecraft before using the new local model." }
    [System.Windows.Forms.MessageBox]::Show(
        "$serverDataMessage`r`n$clientModelMessage`r`n`r`n$restartMessage",
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
