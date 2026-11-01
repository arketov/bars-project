$ErrorActionPreference='Stop'
Add-Type @'
using System;using System.Runtime.InteropServices;
public class IdleIO {
 [StructLayout(LayoutKind.Sequential)]public struct Counts{public ulong readOps,writeOps,otherOps,readBytes,writeBytes,otherBytes;}
 [DllImport("kernel32.dll")]static extern bool GetProcessIoCounters(IntPtr h,out Counts c);
 public static Counts Get(IntPtr h){Counts c;if(!GetProcessIoCounters(h,out c))throw new Exception("IO counters");return c;}
}
'@
$syntheticResults=@()
foreach($syntheticMode in @('large-without-rowid','small-without-rowid','large-rowid')) {
 $syntheticStart=[Diagnostics.ProcessStartInfo]::new('node')
 $syntheticStart.ArgumentList.Add('--disable-warning=ExperimentalWarning')
 $syntheticStart.ArgumentList.Add((Join-Path $PSScriptRoot 'idle-synthetic.cjs'))
 $syntheticStart.ArgumentList.Add($syntheticMode)
 $syntheticStart.ArgumentList.Add((Join-Path (Join-Path (Get-Location) '.bars') ('idle-'+$syntheticMode+'-'+[guid]::NewGuid().ToString('N')+'.sqlite')))
 $syntheticStart.RedirectStandardInput=$true;$syntheticStart.RedirectStandardOutput=$true;$syntheticStart.UseShellExecute=$false
 $syntheticProcess=[Diagnostics.Process]::Start($syntheticStart)
 try {
  $syntheticReady=$syntheticProcess.StandardOutput.ReadLine() | ConvertFrom-Json
  if(-not $syntheticReady.ready){throw 'No ready signal'}
  $syntheticBefore=[IdleIO]::Get($syntheticProcess.Handle)
  $syntheticProcess.StandardInput.WriteLine('delete')
  $syntheticResult=$syntheticProcess.StandardOutput.ReadLine() | ConvertFrom-Json
  $syntheticAfter=[IdleIO]::Get($syntheticProcess.Handle)
  $syntheticResult | Add-Member read_bytes ($syntheticAfter.readBytes-$syntheticBefore.readBytes)
  $syntheticResult | Add-Member read_ops ($syntheticAfter.readOps-$syntheticBefore.readOps)
  $syntheticResult | Add-Member sqlite $syntheticReady.sqlite
  $syntheticResults+=$syntheticResult
  $syntheticResult | ConvertTo-Json -Compress
 } finally {
  $syntheticProcess.StandardInput.Close();$syntheticProcess.WaitForExit();$syntheticProcess.Dispose()
 }
}
$syntheticResults | ConvertTo-Json -Depth 4 | Set-Content docs/knowledge/idle-load-2026-11-01/synthetic.json -Encoding utf8
