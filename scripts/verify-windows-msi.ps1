[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$MsiPath,
    [Parameter(Mandatory = $true)][string]$ExePath,
    [Parameter(Mandatory = $true)][string]$AppVersion
)

$ErrorActionPreference = 'Stop'

# Read package tables and PE version resources only. Never install the MSI or run its EXE.
function Release-ComObject {
    param([object]$Value)
    if ($null -ne $Value -and [Runtime.InteropServices.Marshal]::IsComObject($Value)) {
        [void][Runtime.InteropServices.Marshal]::ReleaseComObject($Value)
    }
}

function Read-MsiTable {
    param(
        [Parameter(Mandatory = $true)][object]$Database,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string[]]$Columns
    )

    $quotedColumns = @($Columns | ForEach-Object { '`' + $_ + '`' }) -join ','
    $query = 'SELECT ' + $quotedColumns + ' FROM `' + $Name + '`'
    $view = $null
    try {
        $view = $Database.GetType().InvokeMember('OpenView', 'InvokeMethod', $null, $Database, @($query))
        $view.GetType().InvokeMember('Execute', 'InvokeMethod', $null, $view, $null) | Out-Null
        while ($true) {
            $record = $view.GetType().InvokeMember('Fetch', 'InvokeMethod', $null, $view, $null)
            if ($null -eq $record) {
                break
            }
            try {
                $row = [ordered]@{}
                for ($index = 0; $index -lt $Columns.Count; $index++) {
                    $row[$Columns[$index]] = $record.GetType().InvokeMember('StringData', 'GetProperty', $null, $record, @($index + 1))
                }
                [pscustomobject]$row
            } finally {
                Release-ComObject $record
            }
        }
    } finally {
        if ($null -ne $view) {
            try {
                $view.GetType().InvokeMember('Close', 'InvokeMethod', $null, $view, $null) | Out-Null
            } finally {
                Release-ComObject $view
            }
        }
    }
}

$fullVersion = $AppVersion.Trim() -replace '^v', ''
$versionMatch = [regex]::Match($fullVersion, '^(?<major>\d+)\.(?<minor>\d+)\.(?<patch>\d+)(?:-(?<revision>\d+))?(?:\+[0-9A-Za-z.-]+)?$')
if (-not $versionMatch.Success) {
    throw "AppVersion must be major.minor.patch with an optional numeric customization revision: $AppVersion"
}
$versionParts = foreach ($name in @('major', 'minor', 'patch', 'revision')) {
    $value = [uint16]0
    $text = if ($versionMatch.Groups[$name].Success) { $versionMatch.Groups[$name].Value } else { '0' }
    if (-not [uint16]::TryParse($text, [ref]$value)) {
        throw "Windows version part '$name' is outside the uint16 range: $text"
    }
    $value
}
$expectedFileVersion = $versionParts -join '.'
$expectedProductVersion = $versionParts[0..2] -join '.'
$resolvedMsi = (Resolve-Path -LiteralPath $MsiPath).Path
$resolvedExe = (Resolve-Path -LiteralPath $ExePath).Path
$exe = Get-Item -LiteralPath $resolvedExe
$exeVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($resolvedExe)
$actualFileVersion = @($exeVersion.FileMajorPart, $exeVersion.FileMinorPart, $exeVersion.FileBuildPart, $exeVersion.FilePrivatePart) -join '.'
$actualProductVersion = @($exeVersion.ProductMajorPart, $exeVersion.ProductMinorPart, $exeVersion.ProductBuildPart, $exeVersion.ProductPrivatePart) -join '.'
$problems = [System.Collections.Generic.List[string]]::new()
if ($exeVersion.FileVersion -cne $fullVersion -or $exeVersion.ProductVersion -cne $fullVersion) {
    $problems.Add("EXE display version must be '$fullVersion'; got FileVersion '$($exeVersion.FileVersion)' and ProductVersion '$($exeVersion.ProductVersion)'.")
}
if ($actualFileVersion -ne $expectedFileVersion -or $actualProductVersion -ne $expectedFileVersion) {
    $problems.Add("EXE numeric version must be '$expectedFileVersion'; got FileVersion '$actualFileVersion' and ProductVersion '$actualProductVersion'.")
}

$installer = $null
$database = $null
try {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    # MSIDBOPEN_READONLY = 0: database inspection does not execute installer actions.
    $database = $installer.GetType().InvokeMember('OpenDatabase', 'InvokeMethod', $null, $installer, @($resolvedMsi, 0))
    $properties = @{}
    foreach ($row in @(Read-MsiTable $database 'Property' @('Property', 'Value'))) {
        $properties[$row.Property] = $row.Value
    }
    $upgradeCode = '{42B4BF3C-E660-4C83-96AA-56A4426B96A2}'
    $expectedProperties = [ordered]@{
        ProductName = 'TianCaiSpaceHub'
        Manufacturer = 'TianCaiSpaceHub'
        UpgradeCode = $upgradeCode
        ProductLanguage = '1033'
        ProductVersion = $expectedProductVersion
        REINSTALLMODE = 'emus'
        MSIRMSHUTDOWN = '1'
        MSIDISABLERMRESTART = '1'
    }
    foreach ($name in $expectedProperties.Keys) {
        if ($properties[$name] -cne $expectedProperties[$name]) {
            $problems.Add("MSI property '$name' must be '$($expectedProperties[$name])'; got '$($properties[$name])'.")
        }
    }
    if ($properties['MSIRESTARTMANAGERCONTROL'] -in @('Disable', 'DisableShutdown')) {
        $problems.Add('MSI must retain Restart Manager shutdown for installed-file holders.')
    }

    $files = @(Read-MsiTable $database 'File' @('File', 'FileName', 'FileSize', 'Version'))
    $mainFiles = @($files | Where-Object { ($_.FileName -split '\|')[-1] -ieq 'TianCaiSpaceHub.exe' })
    if ($mainFiles.Count -ne 1) {
        $problems.Add("MSI must contain exactly one TianCaiSpaceHub.exe file row; found $($mainFiles.Count).")
    } else {
        $mainFile = $mainFiles[0]
        if ($mainFile.File -cne 'TianCaiSpaceHubExe') {
            $problems.Add("MSI executable file ID must be 'TianCaiSpaceHubExe'; got '$($mainFile.File)'.")
        }
        if ($mainFile.Version -ne $expectedFileVersion -or $mainFile.Version -ne $actualFileVersion) {
            $problems.Add("MSI EXE File.Version must match '$expectedFileVersion' and the staged EXE; got '$($mainFile.Version)'.")
        }
        if ($mainFile.FileSize -ne [string]$exe.Length) {
            $problems.Add("MSI EXE FileSize must match staged EXE size '$($exe.Length)'; got '$($mainFile.FileSize)'.")
        }
    }

    $upgrades = @(Read-MsiTable $database 'Upgrade' @('UpgradeCode', 'VersionMin', 'VersionMax', 'Language', 'Attributes', 'Remove', 'ActionProperty'))
    if ($upgrades.Count -eq 0) {
        $problems.Add('MSI Upgrade table must contain upgrade detection rows.')
    }
    foreach ($row in $upgrades) {
        if ($row.UpgradeCode -ne $upgradeCode) {
            $problems.Add("MSI Upgrade row '$($row.ActionProperty)' has an unexpected UpgradeCode '$($row.UpgradeCode)'.")
        }
        if (-not [string]::IsNullOrEmpty($row.Language)) {
            $problems.Add("MSI Upgrade.Language must be empty to include legacy language 0 installs; '$($row.ActionProperty)' limits it to '$($row.Language)'.")
        }
    }
    $upgradeRows = @($upgrades | Where-Object { $_.ActionProperty -ceq 'WIX_UPGRADE_DETECTED' })
    if ($upgradeRows.Count -ne 1) {
        $problems.Add("MSI must contain exactly one WIX_UPGRADE_DETECTED row; found $($upgradeRows.Count).")
    } else {
        $row = $upgradeRows[0]
        $attributes = [int]0
        $validAttributes = [int]::TryParse($row.Attributes, [ref]$attributes)
        if ($row.VersionMax -ne $expectedProductVersion -or -not [string]::IsNullOrEmpty($row.VersionMin) -or
            -not $validAttributes -or ($attributes -band 512) -eq 0 -or ($attributes -band 2) -ne 0 -or
            -not [string]::IsNullOrEmpty($row.Remove)) {
            $problems.Add('MSI WIX_UPGRADE_DETECTED must remove all older and equal-version products (inclusive VersionMax, no OnlyDetect or lower bound).')
        }
    }

    $readyDialogId = 'TianCaiSpaceHubReadyDlg'
    $successDialogId = 'TianCaiSpaceHubSuccessDlg'
    $readyCondition = 'UILevel = 5 AND NOT Installed AND NOT REMOVE AND NOT REINSTALL AND NOT UPGRADINGPRODUCTCODE'
    $successCondition = $readyCondition + ' AND &MainFeature = 3'
    $dialogs = @(Read-MsiTable $database 'Dialog' @('Dialog', 'Control_Default', 'Control_Cancel'))
    foreach ($expectedDialog in @(
        @{ Id = $readyDialogId; Default = 'Install'; Cancel = 'Cancel' },
        @{ Id = $successDialogId; Default = 'Finish'; Cancel = 'Finish' }
        @{ Id = 'MsiRMFilesInUse'; Default = 'OK'; Cancel = 'Cancel' }
    )) {
        $matchingDialogs = @($dialogs | Where-Object { $_.Dialog -ceq $expectedDialog.Id })
        if ($matchingDialogs.Count -ne 1 -or
            $matchingDialogs[0].Control_Default -cne $expectedDialog.Default -or
            $matchingDialogs[0].Control_Cancel -cne $expectedDialog.Cancel) {
            $problems.Add("MSI dialog '$($expectedDialog.Id)' must exist once with default '$($expectedDialog.Default)' and cancel '$($expectedDialog.Cancel)'.")
        }
    }

    $controls = @(Read-MsiTable $database 'Control' @('Dialog_', 'Control', 'Type', 'Property', 'Text'))
    $expectedButtons = @(
        @{ Dialog = $readyDialogId; Control = 'Install' },
        @{ Dialog = $readyDialogId; Control = 'Cancel' },
        @{ Dialog = $successDialogId; Control = 'Open' },
        @{ Dialog = $successDialogId; Control = 'Finish' }
        @{ Dialog = 'MsiRMFilesInUse'; Control = 'OK' }
        @{ Dialog = 'MsiRMFilesInUse'; Control = 'Cancel' }
    )
    foreach ($button in $expectedButtons) {
        $matchingControls = @($controls | Where-Object { $_.Dialog_ -ceq $button.Dialog -and $_.Control -ceq $button.Control })
        if ($matchingControls.Count -ne 1 -or $matchingControls[0].Type -cne 'PushButton') {
            $problems.Add("MSI '$($button.Dialog)/$($button.Control)' must exist once as a PushButton.")
        }
    }
    $rmList = @($controls | Where-Object { $_.Dialog_ -ceq 'MsiRMFilesInUse' -and $_.Control -ceq 'List' })
    $rmOptions = @($controls | Where-Object { $_.Dialog_ -ceq 'MsiRMFilesInUse' -and $_.Control -ceq 'ShutdownOption' })
    $rmText = @($controls | Where-Object { $_.Dialog_ -ceq 'MsiRMFilesInUse' -and $_.Control -ceq 'Text' })
    if ($rmList.Count -ne 1 -or $rmList[0].Type -cne 'ListBox' -or $rmList[0].Property -cne 'FileInUseProcess' -or
        $rmOptions.Count -ne 1 -or $rmOptions[0].Type -cne 'RadioButtonGroup' -or $rmOptions[0].Property -cne 'WixUIRMOption' -or
        $properties['WixUIRMOption'] -cne 'UseRM' -or
        $rmText.Count -ne 1 -or $rmText[0].Text -notlike '*不会自动重新打开*') {
        $problems.Add('MSI files-in-use dialog must list registered holders, select close by default and describe no automatic relaunch.')
    }
    $rmButtons = @(Read-MsiTable $database 'RadioButton' @('Property', 'Value', 'Text') | Where-Object { $_.Property -ceq 'WixUIRMOption' })
    if ($rmButtons.Count -ne 2 -or
        @($rmButtons | Where-Object { $_.Value -ceq 'UseRM' -and $_.Text -ceq '关闭应用程序并继续安装(&C)。' }).Count -ne 1 -or
        @($rmButtons | Where-Object { $_.Value -ceq 'DontUseRM' -and $_.Text -ceq '暂不关闭，文件替换可能需要重启电脑(&D)。' }).Count -ne 1) {
        $problems.Add('MSI files-in-use options must match the close-only policy and retain the explicit do-not-close alternative.')
    }

    $uiSequence = @(Read-MsiTable $database 'InstallUISequence' @('Action', 'Condition', 'Sequence'))
    $stepSequences = @{}
    foreach ($action in @('CostFinalize', $readyDialogId, 'ProgressDlg', 'ExecuteAction', $successDialogId)) {
        $matchingSteps = @($uiSequence | Where-Object { $_.Action -ceq $action })
        $sequenceNumber = [int]0
        if ($matchingSteps.Count -ne 1 -or -not [int]::TryParse($matchingSteps[0].Sequence, [ref]$sequenceNumber)) {
            $problems.Add("MSI InstallUISequence must contain exactly one valid '$action' step.")
        } else {
            $stepSequences[$action] = $sequenceNumber
            if ($action -ceq $readyDialogId -and $matchingSteps[0].Condition -cne $readyCondition) {
                $problems.Add('MSI ready dialog must be limited to full-UI new installs and major upgrades.')
            }
            if ($action -ceq $successDialogId -and $matchingSteps[0].Condition -cne $successCondition) {
                $problems.Add('MSI success dialog must require a successful full-UI installation of MainFeature.')
            }
        }
    }
    if ($stepSequences.ContainsKey('CostFinalize') -and $stepSequences.ContainsKey($readyDialogId) -and
        $stepSequences.ContainsKey('ProgressDlg') -and $stepSequences.ContainsKey('ExecuteAction')) {
        if (-not ($stepSequences['CostFinalize'] -lt $stepSequences[$readyDialogId] -and
            $stepSequences[$readyDialogId] -lt $stepSequences['ProgressDlg'] -and
            $stepSequences['ProgressDlg'] -lt $stepSequences['ExecuteAction'])) {
            $problems.Add('MSI UI must show confirmation after costing, then progress, then execute installation.')
        }
    }
    if ($stepSequences.ContainsKey($successDialogId) -and $stepSequences[$successDialogId] -ne -1) {
        $problems.Add('MSI success dialog must be scheduled only on successful exit (sequence -1).')
    }

    $events = @(Read-MsiTable $database 'ControlEvent' @('Dialog_', 'Control_', 'Event', 'Argument', 'Condition', 'Ordering'))
    $expectedEvents = @(
        @{ Dialog_ = $readyDialogId; Control_ = 'Install'; Event = 'EndDialog'; Argument = 'Return'; Condition = '1'; Ordering = '1' },
        @{ Dialog_ = $readyDialogId; Control_ = 'Cancel'; Event = 'EndDialog'; Argument = 'Exit'; Condition = '1'; Ordering = '1' },
        @{ Dialog_ = $successDialogId; Control_ = 'Open'; Event = 'DoAction'; Argument = 'OpenTianCaiSpaceHub'; Condition = $successCondition; Ordering = '1' },
        @{ Dialog_ = $successDialogId; Control_ = 'Open'; Event = 'EndDialog'; Argument = 'Return'; Condition = '1'; Ordering = '2' },
        @{ Dialog_ = $successDialogId; Control_ = 'Finish'; Event = 'EndDialog'; Argument = 'Return'; Condition = '1'; Ordering = '1' }
        @{ Dialog_ = 'MsiRMFilesInUse'; Control_ = 'OK'; Event = 'EndDialog'; Argument = 'Return'; Condition = '1'; Ordering = '1' }
        @{ Dialog_ = 'MsiRMFilesInUse'; Control_ = 'OK'; Event = 'RMShutdownAndRestart'; Argument = '0'; Condition = 'WixUIRMOption~="UseRM"'; Ordering = '2' }
        @{ Dialog_ = 'MsiRMFilesInUse'; Control_ = 'Cancel'; Event = 'EndDialog'; Argument = 'Exit'; Condition = '1'; Ordering = '1' }
    )
    foreach ($expectedEvent in $expectedEvents) {
        $matchingEvents = @($events | Where-Object {
            $_.Dialog_ -ceq $expectedEvent.Dialog_ -and $_.Control_ -ceq $expectedEvent.Control_ -and $_.Event -ceq $expectedEvent.Event
        })
        $eventIsValid = $matchingEvents.Count -eq 1
        if ($eventIsValid) {
            foreach ($name in $expectedEvent.Keys) {
                if ($matchingEvents[0].$name -cne $expectedEvent[$name]) {
                    $eventIsValid = $false
                }
            }
        }
        if (-not $eventIsValid) {
            $problems.Add("MSI '$($expectedEvent.Dialog_)/$($expectedEvent.Control_)' requires the expected '$($expectedEvent.Event)' event, condition and order.")
        }
    }
    foreach ($button in $expectedButtons) {
        $buttonEvents = @($events | Where-Object { $_.Dialog_ -ceq $button.Dialog -and $_.Control_ -ceq $button.Control })
        $expectedCount = if ($button.Control -ceq 'Open' -or ($button.Dialog -ceq 'MsiRMFilesInUse' -and $button.Control -ceq 'OK')) { 2 } else { 1 }
        if ($buttonEvents.Count -ne $expectedCount) {
            $problems.Add("MSI '$($button.Dialog)/$($button.Control)' contains unexpected additional or missing events.")
        }
    }
    $launchEvents = @($events | Where-Object { $_.Event -ceq 'DoAction' -and $_.Argument -ceq 'OpenTianCaiSpaceHub' })
    $executeSequence = @(Read-MsiTable $database 'InstallExecuteSequence' @('Action', 'Condition', 'Sequence'))
    if ($launchEvents.Count -ne 1 -or $launchEvents[0].Dialog_ -cne $successDialogId -or $launchEvents[0].Control_ -cne 'Open' -or
        @($uiSequence | Where-Object { $_.Action -ceq 'OpenTianCaiSpaceHub' }).Count -ne 0 -or
        @($executeSequence | Where-Object { $_.Action -ceq 'OpenTianCaiSpaceHub' }).Count -ne 0) {
        $problems.Add('MSI launcher must run only when the user clicks Open on the successful completion page, never automatically in a sequence.')
    }

    $startupActionId = 'RemoveUserLoginStartup'
    $startupActions = @(Read-MsiTable $database 'CustomAction' @('Action', 'Type', 'Source', 'Target') | Where-Object { $_.Action -ceq $startupActionId })
    # Installed-file EXE (18), deferred (1024), ignore return (64), with user impersonation.
    if ($startupActions.Count -ne 1 -or $startupActions[0].Type -cne '1106' -or
        $startupActions[0].Source -cne 'TianCaiSpaceHubExe' -or $startupActions[0].Target -cne 'remove-login-startup') {
        $problems.Add('MSI login-startup cleanup must invoke the installed EXE once, deferred and impersonated, without failing uninstall on its return code.')
    }
    $startupSequence = @($executeSequence | Where-Object { $_.Action -ceq $startupActionId })
    $removeFilesSequence = @($executeSequence | Where-Object { $_.Action -ceq 'RemoveFiles' })
    $initializeSequence = @($executeSequence | Where-Object { $_.Action -ceq 'InstallInitialize' })
    if ($startupSequence.Count -ne 1 -or $removeFilesSequence.Count -ne 1 -or $initializeSequence.Count -ne 1 -or
        $startupSequence[0].Condition -cne 'Installed AND REMOVE~="ALL" AND NOT UPGRADINGPRODUCTCODE' -or
        [int]$startupSequence[0].Sequence -le [int]$initializeSequence[0].Sequence -or
        [int]$startupSequence[0].Sequence -ge [int]$removeFilesSequence[0].Sequence) {
        $problems.Add('MSI login-startup cleanup must run within the transaction before RemoveFiles, only on full uninstall and never during major upgrade.')
    }
    if (@($uiSequence | Where-Object { $_.Action -ceq $startupActionId }).Count -ne 0 -or
        @($events | Where-Object { $_.Event -ceq 'DoAction' -and $_.Argument -ceq $startupActionId }).Count -ne 0) {
        $problems.Add('MSI login-startup cleanup must not execute from the UI sequence or dialog events.')
    }
} finally {
    try {
        Release-ComObject $database
    } finally {
        Release-ComObject $installer
    }
}

if ($problems.Count -gt 0) {
    throw ("Windows MSI static verification failed:`n - " + ($problems -join "`n - "))
}
Write-Output "Windows MSI static verification passed: TianCaiSpaceHub $fullVersion; EXE $expectedFileVersion; MSI $expectedProductVersion; legacy upgrades, close-only Restart Manager policy, files-in-use, confirmation/completion UI and impersonated uninstall-only startup cleanup before RemoveFiles verified."
