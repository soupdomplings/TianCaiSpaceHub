param([Parameter(Mandatory=$true)][string]$RequestPath)
$ErrorActionPreference='Stop'
$OutputEncoding=[Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
try {
  $runner=Join-Path $PSScriptRoot 'hub-package-windows.ps1'
  $result=Join-Path (Split-Path -Parent $RequestPath) 'package-result.json'
  if (Test-Path -LiteralPath $result) { Remove-Item -LiteralPath $result }
  $command="& '"+$runner.Replace("'","''")+"' -RequestPath '"+$RequestPath.Replace("'","''")+"'; exit `$LASTEXITCODE"
  $encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
  $powershell=Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
  $process=Start-Process -FilePath $powershell -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-EncodedCommand',$encoded) -Verb RunAs -WindowStyle Hidden -PassThru -Wait
  if (-not (Test-Path -LiteralPath $result -PathType Leaf)) { throw '系统修复未返回结果。原程序未预先卸载，请保留备份进一步检查。' }
  Get-Content -LiteralPath $result -Raw -Encoding UTF8
} catch { @{success=$false;message='系统修复未开始或已中断。请确认系统权限请求，原程序未预先卸载。';applied=$false;canRestore=$false}|ConvertTo-Json -Compress }
