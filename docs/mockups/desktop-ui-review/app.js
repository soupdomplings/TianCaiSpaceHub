'use strict';

// Independent review artifact. No fetch, localStorage, product API, or real action.
const icons = {
  home:'<path d="m3 10 9-7 9 7v10a1 1 0 0 1-1 1h-5v-7H9v7H4a1 1 0 0 1-1-1Z"/>',
  layers:'<path d="m12 3 10 5-10 5L2 8Z M2 12l10 5 10-5 M2 16l10 5 10-5"/>',
  chat:'<path d="M21 11a8 8 0 0 1-8 8H7l-5 3 2-6a8 8 0 0 1-1-5 8 8 0 0 1 8-8h2a8 8 0 0 1 8 8Z"/><path d="M7 10h10M7 14h6"/>',
  activity:'<path d="M3 3v18h18M3 13h4l3-7 4 12 3-7h4"/>',
  settings:'<circle cx="12" cy="12" r="3"/><path d="m9 3-1 3-3 1-2 3 2 2-1 3 2 3 3-1 2 3h3l1-3 3-1 2-3-2-2 1-3-2-3-3 1-2-3Z"/>',
  help:'<circle cx="12" cy="12" r="9"/><path d="M9 9a3 3 0 1 1 5 2c-2 1-2 1-2 3M12 17h.01"/>',
  arrow:'<path d="M4 12h16m-6-6 6 6-6 6"/>',
  chevron:'<path d="m9 5 7 7-7 7"/>',
  plus:'<path d="M12 5v14M5 12h14"/>',
  search:'<circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 5 5"/>',
  check:'<path d="m5 12 4 4L20 5"/>',
  info:'<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7h.01"/>',
  warning:'<path d="m12 3 10 18H2Z M12 9v5M12 17h.01"/>',
  close:'<path d="m6 6 12 12M18 6 6 18"/>',
  shield:'<path d="m12 3 8 3v6c0 4-8 9-8 9s-8-5-8-9V6Z"/><path d="m8 12 3 3 5-6"/>',
  plug:'<path d="M8 3v5m8-5v5M5 8h14v2a7 7 0 0 1-7 7v5M5 10a7 7 0 0 0 7 7"/>',
  clock:'<circle cx="12" cy="12" r="9"/><path d="M12 6v6l4 2"/>',
  back:'<path d="m10 5-7 7 7 7M3 12h17"/>',
  book:'<path d="M12 5v16M3 3c5 0 7 1 9 2 2-1 4-2 9-2v16c-5 0-7 1-9 2-2-1-4-2-9-2Z"/>',
  refresh:'<path d="M20 7a9 9 0 0 0-16 2m0 8a9 9 0 0 0 16-2M20 2v5h-5M4 22v-5h5"/>',
  code:'<path d="m8 6-6 6 6 6m8-12 6 6-6 6m-3-15-2 18"/>',
  sliders:'<path d="M4 7h5m4 0h7M4 17h9m4 0h3M9 4v6m4-3a2 2 0 1 1-4 0 2 2 0 0 1 4 0m4 7v6m0-3a2 2 0 1 1-4 0 2 2 0 0 1 4 0"/>'
};
const svg = key => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${icons[key] || icons.info}</svg>`;
const mark = key => clientMarks[key] || svg('plug');
const e = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const btn = (label, action, value='', kind='') => `<button class="btn ${kind}" data-action="${action}" data-value="${e(value)}">${label}</button>`;
const navBtn = (label, page, kind='') => `<button class="btn ${kind}" data-nav="${page}">${label}</button>`;
const badge = (label,tone='') => `<span class="badge ${tone}">${label}</span>`;
const appIcon = (key,large=false) => `<span class="app-icon ${large?'large':''}" aria-hidden="true">${mark(key)}</span>`;
const clients = {codex:'ChatGPT(Codex)',workbuddy:'WorkBuddy',tiangong:'天工 Work'};
const pages = {home:'概览',squad:'天才小分队',codex:'ChatGPT(Codex)',workbuddy:'WorkBuddy',tiangong:'天工 Work',sources:'模型与渠道',skills:'Skill 管理',chat:'聊天接入',mcp:'MCP 管理',logs:'请求记录',settings:'设置',help:'帮助'};
const state = {page:'home',scenario:'ready',clientTab:'overview',overviewTab:'clients',codexWebsocket:false,selectedSkill:'reports',selectedMcp:'docs',mcpView:'library',source:'team',sourceScope:'normal',setting:'appearance',mcpClient:'codex',mcpStage:'connected',mcpVisible:true,wizardClient:'workbuddy',wizardSource:'团队 API · 示例',editedSource:false,modelEditing:false,modelDraft:null,editingSourceId:null,logFilter:'all',detailTab:'result',savedModel:false,codexSaved:false};
let lastFocus;
let toastTimer;
const main = document.getElementById('main');
const dialog = document.getElementById('dialog');
const sourceData = {
  team:{vendor:'openai',name:'团队 API · 示例',models:'gpt-6、gpt-5.6、Claude Sonnet · 示例',count:3,url:'https://api.example.com',protocol:'Responses / Messages',note:'普通来源 · 供客户端显式选择使用'},
  backup:{vendor:'compatible',name:'备用 API · 示例',models:'gpt-6、Claude Sonnet · 示例',count:2,url:'https://backup.example.net/v1',protocol:'OpenAI 兼容',note:'普通来源 · 尚未观察到模型调用'},
  account:{vendor:'chatgpt',authorized:true,name:'ChatGPT 示例账号',models:'gpt-6 · 其他模型待目录确认',count:1,url:'由账号接口管理',protocol:'账号 Responses',note:'账号凭证引用 · 无需填写 API Key'}
};

function hydrate(root=document){
  root.querySelectorAll('[data-icon]').forEach(n=>n.innerHTML=svg(n.dataset.icon));
  root.querySelectorAll('[data-mark]').forEach(n=>n.innerHTML=mark(n.dataset.mark));
  reviewEnhanceSelects(root);
}
function title(kicker,titleText,description,action='',indicator=''){
  return `<div class="page-heading"><div><span class="eyebrow">${kicker}</span><div class="heading-with-status"><h1>${titleText}</h1>${indicator}</div></div><div class="actions">${action}<button class="page-help-button" data-action="page-help" aria-label="本页按钮与使用说明" aria-haspopup="dialog" title="本页使用说明">${svg('help')}</button></div></div>`;
}
function reviewPageGuide(forcedKey=''){
  const make=(name,steps,items,note='')=>({name,steps,items,note});
  const runtime=()=>make('运行状态',['已有配置时打开客户端；首次使用先点“配置客户端”。','观察当前请求链路，点选节点查看详情。'],[
    ['配置客户端','选择客户端和模型，沿引导保存接入；可从已配置模型复用。'],
    ['客户端 / Hub 节点','查看运行、受管配置和本机服务各自的证据。ChatGPT 行内 App / VS Code / CLI 是执行通道，与模型请求分别记录。'],
    ['模型 / 渠道节点','模型和渠道只取进行中请求。点开先看此路由的请求数量，再选具体记录；“查看渠道配置”直达对应普通来源或专用条目。'],
    ['使用统计','切换到用量和趋势；已结束的请求仍可在请求记录中查看。'],
    ['演示状态 / 暂停动效','前者切换本页合成场景；顶部按钮暂停全局动效。聊天、Skill、MCP 和 Dumpling 摘要进入各自管理页。']
  ],'只显示已打开或实际在用的客户端，以及进行中请求的模型和渠道。没有活动客户端时仅留 Hub。绿色运行点不代表上游调用成功；未知保持中性。流动仅表示进行中，终态或旧快照停止。普通接入不推定为 ChatGPT；同名模型、渠道仍按客户端和条目独立。');
  if(forcedKey==='home/runtime')return runtime();
  if(state.page==='squad')return squadGuide();
  if(state.page==='home'){
    if(review3.homeTab==='runtime')return runtime();
    const usage=ReviewUsage.summary(review3Rows());
    return make('使用统计',['选择时间和接入范围。','看趋势，点时段或排行追到具体请求。'],[
    ['今天 / 近 7 天 / 近 30 天','切换统计范围；接入点、来源、模型进一步筛选同一批记录。'],
    ['Token / 请求','切换曲线指标。输入包含缓存，输出单列，不单独统计缓存。'],
    ['曲线时段 / 模型用量 / 接入点分布','点击进入对应请求列表，保留当前筛选与时段。'],
    ['请求结果 / 查看请求记录','结果数量只查看该类记录；右上入口查看当前范围的完整记录。'],
    ['当前范围',`${review3Number(usage.requests)} 个逻辑请求，其中 ${review3Number(usage.running)} 个进行中；完成 ${review3Number(usage.completed)} / ${review3Number(usage.terminal)} 个终态请求。用量已知 ${review3Percent(usage.coverage)}。`],
    ['统计口径 / 重新读取','口径按钮位于此说明底部；读取失败时可重新读取已有记录，不发起模型调用。']
  ],'仅统计当前保留的 Hub 请求记录，普通接入无法确认具体客户端，费用尚未估算。输入和输出单位为 Token，缓存包含在输入中且不重复相加。无用量上报保持未知，完成率只计算终态请求。趋势今天按小时、近 7 / 30 天按天，使用合成时区 UTC+08:00；排行按已知 Token 排序。统计不含首字指标，记录详情仍保留原字段。');
  }
  if(clients[state.page]){
    const key=state.page,name=clients[key],tab=state.clientTab;
    const opening=['打开 '+name,'按该客户端原有启动流程打开；配置、进程、执行连接与实际模型调用分别判断。'];
    if(tab==='models'&&state.modelEditing)return make(name+' · 编辑模型',['选择来源和模型，按需调整参数。','核对当前条目后保存；取消返回模型列表。'],[
      ['添加新的来源','先保存来源，再回到当前模型草稿；保留已填内容。'],['思考强度 / 高级设置','按模型和协议选择；界面可见不表示上游必定支持。'],['保存更改 / 取消','只处理当前客户端的这个条目；外部配置变化时先核对，草稿保留。']
    ]);
    if(tab==='models')return make(name+' · 模型',['从本端“从我的模型加入”打开选择器；也可在另一端模型行选“用于其他客户端”。','需要差异时再调整本端参数，或复用给另一客户端。'],[
      opening,['从我的模型加入','只在当前客户端添加。兼容、无争议的新增直接演示保存；完成后显示使用中；内部自动处理转换与保存。需要重开时给出一个对应操作。'],['调整参数','仅修改当前端的配置；其他端保留各自参数。'],['用于其他客户端','图标上方、名称下方的网格中点击目标，成功后点亮并排在前面；再次点击停用。以当前端参数创建独立配置，目标已有差异时保留其调整；失败保留原状态与顺序。'],
      key==='codex'?['可见模型目录与手动维护','从来源添加、同步渠道模型或获取远端目录先形成草稿；“保存模型列表”才提交可见列表。']:['手动配置模型','填写来源、协议、强度等完整参数；低频协议选项在高级设置中。']
    ],key==='codex'?'Codex 使用普通路由；可见列表不锁定单一渠道，也不保证上游支持全部能力。':'普通来源、当前条目和其他客户端的专用渠道严格隔离。来源更新后选择同步，保留各端参数。');
    if(tab==='connection'&&key==='codex')return make('ChatGPT(Codex) · 接入设置',['首次使用先初始化受管接入，再从 Hub 打开客户端。','需要时单独调整连接选项。'],[
      opening,['初始化接入配置 / 查看初始化范围','先看受管写入范围；保留账号、当前模型选择和退出预检，配置已最新时无需重写。'],['允许 WebSocket / 预览并保存此项','只保存这一项连接偏好，不夹带初始化或账号登录。'],['本机连接设置','进入 Hub 全局本机服务设置；与访问上游的出站代理分别设置。'],['可见模型 / 账号来源 / 历史与维护','分别管理显示列表、来源授权、历史归属和受保护恢复。']
    ],'App / VS Code / CLI 的执行状态与模型链路各自显示证据。增强选项沿用现有规则，上游能力仍需分别验证。');
    if(tab==='extensions')return make(name+' · Skill 与 MCP',['直接选择本端要使用的 Skill 或 MCP。','打开开关使用，关闭开关停用。'],[
      ['添加','选择内容后直接在当前客户端使用。已有内容自动读取，不必重复导入。'],['管理全部 Skill / MCP','查看全部资源、来源及各客户端的使用选择。'],['Dumpling 环境','进入专用环境，沿原 NVWA 授权和连接流程接入当前端。']
    ],'各端按资源类型判断适配。天工已有 Dumpling 接入；通用远程 MCP 是规划演示，Skill 和本地 MCP 格式仍待核实。');
    if(tab==='maintenance')return key==='codex'?make(name+' · 历史与维护',['先选择具体问题，完成只读检查。','查看影响范围与备份后，再确认修复或恢复。'],[
      ['打开修复中心','按启动、模型或组件问题检查；修复前保留退出预检。'],['管理历史','管理 Codex 会话的提供方归属；不迁移天工历史。'],['预览恢复','只恢复受管接入范围，核对备份和外部后改。'],['高级渠道管理','保留低层入口；Codex 模型仍走普通路由，不创建专用副本。']
    ],'历史恢复已有等待与分页重试，持续 EBADF 的根因尚未确认修复。'):make(name+' · 维护',['外部修改后先重新读取配置。','按条目核对备份或专用渠道，再处理维护操作。'],[
      ['重新读取','重新加载已有客户端配置，不发起模型请求。'],['查看备份','核对最近一次操作和后改，按原规则撤销，不覆盖其他条目。'],['选择模型','选择要删除的模型，仅影响本端条目及其专用渠道。'],['查看专用渠道','查看当前端条目的启用、协议与路由；保持条目隔离。']
    ]);
    return make(name+' · 状态概览',['首次使用先接入；已配置可直接打开客户端。','需要新增、复用或调整配置时进入“模型”。'],[
      opening,['状态概览 / 管理模型','标题显示本端短状态；连接图展示客户端、Hub、模型、Skill、MCP 与聊天执行。点击节点查看或配置。'],
      key==='codex'?['接入设置','管理初始化、WebSocket 和 App / VS Code / CLI 执行通道。']:['维护','仅在配置异常或外部修改时进入重新读取、备份和专用渠道管理。'],
      ['Skill 与 MCP','管理当前端的技能、工具服务和 Dumpling 接入。'],['查看聊天接入','管理聊天账号，再在聊天中选择会话、目录、模型和审批。']
    ],key==='workbuddy'?'WorkBuddy 模型接入已实现，/wb 外部任务执行尚未接通。':'实线表示模型路由，只在有请求时流动；虚线表示扩展配置关系。Codex 普通请求不猜本端归属。');
  }
  if(state.page==='sources')return review3.sourceTab==='models'?make('模型与渠道 · 我的模型',['在这里管理模型定义和来源。','到具体客户端的“模型”页添加或调整该端配置。'],[
    ['搜索','按模型或来源查找库内容。'],['模型设置 / 查看来源','查看模型 ID、基础参数与对应来源；客户端分配和独立参数只在客户端页面管理。'],['查看差异','来源变化时逐端选择同步，继续保留各端调整。']
  ],'选定客户端后直接完成配置，跨端复用生成可独立调整的副本。'):make('模型与渠道 · 账号与渠道',['选择厂商及其接入方式。','保存来源，再到客户端选择模型。'],[
    ['添加来源 / 编辑来源','沿用现有八类厂商分支：OpenAI Responses、ChatGPT 账号、Grok、Chat Completions、DeepSeek、Kimi、Anthropic、智谱。账号不填 API Key；智谱区分标准 API / Coding Plan。'],['模型列表 / 目录 / 高级设置','按厂商处理目录、协议、别名、推理兼容和权重；空列表保存先演示获取目录，不发起真实请求。'],['渠道用途 / 专用渠道','按客户端和条目定位配置；从运行图到此页直接选中对应渠道，不借用同名普通来源。'],['当前专用渠道详情','查看完整路由身份、连接与参数；配置未读取时明确提示，不使用其他配置代替。'],['启停、删除与高级路由','保留精确范围、revision、备份与后改保护。']
  ],'厂商差异和配置保护由程序处理；原型中的保存、授权及目录读取均为合成演示。');
  if(state.page==='skills'||state.page==='mcp'&&state.mcpView!=='dumpling'){
    const skill=state.page==='skills',noun=skill?'Skill':'MCP';
    return make(noun+' 管理',['已有配置会自动显示，直接选择在哪些客户端使用。','新增时填写来源、选择客户端，点击“添加并使用”。'],[
      [skill?'添加 Skill':'添加 MCP',skill?'选择目录或仓库，一次添加并使用。':'默认打开完整表单，填写服务名称，选择 Streamable HTTP、STDIO 或 SSE。远程服务填写地址、认证及请求头；本地服务填写启动命令、逐行参数、环境变量、传递变量名称及工作目录。选客户端后添加并使用，不要求选择用途分类。'],...(skill?[]:[['表单配置 / JSON 配置','两种可编辑视图共用草稿；JSON 支持单个服务或 mcpServers。格式有误时保留原文并给出修改入口；多个服务先明确选择，不自动取首项。附加字段与 Windows 路径保留。从已配置客户端复用沿用原身份。添加的是服务，工具由服务提供；Dumpling 继续环境与 NVWA 授权。']]),['客户端使用摘要','点击摘要打开图标网格，每行 2–4 个。配置成功才点亮，使用中的排前面；再点停用成功后变暗。失败保留原状态和顺序；读取失败保留上次选择并暂停修改。'],['点击名称','查看内容、来源、配置或依赖；客户端已有配置也在同一列表里。'],['客户端选择','新增表单中先选择目标，再点添加并使用；成功才成为使用中。从客户端页添加锁定当前端；目标较多时可展开搜索。']
    ],'Skill / 通用 MCP 尚未在产品实现，当前使用开关为合成演示；真实文件格式、作用范围、权限和备份由适配层负责。');
  }
  if(state.page==='mcp')return make('MCP 管理 · Dumpling-MCP',['保存环境，完成对应的 NVWA 登录或授权。','选择目标客户端，再连接并检测。'],[
    ['添加环境 / 编辑环境','密码方式填写账号、密码和记住选项；认证服务填写 ClientID/ClientSecret，勾选共享应用时增加代表账号，取消则浏览器个人授权。'],['高级设置','独立认证地址、MCP 地址/认证头、租户；密码方式的机构、浏览器限定账号、共享应用的签名算法按需显示。'],['保存环境并继续 / 登录授权','环境先保存再认证。主方式切换清理旧输入，共享开关保留应用资料；重新授权不沿用旧成功。'],['客户端 / 连接并检测','只处理所选环境和客户端，按需保存后检测桥与工具目录；客户端位置自动查找。'],['当前客户端维护 / 环境资料管理','单端断开或撤销保留环境及其他端；删除整环境先处理受管连接。']
  ],'环境授权、本机桥、工具目录和客户端真实工具调用是不同状态。Dumpling 保留专用认证与桥接。');
  if(state.page==='chat')return make('聊天接入',['添加平台账号并完成对应认证。','到聊天中选择执行端、会话、目录和模型，再发送任务。'],[
    ['添加聊天接入','先选平台，按其原认证步骤接入。'],['管理','维护当前账号、允许名单和群提及规则；停用或删除分别确认。'],['查看平台使用说明','查看平台支持范围以及会话和审批流程。'],['查看中断示例','任务结果未知时先去客户端核对，再显式新建；不自动重发。']
  ],'账号接入与执行端就绪分别显示。/wb 外部任务尚未接通，天工外部消息的平台范围按现有规则保留。');
  if(state.page==='logs')return make('请求记录',['筛选范围、结果或关键词。','点选具体请求查看原始证据，再决定如何继续。'],[
    ['时间 / 接入点 / 来源 / 模型','沿用统计范围，定位对应客户端入口和渠道。'],['搜索 / 结果筛选 / 翻页','缩小列表范围；取消、断开、未完整结束和失败分别保留。'],['请求详情','查看本条用量、首字、耗时和上游行为；客户端逻辑请求与上游尝试分别判断。'],['来源或条目入口','在详情中进入对应配置，修正后由用户决定是否重新发起任务。'],['记录设置','调整记录范围，清理先确认影响；保留原日志字段和默认值。']
  ],'统计移除首字和单列缓存，实际请求日志与详情保持原样。缺失字段保留未知，不推算成上游账单。');
  if(state.page==='settings'){
    const guides={
      appearance:make('设置 · 外观与语言',['选择主题或语言，按当前产品规则生效。'],[['主题 / 界面语言','产品沿用保存后重启的规则；当前表单用于布局评审。'],['顶部深色预览 / 暂停动效','仅改变这份原型；动效也遵循系统减少动态效果偏好。']]),
      network:make('设置 · 网络与本机服务',['分别核对上游访问和本机访问，再保存。'],[['出站代理','控制访问模型来源的方式。'],['本机连接方式','控制 GUI 访问本机服务的方式，环回问题可选择兼容模式。'],['保存设置','保存本组设置；不启动真实检测或模型调用。']]),
      system:make('设置 · 系统集成',['按遇到的问题选择关联、托盘或诊断入口。'],[['查看关联设置','查看网页导入关联与预览，不自动关联测试包。'],['了解桌面行为','说明关闭到托盘和正式退出的区别。'],['导出连接诊断','按原诊断边界操作；原型只展示说明。']]),
      optional:make('设置 · 可选功能',['按需要显示 Dumpling 来源入口。'],[['显示 Dumpling-MCP','只控制 MCP 管理中的可选入口，不断开客户端、不删除环境，也不隐藏通用服务库。']],'产品默认关闭；原型默认显示供评审。')
    };return guides[state.setting]||guides.appearance;
  }
  return make('帮助中心',['选择与你当前任务对应的帮助。'],[['常用帮助 · 查看','进入接入、可见模型、修复、导入或工具连接的说明与下一步。'],['关于版本 / 导出诊断','查看版本和诊断边界，原型不收集用户配置。'],['整体重构方案 / 完整功能盘点','查看方案范围和既有能力。']],'本原型全部使用合成数据；真实客户端支持与用户验收单独记录。');
}
function openPageGuide(key=''){
  const guide=reviewPageGuide(key);
  const body=`<div class="page-guide"><p class="page-guide-label">推荐顺序</p><ol class="page-guide-steps">${guide.steps.map(step=>`<li>${e(step)}</li>`).join('')}</ol><dl class="page-guide-actions">${guide.items.map(([label,copy])=>`<div><dt>${e(label)}</dt><dd>${e(copy)}</dd></div>`).join('')}</dl>${guide.note?`<p class="page-guide-note">${e(guide.note)}</p>`:''}<p class="page-guide-synthetic">合成原型 · 本说明不会修改配置，关闭后继续当前操作。</p></div>`;
  const details=state.page==='home'&&review3.homeTab==='usage'?btn('详细统计口径','v3-coverage'):state.page==='sources'&&review3.sourceTab==='models'?btn('复用规则','v3-reuse-policy'):'';
  openDialog(guide.name+' · 使用说明',body,details+btn('知道了','close-dialog','','primary'),'本页帮助');
}
function banner(tone,heading,copy,actions=''){
  return `<div class="banner ${tone}">${svg(tone==='error'||tone==='warn'?'warning':'info')}<div class="task-copy"><h3>${heading}</h3><p>${copy}</p></div>${actions?`<div class="actions">${actions}</div>`:''}</div>`;
}
function status(label,value,note=''){
  return `<div class="status-line"><span>${label}</span><div><div class="status-value">${value}</div>${note?`<small>${note}</small>`:''}</div></div>`;
}
function searchInput(id,placeholder){return `<div class="search">${svg('search')}<input id="${id}" aria-label="${placeholder}" placeholder="${placeholder}"></div>`;}
function hint(text){return `<div class="info-line">${svg('info')}<span>${text}</span></div>`;}
function empty(heading,copy,action){return `<div class="card empty-state">${svg('plug')}<h2>${heading}</h2><p>${copy}</p>${action}</div>`;}
function errorBanner(){
  if(state.scenario==='error')return banner('error','当前无法连接模型来源','配置仍已保存。最近一次请求未完成，先检查来源，再回到客户端继续。',navBtn('检查来源','sources'));
  if(state.scenario==='conflict')return banner('warn','配置在其他位置发生了变化','你的编辑仍保留。查看变化后，再决定如何保存。',btn('查看变化','conflict'));
  return '';
}

function home(){return review3Overview();}

function clientPage(){
  const key=state.page,name=clients[key];
  let html=title('CLIENT WORKSPACE',`<span class="heading-title">${appIcon(key)}${name}</span>`,'',btn('打开 '+(key==='codex'?'Codex':name)+' '+svg('arrow'),'open-client',key,state.clientTab==='overview'&&state.scenario!=='empty'?'primary':''),review3ClientHeadline(key));
  html+=`<div class="tabs" role="tablist" aria-label="客户端工作区">${[['overview','状态概览'],['models','模型'],...(key==='codex'?[['connection','接入设置']]:[]),['extensions','Skill 与 MCP'],['maintenance',key==='codex'?'历史与维护':'维护']].map(([v,t])=>`<button role="tab" aria-selected="${state.clientTab===v}" class="tab ${state.clientTab===v?'active':''}" data-action="client-tab" data-value="${v}">${t}</button>`).join('')}</div>`;
  if(state.clientTab==='connection'&&key==='codex')return html+codexConnection();
  if(state.clientTab==='extensions')return html+clientExtensions(key);
  if(state.scenario==='empty')return html+`<div class="review-scene-banner"><span>首次接入场景</span>${btn('查看已配置示例','ui-ready-scene','','link')}</div>`+(state.clientTab==='overview'?review3ClientOverview(key):empty('还没有接入 '+name,'选择模型即可开始配置。',btn('接入 '+name,'setup',key,'primary')));
  html+=errorBanner();
  if(state.clientTab==='models')return html+(key==='codex'?codexModels():clientModels(key));
  if(state.clientTab==='maintenance')return html+maintenance(key);
  return html+review3ClientOverview(key);
}
function clientModels(key){
  if(state.modelEditing)return modelEditor(key);
  return review3ClientModels(key);

}
function modelEditor(key){
  return `<section class="card"><div class="card-header"><h3>编辑模型</h3>${badge('尚未保存')}</div><div class="form-grid"><label class="field full">模型来源<select id="model-source"><option>团队 API · 示例</option><option>备用 API · 示例</option>${key==='workbuddy'?'<option>ChatGPT 示例账号</option>':''}</select><span>${btn('+ 添加新的来源','new-source','return-model','link')}</span></label><label class="field">模型<select id="edit-model-name"><option>gpt-6.1-sol</option><option>Claude · 示例</option></select></label><label class="field">思考强度<select id="edit-effort">${(key==='workbuddy'?['low','medium','high','xhigh','max']:['自动','none','low','high','max']).map(x=>`<option ${x===(key==='workbuddy'?'max':'自动')?'selected':''}>${x}</option>`).join('')}</select></label>${key==='tiangong'?'<label class="field">最大输出<input type="number" value="8192"></label><label class="field">温度<input type="number" value="0.7" step="0.1"></label>':''}</div><details class="subsection"><summary>协议与高级设置</summary><div class="form-grid"><label class="field">接口类型${key==='workbuddy'?'<select id="edit-protocol"><option>openai-responses</option><option>openai-chat</option><option>anthropic-messages</option></select>':'<input value="openai-responses" readonly>'}</label>${key==='tiangong'?'<label class="field">温度策略<select><option>自动适配</option><option>不发送</option><option>保留填写值</option></select></label>':''}</div></details>${state.scenario==='conflict'?banner('warn','外部配置已变化','草稿仍保留。查看变化后重新确认本条目。',btn('查看变化','conflict')):''}<div class="fixed-footer"><div class="actions">${btn('取消','cancel-model')}${btn('保存更改','save-model',key,'primary')}</div></div></section>`;
}
function codexModels(){
  return review3ClientModels('codex')+ `<details class="card catalog-maintenance"><summary>可见模型目录与手动维护</summary><div class="card-header"><h3>在 Codex 中显示的模型</h3>${badge(state.codexSaved?'演示已保存':'3 个已选择',state.codexSaved?'ok':'')}</div><div class="toolbar">${searchInput('codex-search','搜索候选模型')}${btn('从来源添加','codex-add')}</div><div class="table-wrap"><table class="data-table"><thead><tr><th>显示</th><th>模型</th><th>路由来源</th><th>能力说明</th></tr></thead><tbody>${['gpt-6','gpt-5.6','claude-sonnet · 示例'].map((m,i)=>`<tr data-codex-row="${m}"><td><input type="checkbox" checked aria-label="显示 ${m}"></td><td><strong>${m}</strong><small>内置模型目录</small></td><td>团队 API · 示例</td><td>${i?'本地目录定义':'按本地目录'}</td></tr>`).join('')}</tbody></table></div><details class="subsection"><summary>手动模型与高级能力说明</summary><label class="field">手动添加模型 ID<textarea rows="3" placeholder="每行一个模型 ID"></textarea><small>尚无路由的模型在保存前选择来源。高级能力覆盖继续使用配置文件。</small></label></details><div class="fixed-footer">${btn('保存模型列表','save-codex','','primary')}</div></details>`;
}
function maintenance(key){
  return `<div class="split"><section class="card"><div class="card-header"><h3>${key==='codex'?'按问题检查与修复':'配置维护'}</h3></div>${key==='codex'?`<div class="task-row"><div class="task-copy"><h3>启动、模型或组件异常</h3></div>${btn('打开修复中心','repair')}</div><div class="task-row"><div class="task-copy"><h3>历史会话归属</h3></div>${btn('管理历史','history')}</div><div class="task-row"><div class="task-copy"><h3>恢复原接入配置</h3></div>${btn('预览恢复','restore-codex')}</div>`:`<div class="task-row"><div class="task-copy"><h3>重新读取客户端配置</h3></div>${btn('重新读取','reload-config')}</div><div class="task-row"><div class="task-copy"><h3>撤销上次操作</h3></div>${btn('查看备份','undo')}</div><div class="task-row"><div class="task-copy"><h3>删除管理模型</h3></div>${btn('选择模型','delete-model')}</div>`}</section><section class="card"><h3>高级渠道管理</h3><div class="subsection">${btn('查看专用渠道','reserved',key)}</div></section></div>`;
}

function sources(){
  if(review3.sourceTab==='models')return review3Library();
  let html=title('MODEL SOURCES','模型与渠道','管理来源，再将模型用于需要的客户端。',btn(svg('plus')+' 添加来源','new-source','','primary'))+review3SourceTabs();
  if(state.scenario==='empty')return html+empty('添加第一个模型来源','使用 API 地址和密钥，或通过 ChatGPT 账号登录。保存成功后可继续给客户端使用。',btn('添加来源','new-source','','primary'));
  if(review3.channelTarget)return html+review3ChannelDetail(review3.channelTarget);
  html+=`<div class="toolbar">${searchInput('source-search','搜索来源或模型')}<select id="source-scope" aria-label="渠道用途"><option value="normal" ${state.sourceScope==='normal'?'selected':''}>普通来源</option><option value="all" ${state.sourceScope==='all'?'selected':''}>包含客户端专用渠道</option></select></div>`;
  const s={...sourceData[state.source]};
  if(state.source==='team'&&state.scenario==='source-update')s.url='https://api.example.com/gateway';
  html+=`<div class="list-detail"><section class="card item-list" aria-label="来源列表">${Object.entries(sourceData).map(([id,v])=>`<button class="list-item ${state.source===id?'selected':''}" data-action="select-source" data-value="${id}" data-source-row="${e(v.name+' '+v.models)}"><span class="list-title">${svg(v.vendor==='chatgpt'?'shield':'layers')}${e(v.name)}</span><small>${v.count} 个模型 · ${v.vendor==='chatgpt'?'账号授权':'API 接入'}</small><small>${id==='backup'||id.startsWith('demo-source-')?'尚无调用证据':'目录已获取'}</small></button>`).join('')}${state.sourceScope==='all'?`<div class="list-divider"></div>${review3ChannelBindings.map(r=>`<button class="list-item" data-action="v3-route-channel" data-value="${e(review3RouteKey(r))}"><span class="list-title">${e(clients[r.scope])} · ${e(r.model)}</span><small>${e(ReviewUsage.sources[r.source])} · ${e(r.entry)}</small></button>`).join('')}`:''}</section><section class="card"><div class="card-header"><div><h2>${e(s.name)}</h2></div>${btn('编辑来源','edit-source',state.source)}</div>${status('启用状态',badge(s.enabled===false?'未启用':'已启用',s.enabled===false?'':'ok'))}${status('访问地址',`<span class="mono">${e(s.url)}</span>`)}${status('接入方式',e(s.protocol))}${status('上游证据',badge(state.source==='backup'?'尚未验证模型调用':'目录已获取'))}<div class="subsection"><div class="card-header"><h3>模型 <small> / ${s.count} 个</small></h3>${btn('管理模型','edit-source',state.source,'link')}</div><p class="note">${e(s.models)}</p></div><div class="subsection"><h3>使用此来源的客户端</h3><div class="task-row"><div class="task-copy"><strong>${(state.source.startsWith('demo-source-')||state.source.startsWith('squad-demo-'))?'尚未用于客户端':state.source==='account'?'ChatGPT(Codex) / WorkBuddy':'WorkBuddy / 天工 Work'}</strong></div>${(state.source.startsWith('demo-source-')||state.source.startsWith('squad-demo-'))?'':btn('查看条目','source-diff')}</div></div><details class="subsection"><summary>启停、删除与高级路由</summary><div class="actions" style="margin-top:12px">${btn('停用来源','source-disable')}${btn('删除来源','source-delete','','danger')}</div></details></section></div>`;
  return html;
}

function chat(){
  let html=title('CHAT ACCESS','聊天接入','',btn(svg('plus')+' 添加聊天接入','add-chat','','primary'));
  if(state.scenario==='empty')return html+empty('还没有聊天接入','可以先只使用桌面客户端，需要远程使用时再添加。',btn('选择聊天平台','add-chat','','primary'));
  html+=`<section class="card"><div class="card-header"><h3>已连接账号</h3></div><div class="table-wrap"><table class="data-table"><thead><tr><th>账号</th><th>平台状态</th><th>可用执行端</th><th></th></tr></thead><tbody><tr><td><strong>项目助手 · 示例</strong><small>飞书 · 示例账号</small></td><td>${badge('账号已接入','ok')}</td><td>Codex / 天工 Work</td><td>${btn('管理','chat-manage','feishu','link')}</td></tr><tr><td><strong>个人助手 · 示例</strong><small>Telegram · 示例账号</small></td><td>${badge('账号已接入','ok')}</td><td>Codex</td><td>${btn('管理','chat-manage','telegram','link')}</td></tr></tbody></table></div></section><div class="split" style="margin-top:18px"><section class="card"><h3>从聊天中开始一个任务</h3><div class="form-footer">${btn('查看平台使用说明','chat-guide','','link')}</div></section><section class="card note"><h3>执行状态不确定时，先核对</h3>${btn('查看中断示例','unknown-task','','link')}</section></div>`;
  return html;
}

function dumplingMcp(){return reviewDumplingPage();}

function logs(){return review3Records();}

function settings(){
  const group=state.setting;
  let body='';
  if(group==='appearance')body=`<h2>外观与语言</h2><div class="toggle-row"><div>主题</div><select aria-label="主题偏好"><option>跟随系统</option><option>浅色</option><option>深色</option></select></div><div class="toggle-row"><div>界面语言</div><select aria-label="界面语言"><option>简体中文</option><option>English</option></select></div>`;
  if(group==='network')body=`<h2>网络与本机服务</h2><div class="toggle-row"><div>访问模型来源的出站代理</div><select><option>使用系统代理</option><option>直连</option><option>自定义</option></select></div><div class="toggle-row"><div>本机连接方式</div><select><option>标准连接</option><option>VPN 兼容</option></select></div><div class="form-footer">${btn('保存设置','demo-setting','','primary')}</div>`;
  if(group==='system')body=`<h2>系统集成</h2><div class="toggle-row"><div>网页导入</div>${btn('查看关联设置','import-preview')}</div><div class="toggle-row"><div>托盘与窗口</div>${btn('了解桌面行为','tray-info')}</div><div class="form-footer">${btn('导出连接诊断','diagnostics')}</div>`;
  if(group==='optional')body=`<h2>可选功能</h2><label class="toggle-row"><div>在 MCP 管理中显示 Dumpling-MCP</div><input id="mcp-visible" type="checkbox" ${state.mcpVisible?'checked':''}></label>`;
  return title('PREFERENCES','设置','低频设置按实际用途收纳。')+`<div class="setting-layout"><nav class="setting-nav" aria-label="设置分组">${[['appearance','外观与语言'],['network','网络与本机服务'],['system','系统集成'],['optional','可选功能']].map(([v,t])=>`<button data-action="setting-tab" data-value="${v}" class="${group===v?'active':''}">${t}</button>`).join('')}</nav><section class="card">${body}</section></div>`;
}
function help(){
  return title('HELP & MAINTENANCE','从当前任务找到帮助','保留操作上下文，给出能继续的下一步。')+`<div class="split"><section class="card"><h3>常用帮助</h3>${[['接入第一个客户端','从来源到模型再到打开客户端','setup'],['模型没有出现在客户端','核对可见列表和对应入口的路由','codex-add'],['客户端启动或组件异常','先检查具体问题，再确认修复范围','repair'],['导入已有渠道','预览模型与启用范围后保存','import-preview'],['连接工具扩展','保存环境、登录/授权、连接并检测','mcp-help']].map(([h,p,a])=>`<div class="task-row"><div class="task-copy"><h3>${h}</h3><p>${p}</p></div>${btn('查看','help-route',a,'link')}</div>`).join('')}</section><section class="card"><h3>这份评审的范围</h3><p class="note" style="margin-top:12px">原型基于 v0.4.30-13 未提交工作区；所有数据、成功和错误均为合成示例。页面和操作是设计建议。</p><div class="subsection"><div class="actions">${btn('关于版本','about')}${btn('导出诊断','diagnostics')}</div></div><div class="subsection"><a href="../../customizations/desktop-ui-redesign.md">整体重构方案</a><p class="compact-note"><a href="../../customizations/desktop-ui-inventory.md">完整功能与操作盘点</a></p></div></section></div>`;
}

function render(){
  reviewCloseSelect();
  reviewRenderClientNav();
  const renderer={home, squad:squadAccountPage, sources, skills:()=>capabilityLibrary('skills'), chat, mcp:mcpManager, logs, settings, help}[state.page] || clientPage;
  const viewKey=[state.page,state.page==='squad'?[squadReview.tab,squadReview.orgTab,squadReview.billingTab,squadReview.modelTab].join('/'):'',state.clientTab,state.page==='home'?review3.homeTab:'',state.page==='sources'?review3.sourceTab:'',state.setting,state.mcpView].join(':');
  const entering=main.dataset.view!==viewKey;main.dataset.view=viewKey;
  main.innerHTML=`<div class="page-wrap ${entering?'view-enter':''}">${renderer()}</div>`;
  document.querySelectorAll('[data-nav].nav-item').forEach(n=>n.setAttribute('aria-current',n.dataset.nav===state.page?'page':'false'));
  document.getElementById('breadcrumb').textContent=(clients[state.page]?'客户端 / ':'工作台 / ')+pages[state.page];
  const runtimeView=state.page==='home'&&review3.homeTab==='runtime';
  const hubOffline=runtimeView&&review3FlowEvidence().mode==='hub-offline',hubUnknown=runtimeView&&state.scenario==='stale';
  document.getElementById('service-label').textContent=hubOffline?'本机服务不可用 · 合成':hubUnknown?'本机服务状态未知 · 合成':state.scenario==='error'?'本机服务运行中 · 最近请求失败':'本机服务运行中';
  document.getElementById('service-dot').classList.toggle('error',hubOffline);document.getElementById('service-dot').classList.toggle('unknown',hubUnknown);
  if(!state.mcpVisible)state.mcpView='library';
  hydrate(document);
  hydrate(main);
  restoreModelDraft();
  review3Mount();
}
function go(page){
  if(!pages[page])return;
  review3.stopModelReturn=null;
  closeDialog();
  if(page!==state.page){state.clientTab='overview';state.modelEditing=false;}
  state.page=page;
  reviewRememberClient(page);
  if(location.hash!==`#${page}`)history.replaceState(null,'',`#${page}`);
  render();main.scrollTop=0;
}
function showToast(message){
  const t=document.getElementById('toast');t.textContent=message;t.hidden=false;
  clearTimeout(toastTimer);toastTimer=setTimeout(()=>t.hidden=true,5500);
}
function openDialog(titleText,body,footer,kicker='交互示例 · 仅合成数据'){
  reviewCloseSelect();
  if(!dialog.open)lastFocus=document.activeElement;
  document.getElementById('dialog-title').textContent=titleText;
  document.getElementById('dialog-kicker').textContent=kicker;
  document.getElementById('dialog-body').innerHTML=body;
  document.getElementById('dialog-footer').innerHTML=footer || btn('知道了','close-dialog','','primary');
  hydrate(dialog);if(!dialog.open)dialog.showModal();
}
function closeDialog(){reviewCloseSelect();if(dialog.open)dialog.close();}
dialog.addEventListener('close',()=>{if(lastFocus?.isConnected)lastFocus.focus();});
dialog.addEventListener('click',ev=>{if(ev.target===dialog){const r=dialog.getBoundingClientRect();if(ev.clientX<r.left||ev.clientX>r.right||ev.clientY<r.top||ev.clientY>r.bottom)closeDialog();}});

function setup(client){
  if(client&&clients[client]){state.wizardClient=client;wizardModel();return;}
  openDialog('先选择要用的客户端',`<div class="stepper"><span class="step active"><b>1</b>选客户端</span><span class="rule"></span><span class="step"><b>2</b>选模型</span><span class="rule"></span><span class="step"><b>3</b>保存接入</span></div><p>选择一次，后面的来源和模型配置会保留这个上下文。</p>${reviewTargetField('wizard-target',[state.wizardClient],{single:true})}<p class="compact-note">配置修改会在确认后逐阶段提交，Hub 不发送推理测试。</p>`,btn('取消','close-dialog')+btn('继续：选择模型 '+svg('arrow'),'wizard-model','','primary'));
}
function wizardModel(){
  openDialog('为 '+clients[state.wizardClient]+' 选择模型',`<div class="stepper"><span class="step done"><b>✓</b>客户端</span><span class="rule"></span><span class="step active"><b>2</b>选模型</span><span class="rule"></span><span class="step"><b>3</b>保存接入</span></div><div class="form-grid"><label class="field full">模型来源<select id="wizard-source"><option>${e(state.wizardSource)}</option><option>备用 API · 示例</option></select><span>${btn('+ 添加新的来源','new-source','wizard','link')}</span></label><label class="field full">模型<select id="wizard-model-name"><option>gpt-6.1-sol</option><option>Claude · 示例</option></select></label></div>${hint('这里只列适用来源。天工不包含 ChatGPT 账号，专用条目不能作为其他客户端的来源。')}`,btn('返回','setup')+btn('查看接入内容','wizard-review','','primary'));
}
function wizardReview(){
  state.wizardSource=document.getElementById('wizard-source')?.value||state.wizardSource;
  openDialog('确认接入 '+clients[state.wizardClient],`<p>确认后只保存这个客户端需要的配置。打开客户端是下一步单独操作。</p>${status('模型来源',e(state.wizardSource))}${status('模型','gpt-6.1-sol')}${status('将要修改',state.wizardClient==='codex'?'可见模型列表与受管客户端配置，分别提交':'一个管理模型条目和对应的独立渠道')}${status('保留内容','其他模型、未知字段、原有配置与备份保护')}<details class="subsection"><summary>高级信息与写入边界</summary><p class="note">使用现有 revision、文件锁和受保护备份。来源已保存但客户端保存失败时，保留来源和草稿继续，不能假装跨文件统一事务。</p></details>`,btn('返回修改','wizard-model')+btn('确认保存','wizard-save','','primary'));
}
// Templates mirror the current native vendor branches, not one generic API form.
const sourceTemplates={
  openai:{label:'OpenAI Responses',protocol:'openai-responses',url:'https://api.openai.com/v1',models:'gpt-6'},
  chatgpt:{label:'ChatGPT 账号',protocol:'chatgpt-responses',url:'由账号授权管理',models:'gpt-6'},
  grok:{label:'Grok Responses',protocol:'grok-responses',url:'https://api.x.ai/v1',models:'grok · 示例'},
  compatible:{label:'OpenAI Chat Completions',protocol:'chat-completions',url:'https://api.openai.com/v1',models:'自定义模型 · 示例'},
  deepseek:{label:'DeepSeek Responses',protocol:'deepseek-responses',url:'https://api.deepseek.com/v1',models:'deepseek-v4-pro'},
  kimi:{label:'Kimi Responses',protocol:'kimi-responses',url:'https://api.moonshot.cn/v1',models:'kimi-k3'},
  anthropic:{label:'Anthropic Messages',protocol:'anthropic-messages',url:'https://api.anthropic.com/v1',models:'claude-sonnet · 示例'},
  zai:{label:'智谱 Anthropic',protocol:'anthropic-messages / glm_anthropic',url:'https://open.bigmodel.cn/api/anthropic',models:'GLM · 示例'}
};
function sourceEditor(id,returnTo=''){
  state.editingSourceId=id||null;
  if(returnTo==='return-model')captureModelDraft();
  const old=sourceData[id],type=old?.vendor||(id==='account'?'chatgpt':'openai'),preset=sourceTemplates[type];
  state.sourceEdit={id,returnTo,type,drafts:{[type]:{name:old?.name||preset.label+' · 示例',url:old?.url||preset.url,key:'DEMO-ONLY-NOT-A-KEY',models:old?.models||'',modelsUrl:old?.modelsUrl||'',priority:old?.priority||'1',access:old?.access||'api',disableReasoning:old?.disableReasoning||false,authorized:!!old?.authorized}}};
  drawSourceEditor();
}
function captureSourceDraft(){
  const edit=state.sourceEdit;if(!edit)return;
  const d=edit.drafts[edit.type];
  for(const [id,key] of [['source-name','name'],['source-url','url'],['source-key','key'],['source-models','models'],['source-models-url','modelsUrl'],['source-priority','priority'],['source-zai-access','access']]){
    const input=document.getElementById(id);if(input&&!(edit.type==='chatgpt'&&['url','key','modelsUrl','access'].includes(key)))d[key]=input.value;
  }
  if(edit.type==='compatible')d.disableReasoning=!!document.getElementById('source-disable-reasoning')?.checked;
}
function changeSourceVendor(type){
  if(!sourceTemplates[type])return;
  captureSourceDraft();const edit=state.sourceEdit;edit.type=type;
  edit.drafts[type]??={name:sourceTemplates[type].label+' · 示例',url:sourceTemplates[type].url,key:'DEMO-ONLY-NOT-A-KEY',models:'',modelsUrl:'',priority:'1',access:'api',disableReasoning:false,authorized:false};
  drawSourceEditor();
}
function drawSourceEditor(){
  const edit=state.sourceEdit,d=edit.drafts[edit.type],preset=sourceTemplates[edit.type],account=edit.type==='chatgpt';
  const vendorSelect=`<label class="field full">厂商与接入方式<select id="source-vendor" ${edit.id&&account?'disabled':''}>${Object.entries(sourceTemplates).filter(([id])=>!edit.id||account||id!=='chatgpt').map(([id,v])=>`<option value="${id}" ${id===edit.type?'selected':''}>${v.label}</option>`).join('')}</select></label>`;
  const api=account?`<div class="field full"><span>账号授权</span>${badge(d.authorized?'演示已授权':'待授权',d.authorized?'ok':'warn')}<div>${btn('演示账号授权','source-demo-authorize')}</div></div>`:`<label class="field full">服务地址<input id="source-url" value="${e(d.url)}"></label><label class="field full">API Key<input id="source-key" value="${e(d.key)}" autocomplete="off"><small>合成原型，请勿输入真实密钥。</small></label>`;
  const special=edit.type==='zai'?`<label class="field full">智谱接入模式<select id="source-zai-access">${review3Options({api:'标准 API',coding:'Coding Plan'},d.access)}</select></label>`:edit.type==='compatible'?`<label class="checkbox full"><input id="source-disable-reasoning" type="checkbox" ${d.disableReasoning?'checked':''}><span>此渠道关闭推理</span></label>`:'';
  const advanced=`<details class="subsection"><summary>模型映射与高级设置</summary>${status('处理方式',e(preset.protocol))}${!account?`<label class="field">独立模型目录地址<input id="source-models-url" value="${e(d.modelsUrl)}" placeholder="留空使用厂商默认目录"></label>`:''}<label class="field">路由优先级<input id="source-priority" type="number" min="1" step="1" value="${e(d.priority)}"></label>${hint('别名映射、厂商目录筛选、推理兼容和超时沿用当前 Hub 的对应处理；不会因选择模板将专用副本改为普通渠道。')}</details>`;
  openDialog(edit.id?'编辑模型来源':'添加模型来源',`<div class="form-grid source-editor-grid">${vendorSelect}<label class="field full">来源名称<input id="source-name" value="${e(d.name)}"></label>${special}${api}<label class="field full">模型列表<textarea id="source-models" rows="3" placeholder="留空时保存将演示获取目录">${e(d.models)}</textarea></label></div><div class="actions">${btn('获取模型目录（演示）','source-demo-directory')}</div><div id="source-validation" class="field-error" role="status"></div>${advanced}`,btn('取消','close-dialog')+btn('保存并继续（演示）','save-source',edit.returnTo||edit.id,'primary'));
}
function saveSourceEditor(useDraft=false){
  if(!useDraft)captureSourceDraft();
  const edit=state.sourceEdit,d=edit.drafts[edit.type],account=edit.type==='chatgpt';
  const validUrl=value=>{try{return ['http:','https:'].includes(new URL(value).protocol);}catch{return false;}};
  const error=!d.name.trim()?'请填写来源名称。':!Number.isInteger(Number(d.priority))||Number(d.priority)<1?'路由优先级须为正整数。':account&&!d.authorized?'先完成演示账号授权；账号分支不使用 API Key。':!account&&!validUrl(d.url)?'请填写完整的 HTTP(S) 服务地址。':!account&&d.modelsUrl&&!validUrl(d.modelsUrl)?'独立目录地址须为完整 HTTP(S) 地址。':'';
  if(error){document.getElementById('source-validation').textContent=error;return;}
  if(state.scenario==='conflict'){openDialog('来源配置已变化，草稿仍保留',status('来源',e(d.name))+status('厂商',e(sourceTemplates[edit.type].label))+hint('先重新读取目标并比较；重核本身不写入。此处仅为合成冲突场景。'),btn('返回编辑','source-return-editor')+btn('重新核对','source-recheck','','primary'));return;}
  if(!d.models.trim())d.models=sourceTemplates[edit.type].models;
  const ids=d.models.split(/[,，、\n]/).map(x=>x.trim()).filter(Boolean);
  const id=edit.id||'demo-source-'+(Object.keys(sourceData).length+1);
  sourceData[id]={name:d.name.trim(),url:account?'由账号授权管理':d.url.trim(),models:ids.join('、'),count:ids.length,protocol:sourceTemplates[edit.type].protocol,vendor:edit.type,access:d.access,disableReasoning:d.disableReasoning,modelsUrl:account?'':d.modelsUrl,priority:d.priority,authorized:account&&d.authorized,note:'普通来源 · 合成配置'};
  state.wizardSource=d.name.trim();review3.channelTarget=null;review3.sourceTab='channels';
  if(edit.returnTo==='wizard'){wizardModel();showToast('演示：来源保存到原型内存，继续配置客户端。');}
  else if(edit.returnTo==='return-model'){if(state.modelDraft)state.modelDraft.source=d.name.trim();closeDialog();render();showToast('演示：保留模型草稿，并选择新来源。');}
  else{state.source=id;closeDialog();state.scenario='ready';document.getElementById('scenario').value='ready';go('sources');showToast('仅保存到原型内存，未添加真实渠道或修改客户端副本。');}
}
function conflict(){
  openDialog('配置已变化，草稿仍在',`<p>这是来源或条目的并发修改示例。先核对最新配置，再决定是否用当前草稿重新保存。</p><div class="table-wrap" style="margin-top:18px"><table class="data-table"><thead><tr><th>字段</th><th>开始编辑时</th><th>最新保存值</th><th>当前草稿</th></tr></thead><tbody><tr><td>模型来源</td><td>团队 API</td><td>备用 API</td><td>团队 API</td></tr><tr><td>思考强度</td><td>high</td><td>high</td><td>max</td></tr></tbody></table></div>${hint('继续仍需在后台重读并核对目标；若目标已删除或再次变化，停止覆盖。不是把旧请求换个 revision 自动重放。')}`,btn('保留草稿，稍后处理','close-dialog')+btn('重新核对后保存草稿','resolve-conflict','','primary'));
}
function sourceDiff(){
  openDialog('来源更新，只同步你确认的条目',`<p>WorkBuddy / 条目 1 使用来源的独立副本。普通来源保存不会自动修改它。</p>${status('目标模型','gpt-6.1-sol · WorkBuddy 条目 1')}${status('来源','团队 API · 示例')}${status('变化摘要','模型映射发生变化，凭据显示为“已更换”时不回传原始秘密')}${status('不受影响','其他 WorkBuddy 条目、天工条目、普通 Codex 路由')}<div class="banner info" style="margin-top:18px;margin-bottom:0">${svg('info')}<div class="task-copy"><h3>差异摘要是新增设计</h3><p>实际产品须补充受保护的只读摘要；当前 API 没有统一差异字段。</p></div></div>`,btn('暂不更新','close-dialog')+btn('确认更新这个条目','apply-source-diff','','primary'));
}
function captureModelDraft(){
  const field=id=>document.getElementById(id)?.value;
  if(document.getElementById('model-source'))state.modelDraft={source:field('model-source'),model:field('edit-model-name'),effort:field('edit-effort'),protocol:field('edit-protocol')};
}
function restoreModelDraft(){
  if(!state.modelEditing||!state.modelDraft)return;
  for(const [id,key] of [['model-source','source'],['edit-model-name','model'],['edit-effort','effort'],['edit-protocol','protocol']]){
    const control=document.getElementById(id);const value=state.modelDraft[key];
    if(control&&value){
      if(![...control.options].some(o=>o.value===value)){const option=document.createElement('option');option.value=value;option.textContent=value;control.append(option);}
      control.value=value;
    }
  }
}
function logDetail(id){
  const fail=id==='1041';
  openDialog('请求 #'+id+' · '+(fail?'模型未配置':'完成'),`${fail?banner('error','模型不在此条目的适用列表中','本机请求已到 Hub，但没有选择到合适上游。保留天工条目范围，不切换其他渠道。'):banner('info','最近一次请求完成','这个合成记录只表达一次结果，不证明所有模型能力均可用。')}<div class="tabs" role="tablist" aria-label="请求详情"><button class="tab ${state.detailTab==='result'?'active':''}" data-action="detail-tab" data-value="${id}:result">结果</button><button class="tab ${state.detailTab==='client'?'active':''}" data-action="detail-tab" data-value="${id}:client">客户端请求</button><button class="tab ${state.detailTab==='upstream'?'active':''}" data-action="detail-tab" data-value="${id}:upstream">实际上游</button></div>${state.detailTab==='result'?`${status('请求入口',fail?'天工 / 条目 1':'WorkBuddy / 条目 1')}${status('客户端响应模式','非流式')}${status('实际上游模式',fail?'未知 · 未到达上游':'流式 → Hub 聚合后返回非流式')}${status('最终结果',fail?'HTTP 422 · invalid_model':'completed · 1.84 s · 合成示例')}`:`<p class="note">${state.detailTab==='client'?'保留客户端请求的原始 stream 语义。':'上游头、请求体、SSE 与客户端响应分别查看；旧日志字段缺失时为未知。'}</p><pre class="code-block">${e(state.detailTab==='client'?'{\n  "model": "gpt-6.1-sol",\n  "stream": false,\n  "messages": "[合成内容]"\n}':fail?'没有可用的上游请求证据。':'{\n  "upstreamStream": true,\n  "status": "completed",\n  "headers": "[合成摘要]"\n}')}</pre>`}`,btn('关闭','close-dialog')+(fail?btn('检查这个天工条目','fix-entry','','primary'):btn('查看记录设置','log-settings')));
}

const infoDialogs = {
  'service':['本机服务与连接状态','状态来自当前 Hub 后台身份。若不可用，先展示自动恢复进度，再提供重新连接、本机连接设置和技术详情。这里未读取或启动实际服务。'],
  'account-flow':['保留 ChatGPT 账号接入能力','登录、打开授权网页、取消、导入 auth.json、用量刷新与模型获取仍走现有账号模块。账号凭据不作为普通 API Key 使用；本原型不会打开登录或读取文件。'],
  'mcp-help':['Dumpling-MCP 配置说明','先配置环境并保存，再登录/授权，最后选择客户端连接并检测。密码方式提供账号、密码、记住密码；认证服务提供 ClientID、ClientSecret 和保存应用密钥，共享开关决定代表账号或浏览器个人授权。独立认证地址、MCP 路径/头、租户及条件机构/限定账号/签名算法在高级设置中。正式版沿用原有挑战、系统秘密存储和同身份保护；此原型仅使用示例资料。'],
  'history':['Codex 历史归属管理','保留其他 provider / AI Gateway 两组列表、多选归入/移回、目标 provider、刷新与右键菜单。这里只调整归属，不修复持续 EBADF 根因，不涉及天工历史。'],
  'undo':['撤销上次配置操作','先列目标条目和备份范围，再检查是否发生后改。只撤销最近一次保存/删除，不影响其他条目。冲突时提供查看备份，不覆盖最新修改。'],
  'reload-config':['重新读取配置','页面进入和操作成功后自动只读加载。手工重新读取仅供维护；有草稿时保留草稿并提示外部变化。原型不读取本地客户端文件。'],
  'tray-info':['桌面与后台','普通关闭隐藏到托盘，显式退出和系统结束会话保留原生语义。仅清理 Hub 自有后台，不结束独立 WorkBuddy/天工任务。此处没有实际改变窗口或后台。'],
  'diagnostics':['导出连接诊断','产品继续使用原生保存对话框和既有异步导出。导出不自动上传。本轮原型不读取诊断、用户配置或日志，也不打开文件选择器。'],
  'about':['TianCaiSpaceHub · 界面评审','产品基线为 v0.4.30-13 未提交工作区；本轮不升版本、不构建安装包、不提交、不推送、不发布。原型页面不是已交付产品。'],
  'codex-settings':['Codex 连接设置','保留 WebSocket 开关、本机连接模式、受管初始化/更新/恢复。配置状态已最新时不用重复写入；增强启动仍受原预检和运行实例规则约束。'],
  'demo-directory':['目录获取示例','已显示合成目录。实际实现延续原地址优先和 v13 有效成功后修正的规则，不在本轮发出目录请求或推理调用。'],
  'chat-guide':['在聊天中继续','Codex 沿用平台会话入口。天工在支持的平台发送 /tg，新建/恢复时选择目录和模型；待审批在同一聊天处理。/q 不表示取消任务，/s 不能冒充天工已停止。'],
  'delete-model':['选择删除的模型','只删除当前管理条目和精确对应的专用渠道，展示受影响客户端及备份范围。原生实现继续检查 revision，不按模型名删除其他同名条目。'],
  'mcp-undo':['撤销当前客户端上次连接更改','预览目标和备份、核对归属及后改，然后才调用已有 restore。不会触及其他客户端，也不是恢复整份客户端配置。']
};

function handleAction(action,value,button){
  if(squadAction(action,value))return;
  if(reviewRefinementAction(action,value))return;
  if(review3Action(action,value))return;
  if(capabilityAction(action,value))return;
  if(infoDialogs[action]){openDialog(...infoDialogs[action]);return;}
  switch(action){
    case 'page-help':openPageGuide();break;
    case 'close-dialog':closeDialog();break;
    case 'setup':setup(value);break;
    case 'wizard-client':state.wizardClient=value;setup();break;
    case 'wizard-model':wizardModel();break;
    case 'wizard-review':wizardReview();break;
    case 'wizard-save':
      if(state.scenario==='error')openDialog('来源已保存，客户端配置尚未完成',banner('warn','草稿仍保留','修正客户端状态后可继续，不删除已保存来源，也不自动重放未知写入。'),btn('返回检查','wizard-model')+btn('查看客户端','wizard-done','','primary'));
      else openDialog('接入配置已保存（演示）',`${status('模型来源','已保存')}${status('客户端配置','已保存')}${hint('这里只说明配置已写入；上游与实际调用仍需按各自证据显示。')}`,btn('查看客户端','wizard-done')+btn('打开 '+clients[state.wizardClient],'open-client',state.wizardClient,'primary'));break;
    case 'wizard-done':document.getElementById('scenario').value='ready';state.scenario='ready';go(state.wizardClient);break;
    case 'client-tab':closeDialog();state.clientTab=value;state.modelEditing=false;render();break;
    case 'edit-model':state.modelEditing=true;state.modelDraft=null;render();break;
    case 'cancel-model':state.modelEditing=false;state.modelDraft=null;render();break;
    case 'save-model':if(state.scenario==='conflict'){conflict();break;}state.savedModel=true;state.modelEditing=false;render();showToast('演示：只保存当前条目。没有修改产品或客户端配置。');break;
    case 'save-codex':state.codexSaved=true;render();showToast('演示：可见列表已保存，未调用实际 API。');break;
    case 'codex-add':openDialog('从来源添加可见模型',`<p>同步已保存渠道模型、从所选来源获取远端目录、或手动输入均保留。获取/同步只改变草稿，保存可见列表才落盘。</p><label class="field" style="margin-top:18px">普通来源<select><option>团队 API · 示例</option><option>备用 API · 示例</option></select></label><div class="actions" style="margin-top:18px">${btn('同步渠道模型','codex-candidate')}${btn('获取远端目录','codex-candidate')}</div>`,btn('返回','close-dialog'));break;
    case 'codex-candidate':closeDialog();go('codex');state.clientTab='models';render();showToast('演示：候选已加入草稿，仍需保存模型列表。');break;
    case 'select-source':review3.channelTarget=null;state.source=value;review3.sourceTab='channels';render();break;
    case 'new-source':sourceEditor('',value);break;
    case 'edit-source':sourceEditor(value);break;
    case 'save-source':saveSourceEditor();break;
    case 'source-return-editor':drawSourceEditor();break;
    case 'source-recheck':state.scenario='ready';document.getElementById('scenario').value='ready';openDialog('重新核对完成',status('待保存来源',e(state.sourceEdit.drafts[state.sourceEdit.type].name))+hint('合成核对未发现新的冲突。确认后才保存到原型内存；不写入真实配置。'),btn('返回编辑','source-return-editor')+btn('确认演示保存','source-commit','','primary'));break;
    case 'source-commit':saveSourceEditor(true);break;
    case 'source-demo-authorize':captureSourceDraft();state.sourceEdit.drafts.chatgpt.authorized=true;drawSourceEditor();break;
    case 'source-demo-directory':captureSourceDraft();state.sourceEdit.drafts[state.sourceEdit.type].models=sourceTemplates[state.sourceEdit.type].models;drawSourceEditor();showToast('仅显示合成模型目录，未访问厂商。');break;
    case 'conflict':conflict();break;
    case 'resolve-conflict':closeDialog();state.scenario='ready';document.getElementById('scenario').value='ready';state.modelEditing=false;render();showToast('演示：重新核对后保存草稿。产品仍须校验最新 revision 与目标身份。');break;
    case 'source-diff':review3SourceUpdate(state.source==='backup'?'claude':state.source==='account'?'account':'gpt');break;
    case 'apply-source-diff':state.editedSource=true;closeDialog();go('workbuddy');state.clientTab='models';render();showToast('演示：只更新 WorkBuddy 条目 1。其他副本未改。');break;
    case 'source-disable':case 'source-delete':openDialog(action==='source-delete'?'删除此普通来源？':'停用此普通来源？','<p>影响普通入口的候选路由。专用副本仍独立；先核对使用范围。这里仅展示确认，点击不会操作实际配置。</p>',btn('取消','close-dialog')+btn('确认（仅演示）','confirm-demo','','danger'));break;
    case 'reserved':openDialog(clients[value]+' 的专用渠道',`<p>保留启停、协议/路由低层编辑和直接删除入口，显示具体条目和影响范围。建议正常配置通过客户端模型页进行；内部名称仍不可误改。</p>${status('用途',clients[value]+' 专用')}${status('路由','只选择对应条目，不跨客户端回退')}`,btn('返回','close-dialog')+btn('前往客户端模型','go-models',value,'primary'));break;
    case 'go-models':go(value);state.clientTab='models';render();break;
    case 'open-client':openDialog('打开 '+clients[value],`${status('客户端配置','按原流程只读检查')}${status('启动方式',value==='codex'?'使用 Hub 增强启动，保留退出预检':'识别现有实例，避免重复启动')}${hint('原型不会启动任何程序，也不发送模型请求。实际已运行时不承诺当前 API 能把窗口前置。')}`,btn('取消','close-dialog')+btn('演示启动反馈','simulate-open',value,'primary'));break;
    case 'simulate-open':closeDialog();showToast('演示：已发起打开 '+clients[value]+'。未启动实际程序，未验证模型调用。');break;
    case 'add-chat':openDialog('选择聊天平台',`<p>每个平台保留自己的认证与引导。先选平台，再在同一流程完成接入。</p><div class="client-picks" style="grid-template-columns:repeat(2,1fr)">${['飞书','Telegram','微信','企业微信'].map(p=>`<button class="pick" data-action="chat-platform" data-value="${p}">${svg('chat')}<p style="margin-top:8px">${p}</p></button>`).join('')}</div>`,btn('取消','close-dialog'));break;
    case 'chat-platform':openDialog('接入 '+value,value==='Telegram'?'<p>输入 Token 后保存并连接；错误时保留表单。</p><label class="field" style="margin-top:18px">Token<input placeholder="仅合成占位，不输入真实 Token"></label>':'<p>这里呈现平台二维码、有效期和状态，保留浏览器兜底、验证码或重新获取入口。评审原型不生成真实登录二维码。</p><div class="empty-state" style="padding:25px">'+svg('chat')+'<p>二维码 / 验证步骤占位</p></div>',btn('返回平台','add-chat')+btn('演示接入完成','chat-done','','primary'));break;
    case 'chat-done':closeDialog();state.scenario='ready';document.getElementById('scenario').value='ready';go('chat');showToast('演示：平台账号已接入。执行端仍单独准备。');break;
    case 'chat-manage':openDialog('管理聊天账号',`${status('平台',value==='feishu'?'飞书':'Telegram')}${status('范围','允许名单和原有群提及规则保持')}${status('会话','在平台内新建/恢复，不在 Hub 创建平行会话')}`,btn('停用账号（演示）','confirm-demo')+btn('删除账号（演示）','confirm-demo','','danger')+btn('返回','close-dialog'));break;
    case 'unknown-task':openDialog('任务执行结果未知',banner('warn','请先到天工桌面核对','请求已经进入可能执行的阶段。断开、切换或新建不表示任务已停止，不能自动重发。')+'<p>核对原任务后，按现有 /tg new 流程显式新建；原来的未知任务不会被取消。</p>',btn('保留当前状态','close-dialog')+btn('查看客户端说明','unknown-guide','','primary'));break;
    case 'unknown-guide':closeDialog();go('tiangong');break;
    case 'log-detail':state.detailId=value;state.detailTab='result';logDetail(value);break;
    case 'detail-tab':{const [id,t]=value.split(':');state.detailTab=t;logDetail(id);break;}
    case 'fix-entry':go('tiangong');state.clientTab='models';state.modelEditing=true;render();break;
    case 'log-settings':openDialog('记录设置',`<label class="toggle-row"><div>记录请求<small>沿用产品的现有保存设置。</small></div><input type="checkbox" checked></label><label class="toggle-row"><div>记录完整详情<small>保留原默认与隐私语义，不能为新界面默认开启。</small></div><input type="checkbox"></label><details class="subsection"><summary>清理记录</summary><div class="actions">${btn('清理 3 天前','clear-logs','old')}${btn('清空全部','clear-logs','all','danger')}</div></details>`,btn('取消','close-dialog')+btn('保存设置（演示）','confirm-demo','','primary'));break;
    case 'clear-logs':openDialog(value==='all'?'清空全部日志？':'清理 3 天前的日志？','<p>说明删除范围，不提供虚假的撤销。这里只展示确认，不删除任何实际记录。</p>',btn('取消','close-dialog')+btn('确认清理（演示）','confirm-demo','','danger'));break;
    case 'repair':openDialog('按问题检查与修复',`<p>保留现有 13 类问题。先检查再确认修复，修复前退出 Codex，Hub 保持打开。</p><label class="field" style="margin-top:18px">遇到的问题<select><option>启动失败 / 无法加载组织设置</option><option>模型选项缺失</option><option>插件、浏览器或电脑操控</option><option>已配置的 MCP 无法启动</option></select></label><div class="subsection"><h3>检查结果与恢复记录</h3><p class="note" style="margin-top:8px">结果列出影响范围、备份与后改保护。安装文件修复仍遵守 UAC、签名和两平台各自流程。</p></div>`,btn('查看备份规则','undo')+btn('检查所选问题（演示）','repair-check','','primary'));break;
    case 'repair-check':openDialog('检查完成（合成示例）',banner('warn','程序路径需要核对','检查阶段不改文件。修复必须确认目标、备份及运行状态；无法确认时停止覆盖。'),btn('返回','repair')+btn('查看修复影响','repair-impact','','primary'));break;
    case 'repair-impact':openDialog('确认修复的范围','<p>只处理检查出的已知目标。先保存备份，写前再检查后改；不可用的健康来源不能用其他版本替代。此处不会执行任何真实修复。</p>',btn('取消','close-dialog')+btn('演示修复结果','confirm-demo','','primary'));break;
    case 'import-preview':openDialog('导入渠道预览',`<p>来源为 example.com 合成示例，票据不会在此生成或保存。</p>${status('目标','一个普通渠道')}${status('模型','gpt-6.1-sol')}<div class="subsection"><label class="checkbox"><input type="checkbox"><span>启用此渠道</span></label><label class="checkbox" style="margin-top:12px"><input type="checkbox"><span>追加到 Codex 可见模型</span></label><label class="checkbox" style="margin-top:12px"><input type="checkbox"><span>更新时替换原有模型映射</span></label></div>${hint('原型选项仅说明范围；产品沿用原默认值、目标指纹和网络边界。')}`,btn('取消','close-dialog')+btn('确认保存（演示）','confirm-demo','','primary'));break;
    case 'setting-tab':state.setting=value;render();break;
    case 'discard-and-go':state.modelEditing=false;state.modelDraft=null;go(value);break;
    case 'help-route':handleAction(value);break;
    case 'demo-setting':case 'confirm-demo':closeDialog();showToast('演示操作已结束。没有写入真实配置、文件、授权或日志。');break;
    default:showToast('此入口保留原业务能力，当前为布局示意。');
  }
}

document.addEventListener('click',ev=>{
  const target=ev.target.closest('button');if(!target)return;
  if(target.dataset.nav){
    if(state.modelEditing&&target.dataset.nav!==state.page){
      captureModelDraft();
      openDialog('离开前处理当前草稿','<p>模型编辑尚未保存。可以返回继续编辑，或放弃本次草稿再离开。</p>',btn('继续编辑','close-dialog','','primary')+btn('放弃草稿并离开','discard-and-go',target.dataset.nav));
    }else go(target.dataset.nav);
    return;
  }
  if(target.dataset.action){handleAction(target.dataset.action,target.dataset.value||'',target);}
});
document.addEventListener('input',ev=>{
  const map={'capability-search':['[data-capability-row]','capabilityRow'],'source-search':['[data-source-row]','sourceRow'],'log-search':['[data-log-row]','logRow'],'codex-search':['[data-codex-row]','codexRow']};
  const m=map[ev.target.id];if(m){const q=ev.target.value.trim().toLowerCase();document.querySelectorAll(m[0]).forEach(row=>row.hidden=!row.dataset[m[1]].toLowerCase().includes(q));}
});
document.addEventListener('change',ev=>{
  const value=ev.target.value;
  if(ev.target.id==='scenario'){state.scenario=value;state.modelEditing=false;render();}
  if(ev.target.id==='source-vendor'){changeSourceVendor(value);return;}
  if(ev.target.id==='source-scope'){state.sourceScope=value;render();}
  if(ev.target.id==='log-filter'){state.logFilter=value;render();}
  if(ev.target.id==='mcp-client'){state.mcpClient=value;render();}
  if(ev.target.id==='mcp-visible'){state.mcpVisible=ev.target.checked;render();showToast('演示：只改变 Dumpling 来源可见性，不隐藏 MCP 管理或断开连接。');}
});
document.getElementById('theme-toggle').addEventListener('click',()=>{
  const dark=document.documentElement.dataset.theme!=='dark';document.documentElement.dataset.theme=dark?'dark':'light';document.getElementById('theme-toggle').textContent=dark?'浅色预览':'深色预览';
});
document.getElementById('reset-demo').addEventListener('click',()=>location.reload());
window.addEventListener('hashchange',()=>{const p=location.hash.slice(1);if(pages[p]&&p!==state.page)go(p);});
hydrate();state.page=pages[location.hash.slice(1)]?location.hash.slice(1):'home';reviewRememberClient(state.page);render();
