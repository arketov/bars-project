$ErrorActionPreference = 'Stop'
# Read-only stack sample. A thread is suspended only for context/stack capture;
# ResumeThread runs in finally, before symbol lookup or output.
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class BarsIdleStack {
 [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenProcess(uint a,bool b,int c);
 [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenThread(uint a,bool b,int c);
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
 [DllImport("kernel32.dll",SetLastError=true)] static extern uint SuspendThread(IntPtr h);
 [DllImport("kernel32.dll",SetLastError=true)] static extern uint ResumeThread(IntPtr h);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetThreadContext(IntPtr h,IntPtr c);
 [DllImport("dbghelp.dll",SetLastError=true,CharSet=CharSet.Ansi)] static extern bool SymInitialize(IntPtr p,string path,bool invade);
 [DllImport("dbghelp.dll")] static extern bool SymCleanup(IntPtr p);
 [DllImport("dbghelp.dll")] static extern uint SymSetOptions(uint o);
 [DllImport("dbghelp.dll")] static extern IntPtr SymFunctionTableAccess64(IntPtr p,ulong a);
 [DllImport("dbghelp.dll")] static extern ulong SymGetModuleBase64(IntPtr p,ulong a);
 delegate IntPtr FunctionTable(IntPtr p,ulong a);
 delegate ulong ModuleBase(IntPtr p,ulong a);
 [DllImport("dbghelp.dll",SetLastError=true)] static extern bool StackWalk64(uint machine,IntPtr p,IntPtr t,IntPtr frame,IntPtr context,IntPtr read,FunctionTable fun,ModuleBase mod,IntPtr trans);
 [DllImport("dbghelp.dll",SetLastError=true,CharSet=CharSet.Ansi)] static extern bool SymFromAddr(IntPtr p,ulong a,out ulong displacement,IntPtr symbol);
 static void Put64(IntPtr b,int o,ulong v){Marshal.WriteInt64(b,o,unchecked((long)v));}
 public static string[] Capture(int pid,int tid){
  IntPtr p=OpenProcess(0x410,false,pid),t=IntPtr.Zero,raw=IntPtr.Zero,frame=IntPtr.Zero,symbol=IntPtr.Zero;
  bool symbols=false,suspended=false;
  var addresses=new List<ulong>();var result=new List<string>();
  try {
   if(p==IntPtr.Zero)throw new Exception("OpenProcess: "+Marshal.GetLastWin32Error());
   SymSetOptions(0x200|0x10|0x2|0x20000);
   symbols=SymInitialize(p,"C:\\bt\\bars\\release",true);
   t=OpenThread(0x4a,false,tid);
   if(t==IntPtr.Zero)throw new Exception("OpenThread: "+Marshal.GetLastWin32Error());
   raw=Marshal.AllocHGlobal(1264);var c=new IntPtr((raw.ToInt64()+15)&~15L);
   Marshal.Copy(new byte[1232],0,c,1232);Marshal.WriteInt32(c,48,0x10000b);
   frame=Marshal.AllocHGlobal(512);Marshal.Copy(new byte[512],0,frame,512);
   FunctionTable fun=SymFunctionTableAccess64;ModuleBase mod=SymGetModuleBase64;
   try {
    if(SuspendThread(t)==uint.MaxValue)throw new Exception("SuspendThread: "+Marshal.GetLastWin32Error());
    suspended=true;
    if(!GetThreadContext(t,c))throw new Exception("GetThreadContext: "+Marshal.GetLastWin32Error());
    Put64(frame,0,unchecked((ulong)Marshal.ReadInt64(c,248)));Marshal.WriteInt32(frame,12,3);
    Put64(frame,32,unchecked((ulong)Marshal.ReadInt64(c,160)));Marshal.WriteInt32(frame,44,3);
    Put64(frame,48,unchecked((ulong)Marshal.ReadInt64(c,152)));Marshal.WriteInt32(frame,60,3);
    addresses.Add(unchecked((ulong)Marshal.ReadInt64(c,248)));
    for(int i=0;i<40;i++){
     if(!StackWalk64(0x8664,p,t,frame,c,IntPtr.Zero,fun,mod,IntPtr.Zero))break;
     ulong a=unchecked((ulong)Marshal.ReadInt64(frame));if(a==0)break;addresses.Add(a);
    }
   }finally{if(suspended){ResumeThread(t);suspended=false;}}
   symbol=Marshal.AllocHGlobal(2096);
   foreach(ulong a in addresses){
    Marshal.Copy(new byte[2096],0,symbol,2096);Marshal.WriteInt32(symbol,0,88);Marshal.WriteInt32(symbol,80,2000);
    ulong d;string name=SymFromAddr(p,a,out d,symbol)?Marshal.PtrToStringAnsi(IntPtr.Add(symbol,84)):"?";
    result.Add("0x"+a.ToString("x")+" "+name+"+0x"+d.ToString("x"));
   }
   return result.ToArray();
  }finally{
   if(suspended)ResumeThread(t);
   if(symbol!=IntPtr.Zero)Marshal.FreeHGlobal(symbol);
   if(frame!=IntPtr.Zero)Marshal.FreeHGlobal(frame);
   if(raw!=IntPtr.Zero)Marshal.FreeHGlobal(raw);
   if(t!=IntPtr.Zero)CloseHandle(t);
   if(symbols)SymCleanup(p);
   if(p!=IntPtr.Zero)CloseHandle(p);
  }
 }
}
'@
$idleStacks = for ($sampleIndex=0; $sampleIndex -lt 5; $sampleIndex++) {
  [pscustomobject]@{utc=[DateTime]::UtcNow.ToString('o');pid=8944;tid=56644;stack=[BarsIdleStack]::Capture(8944,56644)}
  Start-Sleep -Milliseconds 250
}
$idleStacks | ConvertTo-Json -Depth 4 | Set-Content docs/knowledge/idle-load-2026-11-01/stacks.json -Encoding utf8
$idleStacks | ConvertTo-Json -Depth 4
