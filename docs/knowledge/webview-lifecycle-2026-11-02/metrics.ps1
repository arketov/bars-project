param([Parameter(Mandatory)][int]$RootId, [ValidateSet('Snapshot','Show','Hide')][string]$Action='Snapshot', [string]$ExpectedPath='')
$ErrorActionPreference='Stop'
Add-Type @'
using System;using System.Collections.Generic;using System.Runtime.InteropServices;using System.Text;
public static class ResearchWindows {
 public record Row(long Handle,string Title,bool Visible,bool Minimized);
 delegate bool Callback(IntPtr h,IntPtr x);
 [DllImport("user32.dll")]static extern bool EnumWindows(Callback c,IntPtr x);
 [DllImport("user32.dll")]static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern int GetWindowText(IntPtr h,StringBuilder s,int n);
 [DllImport("user32.dll")]static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")]static extern bool IsIconic(IntPtr h);
 [DllImport("user32.dll",SetLastError=true)]static extern IntPtr SendMessageTimeout(IntPtr h,uint m,IntPtr w,IntPtr l,uint flags,uint timeout,out IntPtr result);
 public static Row[] List(int pid){var rows=new List<Row>();EnumWindows((h,x)=>{uint p;GetWindowThreadProcessId(h,out p);if(p==(uint)pid){var s=new StringBuilder(512);GetWindowText(h,s,512);if(s.Length>0)rows.Add(new Row(h.ToInt64(),s.ToString(),IsWindowVisible(h),IsIconic(h)));}return true;},IntPtr.Zero);return rows.ToArray();}
 public static void Hide(int pid){foreach(var row in List(pid)){if(row.Title=="bars"&&row.Visible){IntPtr result;SendMessageTimeout(new IntPtr(row.Handle),0x10,IntPtr.Zero,IntPtr.Zero,2,2000,out result);}}}
}
'@
$root=Get-Process -Id $RootId
if($ExpectedPath -and $root.Path -ne $ExpectedPath){throw 'Путь процесса изменился'}
if($Action -ne 'Snapshot'){
 if(-not $ExpectedPath -or $root.ProcessName -ne 'bars'){throw 'Показ/скрытие разрешены только проверенному bars'}
 if($Action -eq 'Show'){Start-Process -FilePath $ExpectedPath -WindowStyle Hidden}
 else{[ResearchWindows]::Hide($RootId)}
 Start-Sleep -Seconds 2
}
$all=@(Get-CimInstance Win32_Process)
$ids=@($RootId)
do{$old=$ids.Count;$ids+=@($all|Where-Object {$_.ParentProcessId -in $ids -and $_.ProcessId -notin $ids}|ForEach-Object {$_.ProcessId})}while($ids.Count -gt $old)
$rows=@($all|Where-Object {$_.ProcessId -in $ids -and ($_.ProcessId -eq $RootId -or $_.Name -eq 'msedgewebview2.exe')}|ForEach-Object {
 $proc=Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
 if($proc){
  $role=if($_.ProcessId -eq $RootId){'native'}elseif($_.CommandLine -match '--type=([^ ]+)'){$Matches[1]}else{'browser'}
  [pscustomobject]@{pid=$proc.Id;parent=$_.ParentProcessId;role=$role;start_utc=$proc.StartTime.ToUniversalTime().ToString('o');cpu=$proc.TotalProcessorTime.TotalSeconds;logical_read=[double]$_.ReadTransferCount;private=$proc.PrivateMemorySize64;working=$proc.WorkingSet64}
 }
})
[pscustomobject]@{utc=[DateTime]::UtcNow.ToString('o');time=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds();logical_processors=[Environment]::ProcessorCount;processes=$rows;windows=@([ResearchWindows]::List($RootId));compilers=@($all|Where-Object {$_.Name -in @('cargo.exe','rustc.exe','link.exe','cl.exe')}|Select-Object ProcessId,Name)}|ConvertTo-Json -Depth 5 -Compress
