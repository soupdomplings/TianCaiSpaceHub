param([string]$Jdk = $env:JAVA_HOME)
$ErrorActionPreference = 'Stop'
if (-not $Jdk) { throw '请通过 -Jdk 指定 JDK 17 或更新版本' }
$build = & (Join-Path $PSScriptRoot 'build.ps1') -Jdk $Jdk
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$classes = $build.Classes
$testClasses = Join-Path (Split-Path $classes) 'test-classes'
New-Item -ItemType Directory -Force -Path $testClasses | Out-Null
$fixtures = @(Get-ChildItem (Join-Path $PSScriptRoot 'tests') -Recurse -Filter '*.java' | ForEach-Object FullName)
& (Join-Path $Jdk 'bin/javac.exe') --release 17 -encoding UTF-8 -cp $classes -d $testClasses @fixtures
if ($LASTEXITCODE) { throw '离线契约测试编译失败' }
& (Join-Path $Jdk 'bin/java.exe') -ea -cp "$testClasses;$classes" space.tiancai.hub.BridgeContractTest
if ($LASTEXITCODE) { throw '离线契约测试失败' }
