param([Parameter(Mandatory)][int]$RootId,[int]$Seconds=30,[Parameter(Mandatory)][string]$Output)
$ErrorActionPreference='Stop'
$exe=Join-Path $env:LOCALAPPDATA 'bars/bars.exe'
$metrics=Join-Path $PSScriptRoot 'metrics.ps1'
function Snapshot([string]$Action='Snapshot'){ & $metrics -RootId $RootId -ExpectedPath $exe -Action $Action | ConvertFrom-Json }
$before=Snapshot
$main=@($before.windows|Where-Object Title -eq 'bars')
if($main.Count -ne 1 -or $main[0].Minimized){throw 'Неоднозначное/минимизированное исходное окно'}
$initialVisible=$main[0].Visible
$report=[ordered]@{root=$RootId;exe_sha256=(Get-FileHash -LiteralPath $exe).Hash;initial=$before;samples=@();success=$false}
try{
 foreach($round in 1..3){foreach($visible in @($false,$true)){
  $state=Snapshot $(if($visible){'Show'}else{'Hide'})
  if(@($state.windows|Where-Object {$_.Title -eq 'bars' -and $_.Visible -eq $visible}).Count -ne 1){throw 'Не подтверждена видимость'}
  Start-Sleep -Seconds 10
  $start=Snapshot
  Start-Sleep -Seconds $Seconds
  $end=Snapshot
  $report.samples+=@{round=$round;visible=$visible;before=$start;after=$end}
  $report|ConvertTo-Json -Depth 12|Set-Content -LiteralPath $Output -Encoding utf8
 }}
 $report.success=$true
}finally{
 $report.final=Snapshot $(if($initialVisible){'Show'}else{'Hide'})
 $report.restored=(@($report.final.windows|Where-Object {$_.Title -eq 'bars' -and $_.Visible -eq $initialVisible}).Count -eq 1)
 $report|ConvertTo-Json -Depth 12|Set-Content -LiteralPath $Output -Encoding utf8
}
$since=(Get-Date).AddDays(-3)
$events=@(Get-WinEvent -FilterHashtable @{LogName='Application';Id=1000,1001,1002;StartTime=$since} -ErrorAction SilentlyContinue)
$matching=@($events|Where-Object Message -Match 'bars.exe|msedgewebview2.exe')
$report.diagnostics=@{
 since_utc=$since.ToUniversalTime().ToString('o')
 queried_events=$events.Count
 matching_events=@($matching|ForEach-Object {@{utc=$_.TimeCreated.ToUniversalTime().ToString('o');id=$_.Id;provider=$_.ProviderName}})
 crashpad_reports=@(Get-ChildItem (Join-Path $env:LOCALAPPDATA 'io.github.arketov.bars/EBWebView/Crashpad/reports') -File -ErrorAction SilentlyContinue|ForEach-Object {@{name=$_.Name;bytes=$_.Length;modified_utc=$_.LastWriteTimeUtc.ToString('o')}})
 runtime_directories=@(Get-ChildItem 'C:/Program Files (x86)/Microsoft/EdgeWebView/Application' -Directory|Where-Object Name -Match '^\d'|ForEach-Object {@{version=$_.Name;created_utc=$_.CreationTimeUtc.ToString('o')}})
}
$report|ConvertTo-Json -Depth 12|Set-Content -LiteralPath $Output -Encoding utf8
