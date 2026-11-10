param([string]$OutputDirectory = $PSScriptRoot)

# Static layout drawings, never a product/browser screenshot. No HTML is run.
# Only creates named figures inside this artifact directory.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$uiFigureRoot = [IO.Path]::GetFullPath($PSScriptRoot)
$uiFigureOutput = [IO.Path]::GetFullPath($OutputDirectory)
if ($uiFigureOutput -ne $uiFigureRoot) { throw 'OutputDirectory must be the artifact directory.' }
$script:svgParts = [Collections.Generic.List[string]]::new()

function Colour([string]$Hex) { [Drawing.ColorTranslator]::FromHtml($Hex) }
function Brush([string]$Hex) { [Drawing.SolidBrush]::new((Colour $Hex)) }
function Draw-UiRect([float]$X,[float]$Y,[float]$W,[float]$H,[string]$Fill,[string]$Stroke = '',[int]$Radius = 0) {
    $b = Brush $Fill
    if ($Radius -gt 0) {
        $path = [Drawing.Drawing2D.GraphicsPath]::new()
        $d = $Radius * 2
        $path.AddArc($X,$Y,$d,$d,180,90)
        $path.AddArc(($X+$W-$d),$Y,$d,$d,270,90)
        $path.AddArc(($X+$W-$d),($Y+$H-$d),$d,$d,0,90)
        $path.AddArc($X,($Y+$H-$d),$d,$d,90,90)
        $path.CloseFigure()
        $script:g.FillPath($b,$path)
        if ($Stroke) { $p=[Drawing.Pen]::new((Colour $Stroke),1);$script:g.DrawPath($p,$path);$p.Dispose() }
        $path.Dispose()
    } else { $script:g.FillRectangle($b,$X,$Y,$W,$H);if($Stroke){$p=[Drawing.Pen]::new((Colour $Stroke),1);$script:g.DrawRectangle($p,$X,$Y,$W,$H);$p.Dispose()} }
    $b.Dispose()
    $strokeAttr = if($Stroke){"stroke='$Stroke'"}else{"stroke='none'"}
    $script:svgParts.Add("<rect x='$X' y='$Y' width='$W' height='$H' rx='$Radius' fill='$Fill' $strokeAttr/>")
}
function T([string]$Text,[float]$X,[float]$Y,[float]$Size=14,[string]$Color='#172B36',[bool]$Bold=$false) {
    $style=if($Bold){[Drawing.FontStyle]::Bold}else{[Drawing.FontStyle]::Regular}
    $font=[Drawing.Font]::new('Microsoft YaHei UI',$Size,$style,[Drawing.GraphicsUnit]::Pixel)
    $b=Brush $Color
    $script:g.DrawString($Text,$font,$b,$X,$Y)
    $b.Dispose();$font.Dispose()
    $safe=[Security.SecurityElement]::Escape($Text)
    $weight=if($Bold){600}else{400}
    $script:svgParts.Add("<text x='$($X+2)' y='$($Y+$Size+1)' font-size='$Size' font-weight='$weight' fill='$Color'>$safe</text>")
}
function L([float]$X,[float]$Y,[float]$W,[string]$C='#E2E8ED') { Draw-UiRect $X $Y $W 1 $C }
function B([string]$Text,[float]$X,[float]$Y,[float]$W=112,[bool]$Primary=$false) {
    $fill=if($Primary){'#087F83'}else{'#FFFFFF'};$ink=if($Primary){'#FFFFFF'}else{'#172B36'}
    Draw-UiRect $X $Y $W 36 $fill $(if($Primary){'#087F83'}else{'#E2E8ED'}) 6
    T $Text ($X+13) ($Y+9) 12 $ink $true
}
function Badge([string]$Text,[float]$X,[float]$Y,[string]$Tone='normal',[int]$W=100) {
    $fill=if($Tone -eq 'ok'){'#EAF6EF'}elseif($Tone -eq 'warn'){'#FFF6E5'}else{'#F4F6F8'}
    $ink=if($Tone -eq 'ok'){'#207457'}elseif($Tone -eq 'warn'){'#94600D'}else{'#586B78'}
    Draw-UiRect $X $Y $W 25 $fill '' 4;T $Text ($X+8) ($Y+5) 10 $ink
}
function Shell([string]$Active,[string]$Breadcrumb) {
    $script:bitmap=[Drawing.Bitmap]::new(1280,840)
    $script:g=[Drawing.Graphics]::FromImage($script:bitmap)
    $script:g.SmoothingMode=[Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $script:g.TextRenderingHint=[Drawing.Text.TextRenderingHint]::AntiAliasGridFit
    $script:g.Clear((Colour '#F4F6F8'))
    $script:svgParts.Clear()
    $script:svgParts.Add("<svg xmlns='http://www.w3.org/2000/svg' width='1280' height='840' viewBox='0 0 1280 840' font-family='Microsoft YaHei UI,Segoe UI,Arial,sans-serif'>")
    Draw-UiRect 0 0 1280 840 '#F4F6F8'
    Draw-UiRect 0 0 1280 36 '#172B36';T '界面方案 02   ·   静态布局示意 / 非运行截图   ·   合成数据' 18 10 11 '#FFFFFF'
    T '详细页面、深浅色和错误场景见同目录 index.html' 925 10 10 '#D5E4E8'
    Draw-UiRect 0 36 214 804 '#FFFFFF';Draw-UiRect 213 36 1 804 '#E2E8ED'
    Draw-UiRect 24 61 35 35 '#087F83' '' 9;T 'T' 33 65 24 '#FFFFFF' $true
    T '天才空间' 69 58 17 '#172B36' $true;T 'TianCaiSpaceHub' 69 82 10 '#586B78'
    $items=@(@('状态概览',126),@('ChatGPT(Codex)',203),@('WorkBuddy',247),@('天工 Work',291),@('模型与渠道',370),@('Skill 管理',412),@('MCP 管理',454),@('聊天连接',496),@('活动记录',538),@('设置',714),@('帮助',758))
    T '客 户 端' 26 180 10 '#71828E';T '共 用 能 力' 26 347 10 '#71828E'
    foreach($it in $items){$name=$it[0];$y=[int]$it[1];$sel=$name -eq $Active;if($sel){Draw-UiRect 14 $y 186 40 '#E8F5F3' '' 7};Draw-UiRect 29 ($y+14) 12 12 $(if($sel){'#087F83'}else{'#71828E'}) '' 3;T $name 54 ($y+10) 12 $(if($sel){'#087F83'}else{'#586B78'}) $sel}
    Draw-UiRect 214 36 1066 57 '#FFFFFF';L 214 92 1066;T $Breadcrumb 246 58 12 '#586B78';Draw-UiRect 1101 63 6 6 '#207457' '' 3;T '本机服务运行中' 1114 56 11 '#586B78'
    Draw-UiRect 214 813 1066 27 '#FFFFFF';L 214 812 1066;T '静态评审图 · 保存、登录、启动、检测均未执行' 246 821 10 '#71828E';T '基线 v0.4.30-13' 1140 821 10 '#71828E'
}
function Heading([string]$Eyebrow,[string]$Title,[string]$Subtitle) { T $Eyebrow 246 118 10 '#087F83' $true;T $Title 246 141 28 '#172B36' $true;T $Subtitle 246 185 13 '#586B78' }
function Card([int]$X,[int]$Y,[int]$W,[int]$H) { Draw-UiRect $X $Y $W $H '#FFFFFF' '#E2E8ED' 10 }
function Field([string]$Label,[string]$Value,[int]$X,[int]$Y,[int]$W=420) { T $Label $X $Y 12 '#586B78';Draw-UiRect $X ($Y+27) $W 36 '#FFFFFF' '#E2E8ED' 6;T $Value ($X+11) ($Y+36) 13 '#172B36' }
function Save-Figure([string]$Name) {
    $script:svgParts.Add('</svg>')
    [IO.File]::WriteAllLines((Join-Path $uiFigureOutput "$Name.svg"),$script:svgParts,[Text.UTF8Encoding]::new($false))
    $script:bitmap.Save((Join-Path $uiFigureOutput "$Name.png"),[Drawing.Imaging.ImageFormat]::Png)
    $script:g.Dispose();$script:bitmap.Dispose()
    Write-Output "$Name.svg / $Name.png"
}

Shell '状态概览' '工作台 / 状态概览'
Heading 'WORKSPACE STATUS' '状态概览' '保留完整连接链路，也让日常打开客户端只需一次操作。'
B '打开 WorkBuddy  →' 1064 144 183 $true
Card 246 231 1001 83
Draw-UiRect 269 256 7 7 '#207457' '' 3;T '本机服务运行中' 285 249 14 '#172B36' $true;T '后台身份已核对 · 刚刚读取 · 合成' 268 280 11 '#586B78'
T '3 个客户端已保存配置' 627 249 14 '#172B36' $true;T '配置与实际调用分别记录' 627 280 11 '#586B78'
T '1 个来源变更待查看' 1007 249 14 '#172B36' $true;T '不自动覆盖客户端独立副本' 1007 280 11 '#586B78'
T '模型与客户端' 246 336 13 '#087F83' $true;T '聊天与执行' 392 336 13 '#586B78';T 'Skill 与 MCP' 517 336 13 '#586B78';L 246 366 1001;Draw-UiRect 246 365 83 2 '#087F83'
Card 246 386 1001 219;T '模型接入链路' 267 401 15 '#172B36' $true;T '各列独立取证' 1133 405 11 '#586B78'
Draw-UiRect 267 433 958 32 '#F4F6F8'
T '客户端' 277 443 11 '#586B78';T '配置' 460 443 11 '#586B78';T '客户端 → Hub' 598 443 11 '#586B78';T '上游 / 实际调用' 817 443 11 '#586B78'
$rowY=478
foreach($r in @(@('ChatGPT(Codex)','模型请求归属待验证','尚无可关联证据','normal'),@('WorkBuddy','查看对应条目请求','最近一次请求完成','ok'),@('天工 Work','本机请求已到达','最近请求失败','warn'))){
 T $r[0] 277 $rowY 13 '#172B36' $true;Badge '已保存' 460 ($rowY-3) 'ok' 72;T $r[1] 598 $rowY 12 '#586B78';Badge $r[2] 817 ($rowY-3) $r[3] 157;T '详情 →' 1163 $rowY 11 '#087F83';if($rowY -lt 560){L 267 ($rowY+28) 958};$rowY+=43
}
Card 246 623 1001 111;T 'ChatGPT(Codex) · 执行通道' 267 638 15 '#172B36' $true;T '接入设置 →' 1135 641 11 '#087F83'
T 'App' 267 676 12 '#172B36' $true;Badge '已观察到连接' 309 671 'ok' 102;T 'VS Code' 518 676 12 '#172B36' $true;Badge '未观察到连接' 591 671 'normal' 109;T 'CLI' 808 676 12 '#172B36' $true;Badge '未观察到连接' 844 671 'normal' 109
T '执行通道、本机模型请求与上游成功是不同状态。' 267 707 11 '#586B78'
Card 246 750 1001 43;T '来源有更新，客户端副本可查看差异；只更新确认过的条目。' 266 763 12 '#586B78';T '查看变化  →' 1137 763 12 '#087F83' $true
Save-Figure 'home-layout'

Shell 'WorkBuddy' '客户端 / WorkBuddy / 模型'
Heading 'CLIENT WORKSPACE' 'WorkBuddy' '日常使用、模型管理和维护各有清晰的位置。'
B '打开 WorkBuddy  →' 1064 144 183
T '状态与使用' 246 229 13 '#586B78';T '模型' 365 229 13 '#087F83' $true;T 'Skill 与 MCP' 433 229 13 '#586B78';T '维护' 558 229 13 '#586B78';L 246 261 1001;Draw-UiRect 365 260 29 2 '#087F83'
Card 246 286 1001 494;T '编辑模型' 271 308 17 '#172B36' $true;Badge '尚未保存' 1137 305 'normal' 86
Field '模型来源' '团队 API · 示例' 272 359 946;T '+ 添加新的来源（完成后返回本草稿）' 272 427 12 '#087F83'
Field '模型' 'gpt-6.1-sol' 272 468 455;Field '思考强度' 'max' 757 468 461
T '按当前模型与协议提供，实际支持由上游决定。' 757 535 11 '#586B78'
L 272 578 946;T '›  协议与高级设置' 272 596 13 '#586B78';T '账号来源只允许 Responses；普通 API 来源保留原协议选择。' 272 630 12 '#586B78'
L 247 707 999;T '仅更新这个客户端的当前条目' 272 733 12 '#586B78';B '取消' 1018 724 80;B '保存更改' 1111 724 109 $true
Save-Figure 'model-layout'

Shell 'MCP 管理' '工作台 / MCP 管理 / Dumpling-MCP'
Heading 'MCP MANAGER' 'Dumpling-MCP' 'MCP 管理中的专用来源：保留环境认证与三端受管连接。'
B '配置说明' 1142 144 105
Card 246 234 236 546;Draw-UiRect 254 242 220 89 '#E8F5F3' '' 7;T '产品环境 · 示例' 269 260 14 '#087F83' $true;T '已授权 · 同一真实身份' 269 292 11 '#586B78';T '+ 添加环境' 269 355 13 '#586B78'
Card 500 234 747 546;T '产品环境 · 示例' 525 257 19 '#172B36' $true;Badge '已授权' 1150 253 'ok' 72;T '环境已保存，认证结果按服务端身份核对' 525 292 12 '#586B78'
T '客户端' 525 342 12 '#586B78';Draw-UiRect 586 330 222 36 '#FFFFFF' '#E2E8ED' 6;T 'ChatGPT(Codex)    ⌄' 599 339 13 '#172B36';B '连接并检测' 818 330 128 $true
$sy=393
foreach($row in @(@('客户端配置','已写入受管连接'),@('本机桥检测','检测通过'),@('工具目录','6 个模块 · 24 个工具（合成）'),@('实际工具调用','尚未验证'))){T $row[0] 525 $sy 12 '#586B78';T $row[1] 688 $sy 13 '#172B36' $true;L 525 ($sy+34) 697;$sy+=53}
T '连接内容概览' 525 616 14 '#172B36' $true;T '少量用途摘要；目录超限时显示“已连通、数量未知”。' 525 645 12 '#586B78'
L 525 685 697;T '› 当前客户端维护：断开 / 撤销' 525 705 12 '#586B78';T '› 环境资料管理：编辑 / 删除整套环境' 525 742 12 '#586B78'
Save-Figure 'mcp-layout'

Shell 'WorkBuddy' '客户端 / WorkBuddy / 模型'
Heading 'CLIENT WORKSPACE' 'WorkBuddy' '当前条目的编辑已保留。'
Card 246 234 1001 546
Draw-UiRect 0 36 1280 777 '#E0E6E9'
# Redraw only a light-weight dialog scene over the inactive workspace.
Card 319 167 745 498;T '交互示例 · 仅合成数据' 345 186 10 '#087F83';T '配置已变化，草稿仍在' 345 211 23 '#172B36' $true;L 320 260 743
T '先核对最新配置，再决定是否用当前草稿重新保存。' 345 285 13 '#586B78'
Draw-UiRect 345 331 693 41 '#F4F6F8';T '字段' 358 343 12 '#586B78';T '开始编辑时' 485 343 12 '#586B78';T '最新保存值' 667 343 12 '#586B78';T '当前草稿' 853 343 12 '#586B78'
T '模型来源' 358 393 13;T '团队 API' 485 393 13;T '备用 API' 667 393 13;T '团队 API' 853 393 13;L 345 424 693
T '思考强度' 358 447 13;T 'high' 485 447 13;T 'high' 667 447 13;T 'max' 853 447 13;L 345 479 693
Draw-UiRect 345 500 693 69 '#E8F5F3' '' 6;T '继续仍需重新读取并核对目标、revision 与后改。' 359 513 12 '#087F83';T '目标已删除或再次变化时停止覆盖，不把旧请求直接重放。' 359 538 12 '#586B78'
L 320 596 743;B '保留草稿，稍后处理' 622 614 176;B '重新核对后保存草稿' 813 614 225 $true
T '静态冲突示意：并非产品已经提供差异比较接口。' 414 706 13 '#586B78'
Save-Figure 'conflict-layout'

Shell 'ChatGPT(Codex)' '客户端 / ChatGPT(Codex) / 接入设置'
Heading 'CLIENT WORKSPACE' 'ChatGPT(Codex)' '模型、接入设置、Skill / MCP、历史与维护各有明确入口。'
B '打开 Codex  →' 1092 144 155
T '状态与使用' 246 229 13 '#586B78';T '模型' 364 229 13 '#586B78';T '接入设置' 434 229 13 '#087F83' $true;T 'Skill 与 MCP' 545 229 13 '#586B78';T '历史与维护' 674 229 13 '#586B78';L 246 261 1001;Draw-UiRect 434 260 55 2 '#087F83'
Card 246 286 610 325;T '受管接入配置' 269 307 17 '#172B36' $true;Badge '配置已保存' 735 305 'ok' 96
T '示例：已是最新，无需重复写入。' 270 352 13 '#586B78';L 270 387 560
T '初始化 / 更新' 270 409 13 '#172B36' $true;T '只补受管接入与缺少的模型控制配置。' 270 437 12 '#586B78';T '保留当前模型、强度、速度选择及退出预检。' 270 463 12 '#586B78'
Draw-UiRect 270 501 560 52 '#F4F6F8' '' 6;T 'MAX / ULTRA / Fast：配置、显示和上游能力分别判断。' 283 513 12 '#586B78';T '不新增强制可用开关。' 283 533 11 '#586B78'
B '查看初始化 / 更新范围' 270 566 195
Card 874 286 373 325;T '模型连接设置' 897 307 17 '#172B36' $true
T '允许 WebSocket' 897 354 14 '#172B36' $true;Draw-UiRect 1187 355 17 17 '#FFFFFF' '#71828E' 3
T '仅保存这一项，不夹带初始化或登录。' 897 385 12 '#586B78';L 897 425 327
T '本机连接方式' 897 448 12 '#586B78';T '标准连接（合成）' 897 478 13 '#172B36';T '本机模式与出站代理在全局设置。' 897 514 11 '#586B78'
B '预览并保存此项' 1062 566 160 $true
Card 246 628 1001 151;T '执行通道与相关入口' 269 647 16 '#172B36' $true
T 'App' 270 685 12 '#172B36' $true;Badge '已观察到连接' 309 680 'ok' 102;T 'VS Code / CLI' 463 685 12 '#172B36' $true;Badge '未观察到连接' 574 680 'normal' 109
L 270 725 952;T '可见模型  → 模型页' 270 745 12 '#087F83';T '账号登录 / 用量 → 模型来源' 510 745 12 '#087F83';T '受管恢复 / 修复 / 历史 → 历史与维护' 838 745 12 '#087F83'
Save-Figure 'codex-settings-layout'

Shell 'Skill 管理' '工作台 / Skill 管理 / 周报整理'
Heading 'SKILL LIBRARY' 'Skill 管理' '技能集中管理，按客户端分别加入和更新。'
B '+ 导入 Skill' 1116 144 131
Draw-UiRect 246 234 1001 59 '#E8F5F3' '' 8;T '规划能力 · 下列适配、作用域和加入结果均为合成示例。' 267 247 13 '#087F83' $true;T '先保存到库，再选择客户端；未确认支持的目标不开放写入。' 267 270 11 '#586B78'
Card 246 311 236 469;Draw-UiRect 254 320 220 99 '#E8F5F3' '' 7;T '周报整理' 269 339 16 '#087F83' $true;T '本地导入 · 1.0 示例' 269 373 11 '#586B78';T '资料研究' 269 448 15 '#172B36' $true;T '仓库导入 · 需要 MCP 依赖' 269 481 11 '#586B78'
Card 500 311 747 469;T '周报整理' 525 336 21 '#172B36' $true;B '加入客户端' 1102 333 120 $true
T '把本周进展整理为清晰的周报，保留问题与下一步。' 525 377 12 '#586B78';Badge 'SKILL.md + references/' 525 410 'normal' 191;Badge '无额外 MCP 依赖' 728 410 'normal' 154
T '各客户端加入情况' 525 462 15 '#172B36' $true;Draw-UiRect 525 496 697 32 '#F4F6F8';T '客户端' 537 505 11 '#586B78';T '加入状态' 751 505 11 '#586B78';T '加载 / 执行' 1001 505 11 '#586B78'
T 'ChatGPT(Codex)' 537 548 13 '#172B36' $true;Badge '已写入（演示）' 751 543 'ok' 122;T '尚未验证加载' 1001 548 12 '#586B78';L 525 578 697
T 'WorkBuddy' 537 598 13 '#172B36' $true;Badge '未加入（演示）' 751 593 'normal' 122;T '—' 1001 598 12 '#586B78';L 525 628 697
T '天工 Work' 537 648 13 '#172B36' $true;Badge '待适配 · 暂不加入' 751 643 'normal' 156;T '确认能力后开放' 1001 648 12 '#586B78'
L 525 688 697;T '选择客户端与范围 → 预览变化 → 分端加入 → 查看加载状态' 525 707 12 '#087F83' $true;T '更新与移除只处理已登记副本；外部后改先比较。' 525 744 11 '#586B78'
Save-Figure 'skills-layout'

Shell 'MCP 管理' '工作台 / MCP 管理 / 服务库'
Heading 'MCP MANAGER' 'MCP 管理' '通用服务与 Dumpling 环境在同一入口，保留各自认证和连接流程。'
B '+ 添加 MCP' 1116 144 131
T '服务库' 246 229 13 '#087F83' $true;T 'Dumpling-MCP' 346 229 13 '#586B78';L 246 261 1001;Draw-UiRect 246 260 41 2 '#087F83'
Card 246 286 236 494;Draw-UiRect 254 294 220 99 '#E8F5F3' '' 7;T '知识库查询' 269 313 16 '#087F83' $true;T '远程服务 · 合成示例' 269 347 11 '#586B78';T '项目文件工具' 269 423 15 '#172B36' $true;T '本地命令 · 导入不执行' 269 456 11 '#586B78'
Card 500 286 747 494;T '知识库查询' 525 309 21 '#172B36' $true;B '加入客户端' 1102 306 120 $true;T '规划能力 · 不代表当前 Hub 已实现任意 MCP 服务接入。' 525 353 12 '#586B78'
Field '远程服务地址 · Streamable HTTP' 'https://mcp.example.com/knowledge' 525 391 697
T '独立认证引用，不复用模型 Key 或 Dumpling 凭据。' 525 465 11 '#586B78'
Draw-UiRect 525 506 697 32 '#F4F6F8';T '客户端' 537 515 11 '#586B78';T '配置状态' 751 515 11 '#586B78';T '实际工具调用' 1001 515 11 '#586B78'
T 'ChatGPT(Codex)' 537 558 13 '#172B36' $true;Badge '已写入（演示）' 751 553 'ok' 122;T '尚未验证' 1001 558 12 '#586B78';L 525 587 697
T 'WorkBuddy' 537 608 13 '#172B36' $true;Badge '未加入（演示）' 751 603 'normal' 122;T '—' 1001 608 12 '#586B78';L 525 637 697
T '天工 Work' 537 658 13 '#172B36' $true;Badge '待适配 · 暂不加入' 751 653 'normal' 156;T '确认协议后开放' 1001 658 12 '#586B78'
L 525 704 697;T 'Dumpling：保留原环境认证与“连接并检测”，不重复接入。' 525 723 12 '#087F83';T '写入成功、客户端加载、桥 / 目录检测、实际调用分别显示。' 525 753 11 '#586B78'
Save-Figure 'mcp-manager-layout'
