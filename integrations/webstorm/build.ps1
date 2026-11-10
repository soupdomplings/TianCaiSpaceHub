param([string]$Jdk = $env:JAVA_HOME)
$ErrorActionPreference = 'Stop'
if (-not $Jdk) { throw '请通过 -Jdk 指定 JDK 17 或更新版本' }
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$buildRoot = Join-Path $projectRoot ('target/webstorm-bridge/build-' + [Guid]::NewGuid().ToString('N'))
$stubRoot = Join-Path $buildRoot 'contracts'
$classes = Join-Path $buildRoot 'classes'
New-Item -ItemType Directory -Force -Path $stubRoot,$classes | Out-Null
$contracts = @{
 'com/intellij/openapi/project/Project.java' = 'package com.intellij.openapi.project; public interface Project {}'
 'com/intellij/openapi/startup/ProjectActivity.java' = 'package com.intellij.openapi.startup; public interface ProjectActivity { Object execute(com.intellij.openapi.project.Project project, kotlin.coroutines.Continuation<? super kotlin.Unit> continuation); }'
 'kotlin/Unit.java' = 'package kotlin; public final class Unit { public static final Unit INSTANCE = new Unit(); }'
 'kotlin/coroutines/Continuation.java' = 'package kotlin.coroutines; public interface Continuation<T> {}'
}
foreach ($item in $contracts.GetEnumerator()) {
 $file = Join-Path $stubRoot $item.Key
 New-Item -ItemType Directory -Force -Path (Split-Path $file) | Out-Null
 [IO.File]::WriteAllText($file,$item.Value)
}
$sourceFiles = @(Get-ChildItem $stubRoot -Recurse -Filter '*.java' | ForEach-Object FullName) + @(Get-ChildItem (Join-Path $PSScriptRoot 'src') -Recurse -Filter '*.java' | ForEach-Object FullName)
& (Join-Path $Jdk 'bin/javac.exe') --release 17 -encoding UTF-8 -d $classes @sourceFiles
if ($LASTEXITCODE) { throw '辅助组件编译失败' }
$stage = Join-Path $buildRoot 'package'
New-Item -ItemType Directory -Force -Path (Join-Path $stage 'META-INF'),(Join-Path $stage 'space') | Out-Null
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'META-INF/plugin.xml') -Destination (Join-Path $stage 'META-INF/plugin.xml') -Force
Copy-Item -LiteralPath (Join-Path $classes 'space/tiancai') -Destination (Join-Path $stage 'space') -Recurse -Force
$output = Join-Path $projectRoot 'resources/webstorm-bridge'
New-Item -ItemType Directory -Force -Path $output | Out-Null
& (Join-Path $Jdk 'bin/jar.exe') --create --file (Join-Path $output 'tiancai-hub-bridge.jar') -C $stage .
if ($LASTEXITCODE) { throw '辅助组件打包失败' }
$jar = Join-Path $output 'tiancai-hub-bridge.jar'
[pscustomobject]@{ Classes=$classes; Jar=$jar; Sha256=(Get-FileHash -LiteralPath $jar -Algorithm SHA256).Hash }
