[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$PackagePath, [string]$FlowData)
$ErrorActionPreference='Stop'
if (-not $FlowData) {
    $scoop=if($env:SCOOP){$env:SCOOP}else{Join-Path $env:USERPROFILE 'scoop'}
    $root=Join-Path $scoop 'apps\flow-launcher\current'
    $manifest=Get-Content (Join-Path $root 'scoop-manifest.json') -Raw | ConvertFrom-Json
    $FlowData=Join-Path $root ("app-$($manifest.version)\UserData")
}
$settingsPath=Join-Path $FlowData 'Settings\Settings.json'
if (-not (Test-Path -LiteralPath $settingsPath)) {throw 'Flow settings not found; supply -FlowData.'}
$unpack=Join-Path $env:TEMP ('flow-glazewm-'+[guid]::NewGuid())
try {
    Expand-Archive -LiteralPath $PackagePath -DestinationPath $unpack
    foreach($name in @('plugin.json','flow-glazewm.exe','icon.png')) {
        if(-not (Test-Path -LiteralPath (Join-Path $unpack $name))) {throw "Incomplete package: $name"}
    }
    $plugin=Get-Content (Join-Path $unpack 'plugin.json') -Raw | ConvertFrom-Json
    if($plugin.ID -ne '8B138D1FF90C44E19FA2920313C4E8C2'){throw 'Unexpected plugin ID'}
    $session=(Get-Process -Id $PID).SessionId
    $running=@(Get-Process -Name Flow.Launcher -ErrorAction SilentlyContinue | Where-Object SessionId -eq $session)
    $exe=if($running.Count){$running[0].Path}else{$null}
    $backup=Join-Path $FlowData ('Backups\glazewm-'+(Get-Date -Format 'yyyyMMdd-HHmmss-fff'))
    New-Item -ItemType Directory -Path $backup -Force | Out-Null
    $destination=Join-Path $FlowData ("Plugins\GlazeWM Windows-$($plugin.Version)")
    if(Test-Path $destination){Copy-Item -LiteralPath $destination -Destination $backup -Recurse}
    Copy-Item -LiteralPath $settingsPath -Destination $backup
    foreach($process in $running){Stop-Process -Id $process.Id -Force}
    try {
        New-Item -ItemType Directory $destination -Force | Out-Null
        Copy-Item -Path (Join-Path $unpack '*') -Destination $destination -Force
        $settings=Get-Content $settingsPath -Raw | ConvertFrom-Json
        $plugins=$settings.PluginSettings.Plugins
        $old=$plugins.PSObject.Properties[$plugin.ID]
        $priority=if($old){[Math]::Max(2,[int]$old.Value.Priority)}else{2}
        $plugins | Add-Member NoteProperty $plugin.ID ([pscustomobject]@{ID=$plugin.ID;Name=$plugin.Name;Version=$plugin.Version;ActionKeywords=@('*','gw');Priority=$priority;Disabled=$false;HomeDisabled=$false}) -Force
        $walker=$plugins.PSObject.Properties['F737A9223560B3C6833B5FFB8CDF78E5']
        if($walker){$walker.Value.Disabled=$true}
        [IO.File]::WriteAllText($settingsPath,($settings | ConvertTo-Json -Depth 50),(New-Object Text.UTF8Encoding($false)))
    } catch {
        Copy-Item -LiteralPath (Join-Path $backup 'Settings.json') -Destination $settingsPath -Force
        throw
    } finally {if($exe){Start-Process -FilePath $exe}}
    Write-Host "Plugin installed. Settings backup: $backup"
} finally {if(Test-Path $unpack){Remove-Item -LiteralPath $unpack -Recurse -Force}}
