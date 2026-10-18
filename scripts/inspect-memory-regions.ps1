[CmdletBinding()]
param([Parameter(Mandatory)][int]$ProcessId, [Parameter(Mandatory)][string]$OutputPath)
$ErrorActionPreference = 'Stop'
if (Test-Path -LiteralPath $OutputPath) { throw 'Output exists' }
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class MemoryRegions {
  [StructLayout(LayoutKind.Sequential)]
  public struct Region {
    public IntPtr BaseAddress, AllocationBase;
    public uint AllocationProtect;
    public ushort PartitionId;
    public UIntPtr RegionSize;
    public uint State, Protect, Type;
  }
  [DllImport("kernel32.dll", SetLastError=true)]
  public static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
  [DllImport("kernel32.dll", SetLastError=true)]
  public static extern UIntPtr VirtualQueryEx(IntPtr handle, IntPtr address, out Region region, UIntPtr length);
  [DllImport("kernel32.dll")]
  public static extern bool CloseHandle(IntPtr handle);
}
'@
$process = Get-Process -Id $ProcessId
$start = $process.StartTime.ToUniversalTime().ToString('o')
$handle = [MemoryRegions]::OpenProcess(0x400, $false, $ProcessId)
if ($handle -eq [IntPtr]::Zero) { throw 'OpenProcess failed' }
$regions = @()
try {
    [long]$address = 0
    $size = [Runtime.InteropServices.Marshal]::SizeOf([type][MemoryRegions+Region])
    while ($true) {
        $region = [MemoryRegions+Region]::new()
        $read = [MemoryRegions]::VirtualQueryEx($handle, [IntPtr]$address, [ref]$region, [UIntPtr]$size)
        if ($read -eq [UIntPtr]::Zero) { break }
        if ($region.State -eq 0x1000) {
            $regions += [ordered]@{allocation_base=$region.AllocationBase.ToInt64();bytes=$region.RegionSize.ToUInt64();type=$region.Type;protect=$region.Protect}
        }
        $next = $region.BaseAddress.ToInt64() + [long]$region.RegionSize.ToUInt64()
        if ($next -le $address) { throw 'Invalid region progression' }
        $address = $next
    }
} finally { [void][MemoryRegions]::CloseHandle($handle) }
if ((Get-Process -Id $ProcessId).StartTime.ToUniversalTime().ToString('o') -ne $start) { throw 'PID reused' }
[ordered]@{utc=[DateTime]::UtcNow.ToString('o');pid=$ProcessId;start_utc=$start;exe_sha256=(Get-FileHash $process.Path).Hash;private_bytes=$process.PrivateMemorySize64;working_set_bytes=$process.WorkingSet64;regions=$regions;note='VirtualQueryEx committed address regions only; no memory contents read. Not atomic. MEM_PRIVATE=131072, MEM_MAPPED=262144, MEM_IMAGE=16777216.'} |
    ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $OutputPath -Encoding utf8
Write-Output "Saved $($regions.Count) committed regions"
