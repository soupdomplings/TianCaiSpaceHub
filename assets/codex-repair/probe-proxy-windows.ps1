# Read-only: output only whitelisted variable NAMES, never environment values.
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class HubEnvironmentProbe {
  [StructLayout(LayoutKind.Sequential)] struct Basic {public IntPtr a,peb,b,c,pid,d;}
  [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenProcess(uint a,bool b,int p);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr p);
  [DllImport("kernel32.dll")] static extern bool IsWow64Process(IntPtr p,out bool wow);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool ReadProcessMemory(IntPtr p,IntPtr a,byte[] b,int n,out IntPtr r);
  [DllImport("ntdll.dll")] static extern int NtQueryInformationProcess(IntPtr p,int c,out Basic b,int n,out int r);
  static long Pointer(IntPtr p,long address) {byte[] b=new byte[8];IntPtr n;if(!ReadProcessMemory(p,new IntPtr(address),b,8,out n)||n.ToInt64()!=8)throw new Exception("unreadable");return BitConverter.ToInt64(b,0);}
  public static string[] Names(int pid,string[] allowed) {
    if(IntPtr.Size!=8)throw new Exception("architecture");IntPtr p=OpenProcess(0x410,false,pid);if(p==IntPtr.Zero)throw new Exception("access");
    try {bool wow;if(!IsWow64Process(p,out wow)||wow)throw new Exception("architecture");Basic b;int length;if(NtQueryInformationProcess(p,0,out b,Marshal.SizeOf(typeof(Basic)),out length)!=0)throw new Exception("PEB");
      long parameters=Pointer(p,b.peb.ToInt64()+0x20),env=Pointer(p,parameters+0x80);var data=new List<byte>();bool end=false;
      for(int offset=0;offset<2*1024*1024&&!end;offset+=4096){byte[] chunk=new byte[4096];IntPtr n;ReadProcessMemory(p,new IntPtr(env+offset),chunk,chunk.Length,out n);int size=(int)n.ToInt64();if(size<=0)throw new Exception("environment");for(int i=0;i<size;i++)data.Add(chunk[i]);for(int i=Math.Max(0,data.Count-size-4);i+3<data.Count;i+=2)if(data[i]==0&&data[i+1]==0&&data[i+2]==0&&data[i+3]==0){end=true;break;}}
      if(!end)throw new Exception("environment-length");var names=new HashSet<string>(StringComparer.Ordinal);foreach(string entry in System.Text.Encoding.Unicode.GetString(data.ToArray()).Split('\0')){int at=entry.IndexOf('=');if(at>0){string key=entry.Substring(0,at);if(Array.IndexOf(allowed,key)>=0)names.Add(key);}}var output=new string[names.Count];names.CopyTo(output);return output;
    } finally {CloseHandle(p);}
  }
}
'@
$names=@('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','NO_PROXY','http_proxy','https_proxy','all_proxy','no_proxy')
$processes=@(Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,ExecutablePath)
$roots=@($processes | Where-Object {$_.Name -in @('ChatGPT.exe','Codex.exe') -and $_.ExecutablePath -match '(?i)\\WindowsApps\\OpenAI\.(Codex|ChatGPT)_[^\\]+\\app\\(ChatGPT|Codex)\.exe$'})
$ids=@{};foreach($root in $roots){$ids[[int]$root.ProcessId]=$true}
for($i=0;$i -lt 24;$i++){$added=$false;foreach($p in $processes){if($ids.ContainsKey([int]$p.ParentProcessId)-and -not $ids.ContainsKey([int]$p.ProcessId)){$ids[[int]$p.ProcessId]=$true;$added=$true}}if(-not $added){break}}
$items=@()
foreach($p in $processes){
  $isRoot=@($roots | Where-Object ProcessId -EQ $p.ProcessId).Count -gt 0
  if(-not $isRoot -and -not ($ids.ContainsKey([int]$p.ProcessId)-and $p.Name -in @('node.exe','node_repl.exe','codex-computer-use.exe'))){continue}
  try {$keys=@([HubEnvironmentProbe]::Names([int]$p.ProcessId,$names));$items+=@{pid=$p.ProcessId;root=$isRoot;readable=$true;names=$keys}}
  catch {$items+=@{pid=$p.ProcessId;root=$isRoot;readable=$false;names=@()}}
}
ConvertTo-Json -InputObject @($items) -Depth 5 -Compress
