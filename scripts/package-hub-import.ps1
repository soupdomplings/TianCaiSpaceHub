param(
    [ValidateSet('debug','release')][string]$Profile = 'release',
    [string]$OutputDirectory = ''
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$manifest = Get-Content -LiteralPath (Join-Path $projectRoot 'Cargo.toml') -Raw
$versionMatch = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"')
if (-not $versionMatch.Success) { throw 'Package version missing' }
$version = $versionMatch.Groups[1].Value
$productVersion = $version.Split('-')[0]
$exe = Join-Path $projectRoot "target\$Profile\codexhub.exe"
if (-not (Test-Path -LiteralPath $exe)) { throw "Build first: cargo build --locked --release --features gui --bin codexhub" }
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $projectRoot "target\dist\hub-import-$version" }
$output = [IO.Path]::GetFullPath($OutputDirectory)
$stage = Join-Path $output 'portable'
$msiSource = Join-Path $output 'msi-source'
New-Item -ItemType Directory -Force -Path $stage,$msiSource | Out-Null
$zip = Join-Path $output "TianCaiSpaceHub-$version-windows-x64.zip"
$msi = Join-Path $output "TianCaiSpaceHub-$version-windows-x64.msi"
if ((Test-Path -LiteralPath $zip) -or (Test-Path -LiteralPath $msi)) { throw 'Package already exists; choose a new output directory' }
Copy-Item -LiteralPath $exe -Destination (Join-Path $stage 'TianCaiSpace Hub.exe')
Copy-Item -LiteralPath $exe -Destination (Join-Path $msiSource 'CodexHub.exe')
foreach ($name in @('README.md','README.en.md','config.example.toml')) {
    Copy-Item -LiteralPath (Join-Path $projectRoot $name) -Destination $stage
    Copy-Item -LiteralPath (Join-Path $projectRoot $name) -Destination $msiSource
}
Copy-Item -LiteralPath (Join-Path $projectRoot 'docs\hub-external-import.md') -Destination (Join-Path $stage 'WEB-IMPORT.md')
Copy-Item -LiteralPath (Join-Path $projectRoot 'packaging\icons\AppIcon.ico') -Destination $msiSource
$wix = Join-Path $env:USERPROFILE '.dotnet\tools\wix.exe'
& $wix build (Join-Path $projectRoot 'packaging\windows\CodexHub.wxs') -acceptEula wix7 -arch x64 -d "ProductVersion=$productVersion" -d "SourceDir=$msiSource" -out $msi
if ($LASTEXITCODE -ne 0) { throw 'WiX build failed' }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
$baseCommit = git -C $projectRoot rev-parse HEAD
$sourceTree = git -C $projectRoot rev-parse 'HEAD^{tree}'
$dirty = [bool](git -C $projectRoot status --porcelain --untracked-files=normal)
$trackedDirty = [bool](git -C $projectRoot status --porcelain --untracked-files=no)
$files = foreach ($file in @($msi,$zip,(Join-Path $stage 'TianCaiSpace Hub.exe'))) {
    [ordered]@{name=(Split-Path $file -Leaf); size=(Get-Item -LiteralPath $file).Length; sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $file).Hash.ToLowerInvariant()}
}
$metadata = [ordered]@{ version=$version; profile=$Profile; sourceCommit=$baseCommit; sourceTree=$sourceTree; workingTreeChanges=$dirty; trackedWorkingTreeChanges=$trackedDirty; signed=$false; builtAtUtc=[DateTime]::UtcNow.ToString('o'); files=@($files) }
[IO.File]::WriteAllText((Join-Path $output 'build-manifest.json'), ($metadata | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
$files | Format-List
