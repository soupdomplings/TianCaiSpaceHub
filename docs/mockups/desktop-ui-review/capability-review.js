'use strict';

// Review 02: synthetic views only. No client discovery, filesystem, network or process access.
const capabilityCatalog = {
  skills: {
    reports: {name:'周报整理',description:'把本周进展整理为清晰的周报，保留问题与下一步。',version:'1.0 · 示例',origin:'本地导入 · 合成目录',format:'SKILL.md + references/',dependency:'无额外 MCP 依赖',files:['SKILL.md','references/report-outline.md']},
    research: {name:'资料研究',description:'检索知识库，形成带来源的研究摘要。',version:'0.3 · 示例',origin:'仓库导入 · example.com',format:'SKILL.md + references/',dependency:'知识库查询 MCP（由用户确认后单独加入）',files:['SKILL.md','references/citation-guide.md']}
  },
  mcp: {
    docs: {name:'知识库查询',description:'向客户端提供知识库搜索与文档读取工具。',version:'配置版本 1 · 示例',origin:'手动添加 · 合成服务',format:'远程服务 · Streamable HTTP',dependency:'独立认证 · 不复用模型 Key',files:['https://mcp.example.com/knowledge','认证引用：DEMO-CREDENTIAL-REF']},
    local: {name:'项目文件工具',description:'通过本地进程提供所选项目的文件工具。',version:'配置版本 1 · 示例',origin:'导入配置 · 合成服务',format:'本地命令 · stdio',dependency:'需要指定项目与运行环境；添加不运行命令',files:['command: DEMO-ONLY-NOT-A-COMMAND','args: ["--project", "DEMO-PROJECT"]']}
  }
};
const capabilityDemoRecords = {skills:{reports:{codex:'hub'},research:{}},mcp:{docs:{codex:'hub'},local:{}}};
const demoAdapters = {
  codex:{label:'演示可加入',scopes:['用户级','指定项目']},
  workbuddy:{label:'演示可加入',scopes:['用户级']},
  tiangong:{label:'按资源判断适配',scopes:['本机']}
};

// Synthetic adapter discovery: same-name items keep separate installation identities.
const capabilityDemoAuth={docs:true};
let capabilitySequence=0;
const capabilityNativeFixtures=[
  {kind:'skills',id:'native-review',key:'workbuddy',name:'代码审查',description:'检查改动，整理可执行的审查意见。',format:'SKILL.md + references/',files:['SKILL.md','references/review-guide.md'],identity:'DEMO-WB-USER-SKILL-REVIEW'},
  {kind:'mcp',id:'native-files',key:'workbuddy',name:'本地文档',description:'读取项目内的文档与目录。',format:'本地命令 · stdio',files:['command: DEMO-ONLY-NOT-A-COMMAND'],identity:'DEMO-WB-USER-MCP-FILES'}
];
function capabilityDiscoverFixtures(){
  for(const f of capabilityNativeFixtures){
    capabilityCatalog[f.kind][f.id]={...f,version:'合成配置',origin:'来自 '+f.key,dependency:'无额外依赖'};
    capabilityDemoRecords[f.kind][f.id]={[f.key]:'native'};
  }
}
capabilityDiscoverFixtures();

function codexEntries(){
  return `<div class="entry-grid">${review3CodexEvidence().entries.map(v=>`<div class="entry-status"><strong>${v.name}</strong>${badge(v.label,v.tone==='ok'?'ok':'')}</div>`).join('')}</div>`;
}
function overviewPage(){
  const missing=state.scenario==='empty',old=state.scenario==='stale'||review3FlowEvidence().mode==='hub-offline';
  const count=kind=>Object.keys(capabilityCatalog[kind]).filter(id=>Object.keys(clients).some(key=>capabilityInUse(kind,id,key))).length;
  const pending=review3SourceVersions('gpt').length;
  return `<div class="runtime-support" aria-label="聊天与扩展摘要"><button data-action="v3-runtime-support">${svg('chat')}<span>聊天接入<small>${old?'旧快照':missing?'尚未配置':'2 个示例账号'}</small></span>${svg('chevron')}</button><button data-nav="skills">${svg('book')}<span>Skill<small>${old?'待重新确认':missing?'未使用':count('skills')+' 项使用中'}</small></span>${svg('chevron')}</button><button data-nav="mcp">${svg('plug')}<span>MCP<small>${old?'待重新确认':missing?'未使用':count('mcp')+' 项使用中'}</small></span>${svg('chevron')}</button>${state.mcpVisible?`<button data-action="open-dumpling">${mark('dumpling')}<span>Dumpling</span>${svg('chevron')}</button>`:''}</div>${pending&&!missing?`<div class="runtime-notice"><span>${pending} 个客户端副本有来源更新</span>${btn('查看差异','v3-source-update','gpt','link')}</div>`:''}`;
}
function review3RuntimeSupport(){
  const missing=state.scenario==='empty',old=state.scenario==='stale'||review3FlowEvidence().mode==='hub-offline';
  const connected=()=>badge(old?'旧快照 · 当前未知':missing?'未配置':'已接入',old||missing?'':'ok');
  openDialog('聊天与外部执行',`<div class="table-wrap"><table class="data-table"><thead><tr><th>平台</th><th>账号连接</th><th>Codex 执行端</th><th>天工执行端</th></tr></thead><tbody><tr><td>飞书</td><td>${connected()}</td><td>${old?'当前未知':missing?'待接入':'已有执行通道'}</td><td>待首次 /tg 准备</td></tr><tr><td>Telegram</td><td>${connected()}</td><td>${old?'当前未知':missing?'待接入':'已有执行通道'}</td><td>暂不支持</td></tr><tr><td>微信 / 企业微信</td><td>未配置</td><td>待平台接入</td><td>待平台接入</td></tr></tbody></table></div>${status('天工运行授权',old?'待重新确认':'待首次使用')}${status('天工桌面消息同步',old?'当前未知':'尚未观察到')}${hint('平台账号、执行授权、模型请求和消息同步分别取证。WorkBuddy /wb 外部任务尚未接通；此处均为合成示例。')}`,btn('关闭','close-dialog')+navBtn('管理聊天接入','chat','primary'));
}

function codexConnection(){
  const missing=state.scenario==='empty';
  return `<div class="split"><section class="card"><div class="card-header"><h3>受管接入配置</h3>${badge(missing?'尚未初始化':'配置已保存',missing?'warn':'ok')}</div>${status('接入状态',missing?'需要初始化':'示例：已是最新；无需重复写入')}${status('增强启动','从 Hub 打开时使用原增强启动流程')}<div class="form-footer">${btn(missing?'初始化接入配置':'查看初始化 / 更新范围','codex-config-review','',missing?'primary':'')}</div></section><section class="card"><div class="card-header"><h3>模型连接设置</h3>${badge('仅作用于受管 Codex')}</div><label class="toggle-row"><div>允许 WebSocket</div><input id="codex-websocket" type="checkbox" ${state.codexWebsocket?'checked':''}></label>${status('本机连接方式','标准连接（合成）')}<div class="form-footer">${btn('本机连接设置','go-network','','link')}${btn('预览并保存此项','codex-ws-preview','','primary')}</div></section></div><section class="card" style="margin-top:18px"><div class="card-header"><h3>App / VS Code / CLI 状态</h3>${btn('查看运行状态','v3-runtime','','link')}</div>${codexEntries()}</section><section class="card" style="margin-top:18px"><h3>相关入口</h3><div class="task-row"><div class="task-copy"><h3>模型与账号来源</h3></div>${btn('可见模型','client-tab','models')}${btn('账号来源','go-account-source')}</div><div class="task-row"><div class="task-copy"><h3>受管恢复、历史与修复</h3></div>${btn('历史与维护','client-tab','maintenance')}</div></section>`;
}

function capabilityInUse(kind,id,key){
  return state.scenario!=='empty'&&['hub','native'].includes(capabilityDemoRecords[kind]?.[id]?.[key]);
}
function capabilityCount(kind,key){return Object.keys(capabilityCatalog[kind]).filter(id=>capabilityInUse(kind,id,key)).length;}
function capabilitySupport(kind,item,key){
  if(key!=='tiangong')return {allowed:true};
  if(kind==='skills')return {allowed:false,reason:'天工原生 Skill 的目录、格式和安装接口尚未核实，不能据此断言客户端不支持 Skill。正式适配前不写入猜测的路径。'};
  if(item?.serviceKind==='remote'||/远程服务|Streamable HTTP/.test(item?.format||''))return {allowed:true,planned:true};
  return {allowed:false,reason:'当前已核实的是天工远程 HTTP MCP 连接接口；本地命令 MCP 的原生格式仍待核实。已有 Dumpling 接入不受影响。'};
}
function capabilitySwitch(kind,id,key,returnTargets=false){
  const item=capabilityCatalog[kind][id],on=capabilityInUse(kind,id,key),support=capabilitySupport(kind,item,key),unread=state.scenario==='stale';
  if(!support.allowed)return `<button class="capability-support" data-action="capability-support" data-value="${kind}:${id}:${key}">待适配 ${svg('help')}</button>`;
  const label=unread?'读取失败':on?'使用中':'未使用';
  return `<button class="cap-use-switch ${on?'is-on':''}" role="switch" aria-checked="${on}" aria-label="${e(clients[key]+' · '+item.name+' · '+label)}" ${unread?'disabled':''} data-action="${returnTargets?'capability-target-toggle':'capability-toggle'}" data-value="${kind}:${id}:${key}"><i aria-hidden="true"></i><span>${label}</span></button>`;
}
function capabilityClientSummary(kind,id){
  const used=Object.keys(clients).filter(k=>capabilityInUse(kind,id,k));
  return `<button class="cap-client-summary" data-action="capability-targets" data-value="${kind}:${id}" aria-label="${e(capabilityCatalog[kind][id].name)} · 选择使用的客户端">${used.slice(0,2).map(k=>appIcon(k)).join('')}<span>${state.scenario==='stale'?'读取失败':used.length?used.length+' 个客户端使用中':'选择客户端'}</span>${svg('chevron')}</button>`;
}
function capabilityRows(kind,key=''){
  return Object.entries(capabilityCatalog[kind]).map(([id,item])=>`<div class="cap-resource" data-capability-row="${e(item.name+' '+item.origin)}"><button class="cap-resource-content" data-action="capability-detail" data-value="${kind}:${id}"><span class="cap-resource-icon">${svg(kind==='skills'?'book':'plug')}</span><span><strong>${e(item.name)}</strong><small>${e(item.description)}</small></span></button><div class="cap-client-controls">${key?capabilitySwitch(kind,id,key):capabilityClientSummary(kind,id)}</div></div>`).join('');
}
function capabilityTargets(kind,id,expanded=false){
  const keys=Object.keys(clients),used=keys.filter(k=>capabilityInUse(kind,id,k)),all=expanded||keys.length<=4,rows=all?[...used,...keys.filter(k=>!used.includes(k))]:used,item=capabilityCatalog[kind][id];
  const tiles=rows.map(k=>{const support=capabilitySupport(kind,item,k),on=capabilityInUse(kind,id,k),unread=state.scenario==='stale';return reviewClientTile(k,{id:'cap-target-'+k,on,unsupported:!support.allowed,disabled:unread,status:unread?'读取失败':!support.allowed?'待适配':'',action:support.allowed?'capability-target-toggle':'capability-support',value:kind+':'+id+':'+k,label:clients[k]+' · '+item.name+' · '+(unread?'读取失败':!support.allowed?'适配待核实，查看原因':on?'使用中，点击停用':'未使用，点击使用')});}).join('');
  openDialog(item.name+' · 使用的客户端',`${all&&keys.length>4?`<div class="cap-target-search">${searchInput('cap-target-search','搜索客户端')}</div>`:''}<div class="cap-target-list">${reviewClientGrid(tiles,rows.length)}</div>${!all?btn('+ 选择客户端','capability-targets-expand',kind+':'+id,'link'):''}`,btn('完成','close-dialog'));
}
function clientExtensions(key){
  return `${state.scenario==='stale'?banner('warn','暂时无法读取配置','保留上次的选择，恢复读取后即可继续。',btn('重新读取','v3-reload')):''}<div class="cap-client-sections">${['skills','mcp'].map(kind=>`<section class="usage-panel"><div class="section-heading"><h2>${kind==='skills'?'Skill':'MCP'}${kind==='skills'&&key==='tiangong'?'':` <small>/ ${capabilityCount(kind,key)+(kind==='mcp'&&reviewDumplingUsed(key)?1:0)} 使用中</small>`}</h2>${kind==='skills'&&key==='tiangong'?'':btn('+ 添加','client-add-capability',kind+':'+key,'link')}</div>${kind==='skills'&&key==='tiangong'?`<div class="cap-resource"><span>天工 Skill 适配待核实</span>${btn('查看接入范围','capability-support','skills:reports:tiangong','link')}</div>`:capabilityRows(kind,key)}${kind==='mcp'&&state.mcpVisible?`<div class="cap-resource"><span class="cap-resource-content">${mark('dumpling')}<strong>Dumpling-MCP</strong></span><div class="actions">${badge(state.scenario==='stale'?'读取失败':reviewDumplingUsed(key)?'使用中':'未使用',state.scenario!=='stale'&&reviewDumplingUsed(key)?'ok':'')}${btn('管理环境','client-dumpling',key,'link')}</div></div>`:''}<div class="form-footer">${navBtn('管理全部'+(kind==='skills'?' Skill':' MCP'),'mcp'===kind?'mcp':'skills','link')}</div></section>`).join('')}</div>`;
}
function capabilityLibrary(kind){
  return title(kind==='skills'?'SKILLS':'MCP',kind==='skills'?'Skill 管理':'MCP 管理','',btn(kind==='skills'?'+ 添加 Skill':'+ 添加 MCP','capability-import',kind,'primary'))+capabilityBody(kind);
}
function capabilityBody(kind){
  const noun=kind==='skills'?'Skill':'MCP';
  if(state.scenario==='empty')return empty('添加第一个 '+noun,'选择内容和客户端，即可使用。',btn('+ 添加 '+noun,'capability-import',kind,'primary'));
  return `${state.scenario==='stale'?banner('warn','暂时无法读取客户端配置','已保留上次的选择。',btn('重新读取','v3-reload')):''}<section class="usage-panel capability-manager"><div class="toolbar">${searchInput('capability-search','搜索名称或来源')}<span class="small-label">${Object.keys(capabilityCatalog[kind]).length} 项</span></div>${capabilityRows(kind)}</section>`;
}
function mcpManager(){
  const html=title('MCP','MCP 管理','',btn('+ 添加 MCP','capability-import','mcp','primary'));
  return html+`<div class="tabs" role="tablist" aria-label="MCP 来源">${[['library','工具服务'],...(state.mcpVisible?[['dumpling','Dumpling-MCP']]:[])].map(([v,t])=>`<button role="tab" aria-selected="${state.mcpView===v}" class="tab ${state.mcpView===v?'active':''}" data-action="mcp-view" data-value="${v}">${t}</button>`).join('')}</div>`+(state.mcpView==='dumpling'&&state.mcpVisible?dumplingMcp():capabilityBody('mcp'));
}
function capabilityDetail(kind,id){
  const item=capabilityCatalog[kind]?.[id];if(!item)return;
  openDialog(item.name,`<p>${e(item.description)}</p><div class="form-footer">${capabilityClientSummary(kind,id)}</div><details class="subsection"><summary>${kind==='skills'?'内容、来源与依赖':'服务配置与来源'}</summary>${status('来源',e(item.origin))}${status('格式',e(item.format))}${status('依赖',e(item.dependency))}<pre class="code-block">${e(item.files.join('\n'))}</pre></details>`,btn('关闭','close-dialog'));
}
// All data below is a fixture. No browser file access or product configuration is used.
function capabilityUse(task,options={}){
  options={...task.options,...options};
  const {kind,id,key,use}=task,item=capabilityCatalog[kind]?.[id];
  if(!item||!clients[key]||!capabilitySupport(kind,item,key).allowed)return false;
  const records=capabilityDemoRecords[kind][id];
  if(state.scenario==='stale'){showToast('无法读取当前配置，请重新读取后继续。');return false;}
  const current=capabilityInUse(kind,id,key);
  if(current===use)return true;
  state.capabilityTask={...task,options};
  if(state.scenario==='error'&&key==='workbuddy'){
    openDialog('未能'+(use?'使用':'停用')+' '+item.name,banner('warn','WorkBuddy 配置暂时被占用','原来的选择已保留。关闭占用配置的编辑器后重试。'),btn('返回','close-dialog')+btn('重试','capability-retry','','primary'));return false;
  }
  if(state.scenario==='conflict'&&key==='workbuddy'&&!options.replace){
    openDialog('WorkBuddy 中有不同的同名内容',`<p>是否备份现有内容，改用所选的「${e(item.name)}」？</p>${hint('只处理 WorkBuddy 中这一个项目，其他配置保持原样。')}`,btn('保留原配置','close-dialog')+btn('备份并替换','capability-replace','','primary'));return false;
  }
  if(!use&&records[key]==='native'&&!options.removeNative){
    openDialog('停用 '+item.name+'？',`<p>这是 ${e(clients[key])} 中原有的配置。停用会备份并移除此客户端的这一项。</p>`,btn('取消','close-dialog')+btn('停用','capability-remove-native','','danger'));return false;
  }
  if(!use&&kind==='mcp'&&id==='docs'&&capabilityInUse('skills','research',key)&&!options.stopDependents){
    openDialog('同时停用资料研究？',`<p>资料研究需要这个 MCP。停用后，将同时停用 ${e(clients[key])} 中的资料研究。</p>`,btn('取消','close-dialog')+btn('同时停用','capability-dependent-stop','','danger'));return false;
  }
  if(use&&kind==='skills'&&id==='research'&&!capabilityInUse('mcp','docs',key)&&!options.dependencies){
    openDialog('同时使用知识库查询？',`<p>「资料研究」需要知识库查询 MCP。将一并为 ${e(clients[key])} 配置。</p>`,btn('取消','close-dialog')+btn('同时使用','capability-dependency','','primary'));return false;
  }
  if(use&&((kind==='mcp'&&id==='docs')||(kind==='skills'&&id==='research'))&&!capabilityDemoAuth.docs){
    openDialog('登录知识库查询',`<p>完成此服务的授权后，继续为 ${e(clients[key])} 配置。</p>`,btn('取消','close-dialog')+btn('演示授权并继续','capability-authorize','','primary'));return false;
  }
  if(use){records[key]='hub';if(options.dependencies&&kind==='skills'&&id==='research')capabilityDemoRecords.mcp.docs[key]='hub';}
  else {delete records[key];if(options.stopDependents)delete capabilityDemoRecords.skills.research[key];}
  state.capabilityTask=null;
  reviewRememberClient(key);
  closeDialog();render();showToast(item.name+' · '+clients[key]+' · '+(use?'使用中':'未使用'));
  if(options.returnTargets){capabilityTargets(kind,id,true);document.getElementById('cap-target-'+key)?.focus();}
  return true;
}
function capabilityToggle(kind,id,key){
  capabilityUse({kind,id,key,use:!capabilityInUse(kind,id,key)});
}
function capabilityImport(kind,preferred=''){
  const skill=kind==='skills',defaultKey=preferred||reviewClientNav.recent.find(k=>k!=='tiangong')||'workbuddy';
  state.capabilityDraft={kind,preferred,type:'0',name:skill?'新技能 · 示例':'新服务 · 示例',source:skill?'DEMO-ONLY-SKILL-FOLDER':'https://mcp.example.com/demo',args:'[]',env:'{}',headers:'{}',auth:'none',token:'',headerName:'Authorization',cwd:'',targets:[defaultKey]};
  if(!skill){reviewMcpBegin();return;}
  capabilityRenderImport();
}
function capabilityCaptureImport(){
  const d=state.capabilityDraft;if(!d)return;
  if(d.kind==='mcp'&&d.entry){reviewMcpCapture();return;}
  for(const key of ['name','source','type','args','env','headers','auth','token','headerName','cwd']){const n=document.getElementById('cap-import-'+key);if(n)d[key]=n.value;}
  d.targets=reviewTargetValues('cap-import-target');
}
function capabilityImportType(type){
  capabilityCaptureImport();const d=state.capabilityDraft;if(!d||d.type===type)return;
  d.typeDrafts??={};d.typeDrafts[d.type]={source:d.source,args:d.args,env:d.env,headers:d.headers,auth:d.auth,token:d.token,headerName:d.headerName,cwd:d.cwd};
  const source=d.kind==='skills'?(type==='0'?'DEMO-ONLY-SKILL-FOLDER':'https://example.com/skills.git'):type==='0'?'https://mcp.example.com/demo':type==='1'?'DEMO-ONLY-NOT-A-COMMAND':'{\n  "mcpServers": {\n    "demo": {"url": "https://mcp.example.com/demo"}\n  }\n}';
  Object.assign(d,{source,args:'[]',env:'{}',headers:'{}',auth:'none',token:'',headerName:'Authorization',cwd:''},d.typeDrafts[type]||{});d.type=type;capabilityRenderImport();
}
function capabilityRenderImport(){
  if(state.capabilityDraft?.kind==='mcp'&&state.capabilityDraft.entry){reviewMcpRender();return;}
  const d=state.capabilityDraft,skill=d.kind==='skills',local=d.type==='1',snippet=d.type==='2';
  const input=(key,label,attrs='')=>`<label class="field">${label}<input id="cap-import-${key}" value="${e(d[key]||'')}" ${attrs}></label>`;
  let fields='';
  if(skill)fields=`<label class="field full">${local?'仓库地址':'Skill 目录'}<input id="cap-import-source" value="${e(d.source)}" placeholder="${local?'https://example.com/skills.git':'包含 SKILL.md 的目录'}"></label>`;
  else if(snippet)fields=`<label class="field full">MCP 配置 JSON<textarea id="cap-import-source" rows="7" spellcheck="false">${e(d.source)}</textarea></label>`;
  else if(local)fields=`${input('source','程序或可执行路径','placeholder="例如 npx 或可执行文件路径"')}<label class="field">参数（JSON 数组）<textarea id="cap-import-args" rows="3" spellcheck="false">${e(d.args)}</textarea></label><label class="field full">环境变量（JSON 对象）<textarea id="cap-import-env" rows="3" spellcheck="false">${e(d.env)}</textarea></label><details class="subsection full"><summary>高级设置</summary>${input('cwd','工作目录（可选）')}</details>`;
  else fields=`${input('source','服务地址','placeholder="https://mcp.example.com/mcp"')}<label class="field">认证方式<select id="cap-import-auth">${[['none','无需认证'],['bearer','Bearer Token'],['header','自定义认证头']].map(([k,v])=>`<option value="${k}" ${d.auth===k?'selected':''}>${v}</option>`).join('')}</select></label>${d.auth!=='none'?`${d.auth==='header'?input('headerName','认证头名称'):''}${input('token',d.auth==='bearer'?'Token':'认证头值','autocomplete="off"')}`:''}<details class="subsection full"><summary>其他请求头</summary><textarea id="cap-import-headers" rows="3" aria-label="其他请求头 JSON">${e(d.headers)}</textarea></details>`;
  openDialog(skill?'添加 Skill':'添加 MCP',`<div class="form-grid"><div class="field full"><span>${skill?'内容来源':'连接方式'}</span><input type="hidden" id="cap-import-type" value="${d.type}"><div class="choice-strip">${(skill?['本地目录','仓库地址']:['远程 HTTP','本地命令','配置导入']).map((v,i)=>`<button class="choice-pill ${d.type===String(i)?'selected':''}" aria-pressed="${d.type===String(i)}" data-action="capability-import-type" data-value="${i}">${v}</button>`).join('')}</div></div>${input('name','名称','maxlength="80"')}${fields}</div><h3 class="cap-target-heading">使用的客户端</h3>${reviewTargetField('cap-import-target',d.targets,{locked:!!d.preferred,single:false})}<p class="field-error" id="cap-import-error" role="alert"></p>`,btn('取消','close-dialog')+btn('添加并使用','capability-import-use','','primary'));
}
function capabilityParsedImport(d){
  if(d.kind==='mcp'&&d.entry)return reviewMcpParsed(d);
  const object=(raw,label)=>{const v=JSON.parse(raw||'{}');if(!v||Array.isArray(v)||typeof v!=='object'||Object.values(v).some(x=>typeof x!=='string'))throw new Error(label+'应为值均为文本的 JSON 对象。');return v;};
  const url=value=>{try{const u=new URL(value);return ['http:','https:'].includes(u.protocol)&&!u.username&&!u.password;}catch{return false;}};
  if(!d.name.trim()||!d.source.trim())throw new Error('请填写名称和内容来源。');
  if(d.kind==='skills'){
    if(d.type==='1'&&!url(d.source))throw new Error('请填写 HTTP(S) 仓库地址。');
    return {format:'SKILL.md + 资源',files:[d.source]};
  }
  let service;
  if(d.type==='2'){
    let v;try{v=JSON.parse(d.source);}catch{throw new Error('配置不是有效的 JSON，请核对后重试。');}
    if(v?.mcpServers){const entries=Object.entries(v.mcpServers);if(entries.length!==1)throw new Error('这份原型一次导入一个 MCP 服务，请选择具体服务。');v=entries[0][1];}
    if(!v||typeof v!=='object'||Array.isArray(v))throw new Error('配置应为一个 MCP 服务对象。');
    if(v.url&&v.command)throw new Error('请分别添加远程服务和本地命令。');
    service=v;
  }else if(d.type==='1'){
    let args;try{args=JSON.parse(d.args||'[]');}catch{throw new Error('参数应为 JSON 数组。');}
    service={command:d.source,args,env:object(d.env,'环境变量'),...(d.cwd?{cwd:d.cwd}:{})};
  }else{
    const headers=object(d.headers,'请求头');
    if(d.auth!=='none'&&!d.token)throw new Error('请填写此服务的认证资料。');
    if(d.auth==='header'&&!/^[A-Za-z0-9-]+$/.test(d.headerName))throw new Error('请核对认证头名称。');
    if(d.auth!=='none')headers[d.auth==='bearer'?'Authorization':d.headerName]=d.auth==='bearer'?'Bearer '+d.token:d.token;
    service={url:d.source,headers};
  }
  if(service.command){
    if(typeof service.command!=='string'||!service.command.trim())throw new Error('请填写程序或可执行路径。');
    if(service.args!==undefined&&(!Array.isArray(service.args)||service.args.some(x=>typeof x!=='string')))throw new Error('参数必须是文本组成的 JSON 数组。');
    if(service.env!==undefined)object(JSON.stringify(service.env),'环境变量');
    return {format:'本地命令 · stdio',serviceKind:'local',files:[JSON.stringify(service,null,2)]};
  }
  if(!url(service.url))throw new Error('请填写有效的 HTTP(S) MCP 服务地址。');
  if(service.headers!==undefined)object(JSON.stringify(service.headers),'请求头');
  if(service.type&&!['http','streamable-http','streamableHttp'].includes(service.type))throw new Error('此原型只演示远程 Streamable HTTP，请核对服务传输方式。');
  return {format:'远程服务 · Streamable HTTP',serviceKind:'remote',files:[JSON.stringify(service,null,2)]};
}
function capabilityImportUse(){
  capabilityCaptureImport();const draft=state.capabilityDraft;if(!draft)return;
  const kind=draft.kind,keys=reviewTargetValues('cap-import-target'),error=document.getElementById('cap-import-error');
  let parsed;try{parsed=capabilityParsedImport(draft);}catch(err){error.textContent=err.message;return;}
  const name=draft.name.trim();
  if(!keys.length){error.textContent='请选择客户端。';return;}
  const blocked=keys.find(k=>!capabilitySupport(kind,parsed,k).allowed);
  if(blocked){error.textContent=clients[blocked]+'：'+capabilitySupport(kind,parsed,blocked).reason;return;}
  if(state.scenario==='stale'){error.textContent='请恢复配置读取后继续，已填内容保留。';return;}
  if(state.scenario==='conflict'){error.textContent='目标配置有变化，请先核对。已填内容保留。';return;}
  const succeeded=keys.filter(k=>!(state.scenario==='error'&&k==='workbuddy')),failed=keys.filter(k=>!succeeded.includes(k));
  if(!succeeded.length){error.textContent='WorkBuddy 配置被占用。解除占用后再次添加。';return;}
  const id='demo-'+(++capabilitySequence);
  capabilityCatalog[kind][id]={name,description:kind==='skills'?'自定义技能':'自定义工具服务',version:'合成版本 1',origin:kind==='skills'?'添加的技能':'添加的服务',dependency:'无额外依赖',...parsed};
  capabilityDemoRecords[kind][id]=Object.fromEntries(succeeded.map(k=>[k,'hub']));
  if(state.scenario==='empty'){state.scenario='ready';document.getElementById('scenario').value='ready';}
  if(kind==='skills')state.selectedSkill=id;else state.selectedMcp=id;
  if(draft.preferred){go(draft.preferred);state.clientTab='extensions';}else go(kind==='skills'?'skills':'mcp');
  state.mcpView='library';closeDialog();render();
  if(failed.length){state.capabilityTask={kind,id,key:failed[0],use:true};openDialog('WorkBuddy 未能使用',`<p>${e(succeeded.map(k=>clients[k]).join('、'))} 已使用。WorkBuddy 配置被占用，解除占用后可重试。</p>`,btn('返回','close-dialog')+btn('重试 WorkBuddy','capability-retry','','primary'));}
  else showToast('已在 '+succeeded.map(k=>clients[k]).join('、')+' 使用 '+name);
}

function capabilityAction(action,value){
  if(reviewMcpAction(action,value))return true;
  switch(action){
    case 'overview-tab':state.overviewTab=value;render();break;
    case 'go-codex-settings':go('codex');state.clientTab='connection';render();break;
    case 'go-network':go('settings');state.setting='network';render();break;
    case 'go-account-source':review3.channelTarget=null;review3.sourceTab='channels';state.source='account';go('sources');break;
    case 'codex-settings':state.clientTab='connection';render();break;
    case 'codex-config-review':openDialog('初始化 / 更新接入范围',`${status('受管配置','仅处理 Hub 管理的接入项')}${status('模型控制','按原规则补齐 MAX / ULTRA / Fast；不切换当前选择')}${status('运行实例','保留退出等待与预检')}${hint('配置已最新时不重复写入。恢复原配置仍在历史与维护中独立确认。')}`,btn('取消','close-dialog')+btn('演示确认','confirm-demo','','primary'));break;
    case 'codex-ws-preview':state.codexWebsocket=document.getElementById('codex-websocket').checked;openDialog('保存 WebSocket 设置',`${status('本次变化',state.codexWebsocket?'允许 WebSocket':'不允许 WebSocket')}${hint('正式实现调用原受保护保存。')}`,btn('返回修改','close-dialog')+btn('保存','confirm-demo','','primary'));break;
    case 'restore-codex':openDialog('恢复原接入配置',banner('warn','只恢复 Hub 管理的接入范围','先核对备份及后改。客户端仍运行时沿用原退出预检。'),btn('取消','close-dialog')+btn('演示预检','confirm-demo','','primary'));break;
    case 'mcp-view':state.mcpView=value;render();break;
    case 'open-mcp-library':go('mcp');state.mcpView='library';render();break;
    case 'open-dumpling':go('mcp');state.mcpView='dumpling';state.mcpVisible=true;render();break;
    case 'capability-import-type':capabilityImportType(value);break;
    case 'capability-targets':{const [kind,id]=value.split(':');capabilityTargets(kind,id);break;}
    case 'capability-targets-expand':{const [kind,id]=value.split(':');capabilityTargets(kind,id,true);break;}
    case 'capability-target-toggle':{const [kind,id,key]=value.split(':');capabilityUse({kind,id,key,use:!capabilityInUse(kind,id,key)},{returnTargets:true});break;}
    case 'capability-support':{const [kind,id,key]=value.split(':');openDialog(clients[key]+' · 适配范围','<p>'+e(capabilitySupport(kind,capabilityCatalog[kind][id],key).reason)+'</p>',btn('关闭','close-dialog')+btn('配置 Dumpling','open-dumpling','','primary'));break;}
    case 'capability-toggle':{const [kind,id,key]=value.split(':');capabilityToggle(kind,id,key);break;}
    case 'capability-detail':{const [kind,id]=value.split(':');capabilityDetail(kind,id);break;}
    case 'client-add-capability':{const [kind,key]=value.split(':');capabilityImport(kind,key);break;}
    case 'client-dumpling':state.mcpClient=value;go('mcp');state.mcpView='dumpling';state.mcpVisible=true;render();break;
    case 'capability-import':capabilityImport(value);break;
    case 'capability-import-use':capabilityImportUse();break;
    case 'capability-retry':if(state.capabilityTask)capabilityUse(state.capabilityTask);break;
    case 'capability-replace':if(state.capabilityTask)capabilityUse(state.capabilityTask,{replace:true});break;
    case 'capability-remove-native':if(state.capabilityTask)capabilityUse(state.capabilityTask,{removeNative:true});break;
    case 'capability-dependency':if(state.capabilityTask)capabilityUse(state.capabilityTask,{dependencies:true});break;
    case 'capability-dependent-stop':if(state.capabilityTask)capabilityUse(state.capabilityTask,{stopDependents:true});break;
    case 'capability-authorize':capabilityDemoAuth.docs=true;if(state.capabilityTask)capabilityUse(state.capabilityTask,{dependencies:true});break;
    default:return false;
  }
  return true;
}
