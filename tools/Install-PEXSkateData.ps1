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
$mapsPath = if ($dataPath) { Join-Path $dataPath "maps" } else { $null }

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
$clientMapsPath = if ($clientConfigPath) {
    Join-Path $clientConfigPath "pex-skate\maps"
} else {
    $null
}
$clientMapsManifestPath = if ($clientConfigPath) {
    Join-Path $clientConfigPath "pex-skate\maps.json"
} else {
    $null
}
$clientPropsPath = if ($clientConfigPath) {
    Join-Path $clientConfigPath "pex-skate\props"
} else {
    $null
}

if ($dataPath -and ((Test-Path -LiteralPath $privatePath) -or (Test-Path -LiteralPath $mapsPath))) {
    $answer = [System.Windows.Forms.MessageBox]::Show(
        "The selected PEX Skate data folder already has converted assets or maps. Replace them with a fresh conversion? Other files in the folder will be kept.",
        "Replace converted assets?",
        [System.Windows.Forms.MessageBoxButtons]::YesNo,
        [System.Windows.Forms.MessageBoxIcon]::Warning
    )
    if ($answer -ne [System.Windows.Forms.DialogResult]::Yes) { exit 0 }
}

$stagePath = Join-Path ([System.IO.Path]::GetTempPath()) ("pex-skate-import-" + [guid]::NewGuid().ToString("N"))
$backupPath = $null
$mapsManifestBackupPath = $null
if ($dataPath) {
    New-Item -ItemType Directory -Force -Path $dataPath | Out-Null
    $backupPath = Join-Path $dataPath ("private.previous-" + [guid]::NewGuid().ToString("N"))
    $mapsBackupPath = Join-Path $dataPath ("maps.previous-" + [guid]::NewGuid().ToString("N"))
    $mapsManifestBackupPath = Join-Path $dataPath ("maps.json.previous-" + [guid]::NewGuid().ToString("N"))
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
    $mapManifestPath = Join-Path $stagePath "maps.json"
    if (-not (Test-Path -LiteralPath $mapManifestPath -PathType Leaf)) {
        throw "Conversion did not produce maps.json. Nothing was installed."
    }
    $mapEntries = @(Get-Content -LiteralPath $mapManifestPath -Raw | ConvertFrom-Json)
    if ($mapEntries.Count -eq 0) {
        throw "Conversion did not produce any playable Skate map packages. Nothing was installed."
    }
    foreach ($entry in $mapEntries) {
        if ($entry.path -notmatch '^maps/[A-Za-z0-9._-]+\.skate$') {
            throw "Conversion produced an unsafe map path: $($entry.path)"
        }
        $relativeMap = $entry.path.Replace('/', [IO.Path]::DirectorySeparatorChar)
        $sourceMap = Join-Path $stagePath $relativeMap
        if (-not (Test-Path -LiteralPath $sourceMap -PathType Leaf)) {
            throw "Conversion did not produce $($entry.path). Nothing was installed."
        }
        $actualHash = (Get-FileHash -LiteralPath $sourceMap -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actualHash -ne $entry.sha256) {
            throw "Converted map hash does not match maps.json: $($entry.path)"
        }
        if ($entry.props) {
            if ($entry.props.path -notmatch '^props/[A-Za-z0-9._-]+\.skate$' -or
                $entry.props.path -ne ("props/" + [System.IO.Path]::GetFileName($entry.path)) -or
                $entry.props.sha256 -notmatch '^[A-Fa-f0-9]{64}$') {
                throw "Conversion produced invalid authored prop metadata for $($entry.path)."
            }
            $propsFileName = [System.IO.Path]::GetFileName($entry.props.path)
            $sourceProps = Join-Path (Join-Path $stagePath "assets\private\native-props") $propsFileName
            if (-not (Test-Path -LiteralPath $sourceProps -PathType Leaf)) {
                throw "Conversion did not produce $($entry.props.path). Nothing was installed."
            }
            $propsHash = (Get-FileHash -LiteralPath $sourceProps -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($propsHash -ne $entry.props.sha256.ToLowerInvariant()) {
                throw "Converted prop hash does not match maps.json: $($entry.props.path)"
            }
        }
    }

    $serverDataMessage = "Server data not installed (client-only setup)."
    $modelSourcePath = Join-Path $stagePath "assets\private\skater.glb"
    $propsSourcePath = Join-Path $stagePath "assets\private\native-props"
    if ($dataPath) {
        $hadPrivate = Test-Path -LiteralPath $privatePath
        $hadMaps = Test-Path -LiteralPath $mapsPath
        $mapsManifestPath = Join-Path $dataPath "maps.json"
        $hadMapsManifest = Test-Path -LiteralPath $mapsManifestPath
        try {
            if ($hadPrivate) { Move-Item -LiteralPath $privatePath -Destination $backupPath }
            if ($hadMaps) { Move-Item -LiteralPath $mapsPath -Destination $mapsBackupPath }
            if ($hadMapsManifest) { Move-Item -LiteralPath $mapsManifestPath -Destination $mapsManifestBackupPath }
            Move-Item -LiteralPath (Join-Path $stagePath "assets\private") -Destination $privatePath
            Move-Item -LiteralPath (Join-Path $stagePath "maps") -Destination $mapsPath
            Move-Item -LiteralPath $mapManifestPath -Destination $mapsManifestPath
            Copy-Item -LiteralPath (Join-Path $stagePath "conversion.log") -Destination (Join-Path $dataPath "conversion.log") -Force
        } catch {
            if (Test-Path -LiteralPath $privatePath) { Remove-Item -LiteralPath $privatePath -Recurse -Force }
            if (Test-Path -LiteralPath $mapsPath) { Remove-Item -LiteralPath $mapsPath -Recurse -Force }
            if (Test-Path -LiteralPath $mapsManifestPath) { Remove-Item -LiteralPath $mapsManifestPath -Force }
            if (Test-Path -LiteralPath $backupPath) { Move-Item -LiteralPath $backupPath -Destination $privatePath }
            if (Test-Path -LiteralPath $mapsBackupPath) { Move-Item -LiteralPath $mapsBackupPath -Destination $mapsPath }
            if (Test-Path -LiteralPath $mapsManifestBackupPath) { Move-Item -LiteralPath $mapsManifestBackupPath -Destination $mapsManifestPath }
            throw
        }
        if (Test-Path -LiteralPath $backupPath) {
            $resolvedBackup = (Resolve-Path -LiteralPath $backupPath).Path
            if ([System.IO.Path]::GetDirectoryName($resolvedBackup) -eq $dataPath) {
                Remove-Item -LiteralPath $resolvedBackup -Recurse -Force
            }
        }
        if (Test-Path -LiteralPath $mapsBackupPath) {
            $resolvedMapsBackup = (Resolve-Path -LiteralPath $mapsBackupPath).Path
            if ([System.IO.Path]::GetDirectoryName($resolvedMapsBackup) -eq $dataPath) {
                Remove-Item -LiteralPath $resolvedMapsBackup -Recurse -Force
            }
        }
        if (Test-Path -LiteralPath $mapsManifestBackupPath) {
            Remove-Item -LiteralPath $mapsManifestBackupPath -Force
        }
        $serverDataMessage = "Server data and $( $mapEntries.Count ) converted map(s) installed: $dataPath"
        $modelSourcePath = Join-Path $privatePath "skater.glb"
        $propsSourcePath = Join-Path $privatePath "native-props"
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

    if ($clientMapsPath) {
        $replaceClientMaps = $true
        if (Test-Path -LiteralPath $clientMapsPath -PathType Container) {
            $answer = [System.Windows.Forms.MessageBox]::Show(
                "Replace the existing local PEX Skate map and authored prop packages?",
                "Replace client maps?",
                [System.Windows.Forms.MessageBoxButtons]::YesNo,
                [System.Windows.Forms.MessageBoxIcon]::Warning
            )
            $replaceClientMaps = $answer -eq [System.Windows.Forms.DialogResult]::Yes
        }
        if ($replaceClientMaps) {
            $clientMapParent = Split-Path -Parent $clientMapsPath
            $temporaryMapsPath = Join-Path $clientMapParent (".maps-" + [guid]::NewGuid().ToString("N") + ".tmp")
            $oldMapsPath = Join-Path $clientMapParent (".maps-" + [guid]::NewGuid().ToString("N") + ".previous")
            $temporaryPropsPath = Join-Path $clientMapParent (".props-" + [guid]::NewGuid().ToString("N") + ".tmp")
            $oldPropsPath = Join-Path $clientMapParent (".props-" + [guid]::NewGuid().ToString("N") + ".previous")
            $temporaryManifestPath = Join-Path $clientMapParent (".maps-" + [guid]::NewGuid().ToString("N") + ".json.tmp")
            $oldManifestPath = Join-Path $clientMapParent (".maps-" + [guid]::NewGuid().ToString("N") + ".json.previous")
            $installedClientMaps = $false
            $installedClientProps = $false
            $installedClientManifest = $false
            try {
                New-Item -ItemType Directory -Force -Path $clientMapParent | Out-Null
                $mapsSource = if ($dataPath) { $mapsPath } else { Join-Path $stagePath "maps" }
                Copy-Item -LiteralPath $mapsSource -Destination $temporaryMapsPath -Recurse
                $manifestSource = Join-Path (Split-Path -Parent $mapsSource) "maps.json"
                Copy-Item -LiteralPath $manifestSource -Destination $temporaryManifestPath
                New-Item -ItemType Directory -Force -Path $temporaryPropsPath | Out-Null
                foreach ($entry in $mapEntries) {
                    if (-not $entry.props) { continue }
                    if ($entry.props.path -notmatch '^props/[A-Za-z0-9._-]+\.skate$' -or
                        $entry.props.path -ne ("props/" + [System.IO.Path]::GetFileName($entry.path)) -or
                        $entry.props.sha256 -notmatch '^[A-Fa-f0-9]{64}$') {
                        throw "Conversion produced invalid authored prop metadata for $($entry.path)."
                    }
                    $propsFileName = [System.IO.Path]::GetFileName($entry.props.path)
                    $propsSource = Join-Path $propsSourcePath $propsFileName
                    if (-not (Test-Path -LiteralPath $propsSource -PathType Leaf)) {
                        throw "Conversion did not produce $($entry.props.path). Nothing was installed to this client."
                    }
                    $propsHash = (Get-FileHash -LiteralPath $propsSource -Algorithm SHA256).Hash.ToLowerInvariant()
                    if ($propsHash -ne $entry.props.sha256.ToLowerInvariant()) {
                        throw "Converted prop hash does not match maps.json: $($entry.props.path)"
                    }
                    Copy-Item -LiteralPath $propsSource -Destination (Join-Path $temporaryPropsPath $propsFileName)
                }
                if (Test-Path -LiteralPath $clientMapsPath) {
                    Move-Item -LiteralPath $clientMapsPath -Destination $oldMapsPath
                }
                if (Test-Path -LiteralPath $clientPropsPath) {
                    Move-Item -LiteralPath $clientPropsPath -Destination $oldPropsPath
                }
                if (Test-Path -LiteralPath $clientMapsManifestPath) {
                    Move-Item -LiteralPath $clientMapsManifestPath -Destination $oldManifestPath
                }
                Move-Item -LiteralPath $temporaryMapsPath -Destination $clientMapsPath
                $installedClientMaps = $true
                Move-Item -LiteralPath $temporaryPropsPath -Destination $clientPropsPath
                $installedClientProps = $true
                Move-Item -LiteralPath $temporaryManifestPath -Destination $clientMapsManifestPath
                $installedClientManifest = $true
                try {
                    if (Test-Path -LiteralPath $oldMapsPath) {
                        Remove-Item -LiteralPath $oldMapsPath -Recurse -Force
                    }
                    if (Test-Path -LiteralPath $oldPropsPath) {
                        Remove-Item -LiteralPath $oldPropsPath -Recurse -Force
                    }
                    if (Test-Path -LiteralPath $oldManifestPath) {
                        Remove-Item -LiteralPath $oldManifestPath -Force
                    }
                } catch {
                    Write-Warning "Client maps were installed, but an old backup could not be removed: $($_.Exception.Message)"
                }
                $clientModelMessage += "`r`nClient map packages, authored props, and integrity catalog installed: $clientMapsPath"
            } catch {
                if ($installedClientMaps -and (Test-Path -LiteralPath $clientMapsPath)) { Remove-Item -LiteralPath $clientMapsPath -Recurse -Force }
                if (Test-Path -LiteralPath $oldMapsPath) { Move-Item -LiteralPath $oldMapsPath -Destination $clientMapsPath }
                if ($installedClientProps -and (Test-Path -LiteralPath $clientPropsPath)) { Remove-Item -LiteralPath $clientPropsPath -Recurse -Force }
                if (Test-Path -LiteralPath $oldPropsPath) { Move-Item -LiteralPath $oldPropsPath -Destination $clientPropsPath }
                if ($installedClientManifest -and (Test-Path -LiteralPath $clientMapsManifestPath)) { Remove-Item -LiteralPath $clientMapsManifestPath -Force }
                if (Test-Path -LiteralPath $oldManifestPath) { Move-Item -LiteralPath $oldManifestPath -Destination $clientMapsManifestPath }
                $clientModelMessage += "`r`nClient map copy failed: $($_.Exception.Message)"
            } finally {
                if (Test-Path -LiteralPath $temporaryMapsPath) {
                    Remove-Item -LiteralPath $temporaryMapsPath -Recurse -Force
                }
                if (Test-Path -LiteralPath $temporaryPropsPath) {
                    Remove-Item -LiteralPath $temporaryPropsPath -Recurse -Force
                }
                if (Test-Path -LiteralPath $temporaryManifestPath) {
                    Remove-Item -LiteralPath $temporaryManifestPath -Force
                }
            }
        } else {
            $clientModelMessage += "`r`nExisting client maps kept."
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
