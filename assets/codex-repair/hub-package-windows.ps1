[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$RequestPath)
$ErrorActionPreference = 'Stop'
$request = Get-Content -LiteralPath $RequestPath -Raw -Encoding UTF8 | ConvertFrom-Json
$resultPath = Join-Path (Split-Path -Parent $RequestPath) 'package-result.json'
function Save-AtomicJson([string]$path,$object) {
  $temporary=$path+'.'+[guid]::NewGuid().ToString('N')+'.tmp'
  try {
    $data=[Text.UTF8Encoding]::new($false).GetBytes(($object|ConvertTo-Json -Depth 12))
    $stream=[IO.File]::Open($temporary,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
    try {$stream.Write($data,0,$data.Length);$stream.Flush($true)} finally {$stream.Dispose()}
    if ([IO.File]::Exists($path)) {[IO.File]::Replace($temporary,$path,$null)} else {[IO.File]::Move($temporary,$path)}
  } finally {if ([IO.File]::Exists($temporary)) {[IO.File]::Delete($temporary)}}
}
function Save-Result($object) { Save-AtomicJson $resultPath $object }
try {
  $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
  if ($identity.User.Value -cne $request.userSid -or (Get-Process -Id $PID).SessionId -ne $request.sessionId) { throw '修复执行器的用户或桌面会话已变化，未安装。' }
  if (@(Get-Process -Name ChatGPT,Codex -ErrorAction SilentlyContinue).Count -gt 0) { throw '请先完全退出 ChatGPT/Codex，Hub 可以保持运行。' }
  # Load only reviewed function declarations, never the skill's full workflow.
  $source = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'patch_codex_fast_mode_windows_msix.ps1') -Raw -Encoding UTF8
  $stop = $source.IndexOf("`nAssert-ComputerUseSurfaceOptions`n")
  if ($stop -lt 0) { $stop = $source.IndexOf("`r`nAssert-ComputerUseSurfaceOptions`r`n") }
  if ($stop -lt 0) { throw '内置修复工具的入口校验失败。' }
  $HubToolkitRoot=$PSScriptRoot
  . ([scriptblock]::Create($source.Substring(0,$stop).Replace('$PSScriptRoot','$HubToolkitRoot')))
  $OutputRoot = Split-Path -Parent $RequestPath
  $InstallPrerequisites = $true
  $script:InstalledWindowsSdkViaNuGet = $false
  $script:InstalledWindowsSdkViaWinget = $false
  $package = Get-AppxPackage -Name $request.packageName -ErrorAction Stop | Sort-Object Version -Descending | Select-Object -First 1
  if (-not $package) { throw '找不到当前用户安装的 ChatGPT/Codex。' }
  Assert-MsixExternalExecutor $package
  $journalPath = Join-Path (Split-Path -Parent $RequestPath) 'package-journal.json'
  if ($request.action -eq 'restore') {
    $journal = Get-Content -LiteralPath $journalPath -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($journal.stage -notin @('applied','installing','failed') -or $package.PackageFullName -cne $journal.installedPackage -or
        (Get-FileHash -LiteralPath (Join-Path $package.InstallLocation 'app\resources\app.asar')).Hash -ine $journal.patchedAsarSha256) {
      throw '客户端已在修复后更新或修改，未覆盖；请保留恢复包进一步检查。'
    }
    if ((Get-FileHash -LiteralPath $journal.recoveryPackage).Hash -ine $journal.recoverySha256) { throw '恢复包完整性校验失败。' }
    $restored = Invoke-TransactionalMsixInstall -MsixPath $journal.recoveryPackage -PackageName $package.Name
    if ((Get-FileHash -LiteralPath (Join-Path $restored.InstallLocation 'app\resources\app.asar')).Hash -ine $journal.originalAsarSha256) {throw '恢复后的客户端完整性不符，请保留恢复包进一步检查。'}
    $journal.stage = 'restored'
    Save-AtomicJson $journalPath $journal
    Save-Result @{ success=$true; message='已恢复修复前的客户端文件，保留用户配置和会话。'; applied=$false; canRestore=$false }
    exit 0
  }
  $plan = $request.plan
  if ((Get-FileHash -LiteralPath $plan.source).Hash -ine $plan.sourceSha256 -or
      (Get-FileHash -LiteralPath $plan.prepared).Hash -ine $plan.preparedSha256 -or
      $package.PackageFullName -cne $request.packageFullName) { throw '客户端在检查后已变化，请重新检查。' }
  $sourceRoot = [IO.Path]::GetFullPath($package.InstallLocation)
  if ([IO.Path]::GetFullPath($plan.source) -ine (Join-Path $sourceRoot 'app\resources\app.asar')) { throw '来源与当前用户安装包不一致。' }
  $output = [IO.Path]::GetFullPath((Split-Path -Parent $RequestPath))
  if ($output.StartsWith($sourceRoot+'\',[StringComparison]::OrdinalIgnoreCase)) { throw '修复工作目录不能位于安装目录。' }
  $makeappx = Require-WindowsSdkTool 'makeappx.exe'
  $signtool = Require-WindowsSdkTool 'signtool.exe'
  $patchedRoot = Join-Path $output 'patched-package'
  $recoveryRoot = Join-Path $output 'recovery-package'
  foreach ($stage in @($patchedRoot,$recoveryRoot)) {
    if (Test-Path -LiteralPath $stage) { throw '修复工作目录已存在，请重新检查生成新任务。' }
    Copy-PackageLayout $sourceRoot $stage
    Remove-OldPackageArtifacts $stage
  }
  Copy-Item -LiteralPath $plan.prepared -Destination (Join-Path $patchedRoot 'app\resources\app.asar')
  Update-ElectronAsarIntegrity (Join-Path $patchedRoot 'app')
  $patchedVersion = Set-MsixUpdateVersion -ManifestPath (Join-Path $patchedRoot 'AppxManifest.xml')
  $recoveryVersion = Set-MsixUpdateVersion -ManifestPath (Join-Path $recoveryRoot 'AppxManifest.xml') -MinimumVersion $patchedVersion
  $cert = Get-OrCreateSigningCertificate ([string]$package.Publisher)
  Trust-SigningCertificate $cert
  $patchedMsix = Join-Path $output 'hub-codex-repaired.msix'
  $recoveryMsix = Join-Path $output 'hub-codex-original-recovery.msix'
  Invoke-MakeAppxPack $makeappx $patchedRoot $patchedMsix
  Invoke-SignPackage $signtool $patchedMsix $cert
  Invoke-MakeAppxPack $makeappx $recoveryRoot $recoveryMsix
  Invoke-SignPackage $signtool $recoveryMsix $cert
  . (Join-Path $PSScriptRoot 'lib\msix-payload.ps1')
  Test-MsixPayload -Path $patchedMsix | Out-Null
  Test-MsixPayload -Path $recoveryMsix | Out-Null
  $null = Test-MsixDeploymentPreflight -MsixPath $patchedMsix -PackageName $package.Name
  $null = Test-MsixDeploymentPreflight -MsixPath $recoveryMsix -PackageName $package.Name
  $journal = [ordered]@{schemaVersion=1;stage='prepared';sourcePackage=$package.PackageFullName;installedPackage='';patchedAsarSha256=$plan.preparedSha256;recoveryPackage=$recoveryMsix;recoverySha256=(Get-FileHash -LiteralPath $recoveryMsix).Hash;originalAsarSha256=$plan.sourceSha256;publisher=$package.Publisher;patchedVersion=[string]$patchedVersion;recoveryVersion=[string]$recoveryVersion}
  function Save-Journal { Save-AtomicJson $journalPath $journal }
  Save-Journal
  if (@(Get-Process -Name ChatGPT,Codex -ErrorAction SilentlyContinue).Count -gt 0 -or
      (Get-AppxPackage -Name $package.Name | Sort-Object Version -Descending | Select-Object -First 1).PackageFullName -cne $package.PackageFullName) { throw '应用重新启动或更新了，请退出后重新检查。' }
  $journal.stage = 'installing'
  # Persist the expected identity before deployment so recovery survives a crash.
  $journal.installedPackage = $package.Name+'_'+$patchedVersion+'_'+$package.Architecture.ToString().ToLowerInvariant()+'__'+($package.PackageFamilyName -replace '^.*_','')
  Save-Journal
  $validate = {
    param($installed)
    $asar = Join-Path $installed.InstallLocation 'app\resources\app.asar'
    $expected = if ([version]$installed.Version -eq $recoveryVersion -or $installed.PackageFullName -ceq $package.PackageFullName) { $plan.sourceSha256 } else { $plan.preparedSha256 }
    if ((Get-FileHash -LiteralPath $asar).Hash -ine $expected) { throw '安装后的客户端完整性校验失败。' }
    foreach ($hostExe in (Get-AsarIntegrityHostCandidates (Join-Path $installed.InstallLocation 'app'))) {
      $records = Get-AsarIntegrityRecords -ExePath $hostExe.FullName -AppRoot (Join-Path $installed.InstallLocation 'app')
      if ($records.Records.Count) { Assert-AsarIntegrityConsistent -ExePath $hostExe.FullName -AppRoot (Join-Path $installed.InstallLocation 'app') }
    }
  }
  $outcome = Invoke-RecoverableMsixInstall -MsixPath $patchedMsix -RecoveryMsixPath $recoveryMsix -ValidateInstalledPackage $validate -PackageName $package.Name
  if (-not $outcome.Success) { $journal.stage = if ($outcome.Recovered) {'restored'} else {'failed'}; Save-Journal; throw "安装未完成，已保留原程序或恢复原文件：$($outcome.Error)" }
  $journal.installedPackage = $outcome.Package.PackageFullName
  $journal.stage = 'applied'; Save-Journal
  Save-Result @{success=$true;message='客户端文件修复及安装完整性校验完成，恢复包已保留。请使用增强启动，再测试相应功能。';applied=$true;canRestore=$true}
} catch {
  Save-Result @{success=$false;message=$_.Exception.Message;applied=$false;canRestore=(Test-Path -LiteralPath (Join-Path (Split-Path -Parent $RequestPath) 'package-journal.json'))}
  exit 1
} finally {
  if (($script:InstalledWindowsSdkViaNuGet -or $script:InstalledWindowsSdkViaWinget) -and (Get-Command Cleanup-WindowsSdk -CommandType Function -ErrorAction SilentlyContinue)) {try {Cleanup-WindowsSdk} catch {}}
}
