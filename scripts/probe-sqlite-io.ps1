param([Parameter(Mandatory)][string]$Index)
$ErrorActionPreference = 'Stop'
# Process file I/O includes Windows cache hits; this is not physical SSD traffic.
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class BarsSqliteIo {
    [StructLayout(LayoutKind.Sequential)]
    public struct Counts { public ulong readOps,writeOps,otherOps,readBytes,writeBytes,otherBytes; }
    [DllImport("kernel32.dll")]
    static extern bool GetProcessIoCounters(IntPtr handle, out Counts counts);
    public static Counts Get(IntPtr handle) {
        Counts counts;
        if (!GetProcessIoCounters(handle,out counts)) throw new Exception("GetProcessIoCounters failed");
        return counts;
    }
}
'@
$start = [Diagnostics.ProcessStartInfo]::new('node')
$start.ArgumentList.Add((Join-Path $PSScriptRoot 'probe-sqlite-io.cjs'))
$start.ArgumentList.Add((Resolve-Path -LiteralPath $Index).Path)
$start.RedirectStandardInput = $true
$start.RedirectStandardOutput = $true
$start.UseShellExecute = $false
$process = [Diagnostics.Process]::Start($start)
$results = @()
try {
    foreach ($mode in @('lookup','sequential','lookup','sequential','lookup_keys','sequential_keys')) {
        $before = [BarsSqliteIo]::Get($process.Handle)
        $process.StandardInput.WriteLine($mode)
        $result = $process.StandardOutput.ReadLine() | ConvertFrom-Json
        if (-not $result) { throw 'SQLite probe returned no result' }
        $after = [BarsSqliteIo]::Get($process.Handle)
        $result | Add-Member readBytes ($after.readBytes - $before.readBytes)
        $result | Add-Member readOps ($after.readOps - $before.readOps)
        $results += $result
    }
} finally {
    $process.StandardInput.Close()
    $process.WaitForExit()
    $process.Dispose()
}
if ($results[0].digest -ne $results[1].digest -or $results[2].digest -ne $results[3].digest -or $results[4].digest -ne $results[5].digest) {
    throw 'SQL results differ'
}
$results | ConvertTo-Json -Depth 5
