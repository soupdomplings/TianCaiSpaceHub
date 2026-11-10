'use strict';

// Review 03. Every interaction is confined to synthetic in-memory state.
const review3 = {
  homeTab:'runtime',range:'7d',scope:'all',source:'all',model:'all',metric:'tokens',
  bucket:null,recordStatus:'all',recordPage:0,recordSearch:'',sourceTab:'models',
  modelQuery:'',modelTarget:'all',preferredTarget:null,reuse:null,adjust:null,chartObserver:null,
  flowScope:'workbuddy',runtimeDemo:'auto',runtimeRecovered:false,motion:true,routeFocus:null,routeObserver:null,
  requestSet:null,channelTarget:null
};
const review3RuntimeScenes={auto:'跟随顶部场景',idle:'客户端打开 · 没有请求',closed:'全部客户端已关闭',capacity:'8 个客户端同时打开 · 布局演示',stopped:'所选客户端已关闭',waiting:'已启动 · 等待请求',mismatch:'接入设置需更新','hub-offline':'Hub 服务不可用'};
const review3Profiles = {
  gpt:{id:'gpt',model:'gpt-6',label:'GPT-6',source:'team',effort:'high',output:8192,temperature:.7,description:'日常编程',version:1},
  claude:{id:'claude',model:'claude-sonnet · 示例',label:'Claude Sonnet · 示例',source:'backup',effort:'high',output:8192,temperature:.7,description:'文档与复杂任务',version:1},
  account:{id:'account',model:'gpt-6',label:'GPT-6 · 账号来源',source:'account',effort:'high',output:8192,temperature:.7,description:'使用已登录的示例账号',version:1}
};
const review3Assignments = {
  gpt:{codex:{effort:'high',output:8192,temperature:.7,version:1},workbuddy:{effort:'high',output:8192,temperature:.7,version:1}},
  claude:{workbuddy:{effort:'high',output:8192,temperature:.7,version:1},tiangong:{effort:'自动',output:8192,temperature:.7,version:1}},
  account:{codex:{effort:'high',output:8192,temperature:.7,version:1}}
};
const review3Short = {codex:'Codex',workbuddy:'WorkBuddy',tiangong:'天工 Work'};
function review3Number(n){return n === null || n === undefined ? '—' : n.toLocaleString('zh-CN',{maximumFractionDigits:0});}
function review3Compact(n){return n == null ? '—' : n >= 1e6 ? (n/1e6).toFixed(2)+' M' : n >= 1e3 ? (n/1e3).toFixed(1)+' K' : String(Math.round(n));}
function review3Wait(n){return n == null ? '—' : (n/1000).toFixed(2)+' s';}
function review3Percent(n){return n == null ? '—' : (100*n).toFixed(1)+'%';}
function review3Tabs(items,selected,action,label){
  return `<div class="tabs" role="tablist" aria-label="${label}">${items.map(([id,text])=>`<button class="tab ${selected===id?'active':''}" role="tab" aria-selected="${selected===id}" data-action="${action}" data-value="${id}">${text}</button>`).join('')}</div>`;
}
function review3Filter(){return {range:review3.range,scope:review3.scope,source:review3.source,model:review3.model};}
function review3Rows(){return state.scenario==='empty' ? [] : ReviewUsage.select(review3Filter());}
function review3Options(values,selected){return Object.entries(values).map(([id,name])=>`<option value="${e(id)}" ${selected===id?'selected':''}>${e(name)}</option>`).join('');}
function review3Filters(){
  return `<div class="usage-toolbar"><div class="range-switch" role="group" aria-label="统计时间">${[['today','今天'],['7d','近 7 天'],['30d','近 30 天']].map(([id,label])=>`<button aria-pressed="${review3.range===id}" data-action="v3-range" data-value="${id}">${label}</button>`).join('')}</div><div class="usage-selects"><label><span>接入点</span><select id="v3-scope">${review3Options({all:'全部接入点',...ReviewUsage.scopes},review3.scope)}</select></label><label><span>来源</span><select id="v3-source">${review3Options({all:'全部来源',...ReviewUsage.sources},review3.source)}</select></label><label><span>模型</span><select id="v3-model">${review3Options({all:'全部模型',...Object.fromEntries(ReviewUsage.models.map(x=>[x,x]))},review3.model)}</select></label></div></div>`;
}
function review3Overview(){
  const tabs=review3Tabs([['runtime','运行状态'],['usage','使用统计']],review3.homeTab,'v3-home-tab','概览内容');
  if(review3.homeTab==='runtime'){
    return title('WORKSPACE','概览','',btn('+ 配置客户端','setup'))+tabs+review3Flow()+overviewPage();
  }
  const rows=review3Rows(),s=ReviewUsage.summary(rows);
  const group=ReviewUsage.series(review3Filter());
  if(state.scenario==='empty')group.forEach(p=>Object.assign(p,ReviewUsage.summary([])));
  const modelRows=ReviewUsage.breakdown(rows,'model'),scopeRows=ReviewUsage.breakdown(rows,'scope');
  let html=title('WORKSPACE','概览','',btn('查看请求记录 '+svg('arrow'),'v3-all-records'))+tabs;
  if(state.scenario==='stale')html+=banner('warn','统计暂时无法更新，保留上次结果','上次读取：11 月 7 日 17:45（合成）。已有数据仍可查看，重新读取不会发起模型调用。',btn('重新读取','v3-reload'));
  html+=review3Filters();
  html+=`<section class="usage-metrics" aria-label="统计指标"><div class="usage-stat"><span>请求数</span><strong>${review3Number(s.requests)}</strong></div><div class="usage-stat"><span>输入用量</span><strong title="Token">${review3Compact(s.input)}</strong></div><div class="usage-stat"><span>输出用量</span><strong title="Token">${review3Compact(s.output)}</strong></div><div class="usage-stat"><span>请求完成率</span><strong>${review3Percent(s.completion)}</strong></div></section>`;
  html+=`<section class="usage-panel trend-panel"><div class="section-heading"><h2>使用趋势</h2><div class="range-switch" role="group" aria-label="趋势指标">${[['tokens','Token'],['requests','请求']].map(([id,label])=>`<button aria-pressed="${review3.metric===id}" data-action="v3-metric" data-value="${id}">${label}</button>`).join('')}</div></div><div id="v3-chart" class="usage-chart">${review3Chart(group,860)}</div><div class="chart-caption"><span class="chart-legend">${review3.metric==='tokens'?'<i class="legend-mark input"></i>输入 <i class="legend-mark output"></i>输出':'<i class="legend-mark input"></i>请求数'}</span></div></section>`;
  html+=`<div class="usage-bottom-grid"><section class="usage-panel"><div class="section-heading"><h2>模型用量</h2></div>${review3Bars(modelRows,'model',s.tokens)}</section><section class="usage-panel"><div class="section-heading"><h2>接入点分布</h2></div>${review3Bars(scopeRows,'scope',s.tokens)}</section></div>`;
  html+=`<section class="usage-panel recent-panel"><div class="section-heading"><h2>请求结果</h2>${btn('查看完整记录','v3-all-records','','link')}</div><div class="result-summary">${[['completed','完成',s.completed],['failed','失败',s.failed],['incomplete','未完整结束',s.incomplete],['cancelled','已取消',s.cancelled],['client_disconnected','客户端断开',s.disconnected]].map(([id,name,n])=>`<button data-action="v3-result" data-value="${id}"><span class="result-dot ${id}"></span>${name}<strong>${review3Number(n)}</strong>${svg('chevron')}</button>`).join('')}</div></section>`;
  return html;
}
function review3Chart(points,width){
  width=Math.max(300,Math.round(width));
  const height=240,left=58,right=20,top=24,bottom=38,plotW=width-left-right,plotH=height-top-bottom;
  const names=review3.metric==='tokens'?['input','output']:['requests'];
  const all=points.flatMap(p=>names.map(n=>p[n])).filter(Number.isFinite);
  const hasRecords=points.some(p=>p.requests>0);
  const max=Math.max(1,...all)*1.15;
  const x=i=>left+plotW*(points.length<=1?.5:i/(points.length-1));
  const y=value=>top+plotH*(1-value/max);
  const unit=review3.metric==='tokens'?'Token':'次';
  const fmt=review3Compact;
  let svgText=`<svg class="trend-svg" viewBox="0 0 ${width} ${height}" role="img" aria-labelledby="v3-chart-title v3-chart-desc"><title id="v3-chart-title">${review3.metric==='tokens'?'输入（含缓存）和输出 Token':'请求数量'}随时间变化</title><desc id="v3-chart-desc">全部为合成数据。每个时段可用键盘或鼠标选择，进入相应请求记录。</desc><text x="${left}" y="12" class="chart-axis">${unit}</text>`;
  for(let i=0;i<=3;i++){
    const value=max*i/3,cy=y(value);
    svgText+=`<line x1="${left}" y1="${cy}" x2="${width-right}" y2="${cy}" class="chart-grid"/><text x="${left-10}" y="${cy+4}" text-anchor="end" class="chart-axis">${fmt(value)}</text>`;
  }
  names.forEach((name,index)=>{
    let path='',active=false;
    points.forEach((p,i)=>{if(!Number.isFinite(p[name])){active=false;return;}path+=(active?' L':' M')+x(i).toFixed(1)+' '+y(p[name]).toFixed(1);active=true;});
    if(path)svgText+=`<path d="${path}" class="chart-series ${index?'secondary':''}"/>`;
    points.forEach((p,i)=>{if(Number.isFinite(p[name]))svgText+=`<circle cx="${x(i)}" cy="${y(p[name])}" r="${points.length>15?2:3}" class="chart-point ${index?'secondary':''}"/>`;});
  });
  const every=Math.max(1,Math.ceil(points.length/(width<500?4:8)));
  points.forEach((p,i)=>{
    const label=ReviewUsage.date(p.time,review3.range==='today'?'hour':'day');
    if(i%every===0||i===points.length-1)svgText+=`<text x="${x(i)}" y="${height-12}" text-anchor="${i===0?'start':i===points.length-1?'end':'middle'}" class="chart-axis">${label}</text>`;
    const hitW=points.length<=1?plotW:plotW/(points.length-1),hx=Math.max(left,x(i)-hitW/2);
    svgText+=`<g role="button" tabindex="0" data-usage-bucket="${i}" aria-label="${e(label+'，'+p.requests+' 次请求，'+review3Number(p.tokens)+' Token，查看记录')}"><title>${e(label+' · '+p.requests+' 次请求 · '+review3Number(p.tokens)+' Token')}</title><rect x="${hx}" y="${top}" width="${Math.min(hitW,width-right-hx)}" height="${plotH}" class="chart-hit"/></g>`;
  });
  if(!hasRecords||!all.length)svgText+=`<text x="${left+plotW/2}" y="${top+plotH/2}" text-anchor="middle" class="chart-empty">${!hasRecords?'当前范围还没有请求记录':'当前范围没有有效样本'}</text>`;
  return svgText+'</svg>';
}
function review3Bars(rows,type,total){
  if(!rows.length)return '<div class="chart-empty-block">当前筛选没有记录</div>';
  return `<div class="rank-list">${rows.slice(0,5).map((row,i)=>`<button class="rank-row" data-action="v3-breakdown" data-value="${type}:${e(row.key)}"><span class="rank-no">${String(i+1).padStart(2,'0')}</span><span class="rank-body"><span class="rank-label">${e(type==='scope'?ReviewUsage.scopes[row.key]:row.key)}<strong>${review3Compact(row.tokens)}</strong></span><span class="rank-track"><span style="width:${Math.max(0,total?(row.tokens||0)/total*100:0)}%"></span></span><small>${review3Number(row.requests)} 次请求</small></span>${svg('chevron')}</button>`).join('')}</div>`;
}
function review3Mount(){
  if(review3.chartObserver){review3.chartObserver.disconnect();review3.chartObserver=null;}
  if(review3.routeObserver){review3.routeObserver.disconnect();review3.routeObserver=null;}
  review3MountRoutes();
  const host=document.getElementById('v3-chart');
  if(host){
    const draw=()=>{if(!host.isConnected)return;const points=ReviewUsage.series(review3Filter());if(state.scenario==='empty')points.forEach(p=>Object.assign(p,ReviewUsage.summary([])));host.innerHTML=review3Chart(points,host.clientWidth);};
    draw();
    if(typeof ResizeObserver!=='undefined'){review3.chartObserver=new ResizeObserver(draw);review3.chartObserver.observe(host);}
  }
}
function review3FlowEvidence(scope=review3.flowScope){
  const canDemo=!['empty','stale','error'].includes(state.scenario);
  const hubOffline=canDemo&&review3.runtimeDemo==='hub-offline';
  const allClosed=canDemo&&review3.runtimeDemo==='closed';
  const scene=allClosed?'stopped':canDemo?(review3.runtimeRecovered?'waiting':scope===review3.flowScope?review3.runtimeDemo:'auto'):'auto';
  const terminalOnly=hubOffline||['idle','stopped','mismatch'].includes(scene);
  const rows=state.scenario==='empty'||scene==='waiting'||allClosed?[]:ReviewUsage.records.filter(r=>r.scope===scope&&(state.scenario!=='error'||r.status==='failed')&&(!terminalOnly||r.status!=='running'));
  const record=(canDemo&&!terminalOnly?rows.findLast(r=>r.status==='running'):null)||rows.at(-1)||null;
  const mode=state.scenario==='stale'?'stale':hubOffline?'hub-offline':scene==='stopped'?'stopped':scene==='waiting'?'waiting':scene==='mismatch'?'mismatch':!record?'idle':record.status==='running'?'active':record.status==='failed'?'failed':'settled';
  return {record,mode,scene};
}
function review3RuntimeState(scope=review3.flowScope){
  const evidence=review3FlowEvidence(scope),{record:r,mode}=evidence;
  const ordinary=scope==='ordinary',missing=state.scenario==='empty',stale=mode==='stale';
  const fact=(label,tone='muted')=>({label,tone});
  const running=ordinary?fact('具体客户端未知'):stale?fact('运行状态未知'):missing?fact('尚未接入'):mode==='stopped'?fact('未启动'):fact('运行中','ok');
  const configured=ordinary?fact('客户端配置未知'):stale?fact('配置待核对'):missing?fact('尚未配置'):mode==='mismatch'?fact('接入地址需更新','warn'):fact('已配置到当前 Hub','ok');
  const hub=mode==='hub-offline'?fact('本机服务不可用','error'):stale?fact('当前服务状态未知'):fact('本机服务运行中','ok');
  const ingress=stale?fact('当前连接未知'):mode==='hub-offline'?fact('Hub 暂时不可达','error'):mode==='mismatch'?fact('接入地址不匹配','warn'):mode==='stopped'?fact('本轮连接未验证'):!r?fact('等待首次请求'):fact(ordinary?'已收到普通请求':'已收到客户端请求','ok');
  const route=stale?fact('路由状态未知'):mode==='hub-offline'?fact('等待服务恢复','error'):mode==='mismatch'?fact('等待更新配置','warn'):missing?fact('等待选择模型'):mode==='active'?fact('请求处理中','active'):fact(r?'保留上次路由':'已选择配置示例');
  const result=!r?fact('尚未验证调用'):stale?fact('上次快照 · 当前未知'):r.status==='running'?fact('本次结果待返回'):r.status==='completed'?fact('最近调用完成','ok'):r.status==='failed'?fact('最近请求失败','error'):fact(ReviewUsage.results[r.status],r.status==='incomplete'?'warn':'muted');
  const model=r?r.model:missing?'尚未选择模型':ordinary?'由请求指定':'GPT-6 · 配置示例';
  const source=r?ReviewUsage.sources[r.source]:missing?'尚未选择来源':ordinary?'按普通路由选择':'团队 API · 配置示例';
  const stamp=r?ReviewUsage.date(r.time,'full')+' · 合成记录':'尚无调用记录';
  return {...evidence,running,configured,hub,ingress,route,result,model,source,stamp};
}
function review3Fact(item){
  return `<span class="flow-fact tone-${item.tone}"><i aria-hidden="true"></i>${e(item.label)}</span>`;
}
function review3CodexEvidence(){
  const missing=state.scenario==='empty',stale=state.scenario==='stale',offline=review3FlowEvidence().mode==='hub-offline';
  const closed=review3.runtimeDemo==='closed'&&!['empty','stale','error'].includes(state.scenario);
  const unknown=missing||stale||offline||review3.runtimeRecovered;
  return {missing,stale,offline,closed,entries:['Codex App','VS Code','CLI'].map((name,i)=>({name,label:unknown?'当前连接未知':!closed&&i===0?'已观察到执行通道':'未观察到连接',tone:!unknown&&!closed&&i===0?'ok':'muted'}))};
}
function review3ClientEvidence(key){
  if(key!=='codex'){
    const s=review3RuntimeState(key),active=review3RouteRows().filter(r=>r.scope===key).reduce((n,r)=>n+r.requests.length,0);
    const headline=s.mode==='stale'?{label:'状态未知',tone:'muted'}:s.mode==='hub-offline'?{label:'Hub 不可用',tone:'error'}:s.mode==='mismatch'?{label:'接入需更新',tone:'warn'}:state.scenario==='empty'?{label:'尚未接入',tone:'muted'}:s.mode==='stopped'?{label:'未启动',tone:'muted'}:active?{label:active+' 个请求处理中',tone:'active'}:s.mode==='failed'?{label:'最近请求失败',tone:'error'}:{label:'已打开 · 空闲',tone:'ok'};
    return {...s,headline,active};
  }
  const c=review3CodexEvidence(),fact=(label,tone='muted')=>({label,tone});
  const headline=c.stale?fact('状态未知'):c.missing?fact('尚未接入'):c.offline?fact('已打开 · Hub 不可用','warn'):c.closed?fact('未启动'):fact('已打开','ok');
  return {...c,headline,active:null,running:c.stale?fact('运行状态未知'):c.missing||c.closed?fact('未启动'):fact('已打开','ok'),
    configured:c.stale?fact('配置待核对'):c.missing?fact('尚未初始化'):fact('受管配置已保存','ok'),
    hub:c.offline?fact('本机服务不可用','error'):c.stale?fact('当前服务状态未知'):fact('本机服务运行中','ok'),
    ingress:fact('普通路由 · 客户端归属未确认'),result:fact('无法据普通请求判断本端调用')};
}
function review3ClientHeadline(key){
  const s=review3ClientEvidence(key);
  return `<span class="client-heading-status tone-${s.headline.tone}" role="status"><i aria-hidden="true"></i>${e(s.headline.label)}</span>`;
}
function review3ClientOverview(key){
  const s=review3ClientEvidence(key),unknown=state.scenario==='stale'||s.hub.tone==='error',missing=state.scenario==='empty';
  const modelCount=missing?0:Object.values(review3Assignments).filter(v=>v[key]).length;
  const active=!!s.active&&!unknown,clientTone=s.running.tone,hubTone=s.hub.tone;
  const modelTone=unknown?'muted':active?'active':s.result?.tone==='error'?'error':modelCount?'ok':'muted';
  const modelLabel=unknown?'当前未知':active?s.active+' 个请求进行中':modelCount?modelCount+' 个使用中':'未使用';
  const extensionCount=kind=>capabilityCount(kind,key)+(kind==='mcp'&&reviewDumplingUsed(key)?1:0);
  const used=kind=>unknown?'读取失败':kind==='skills'&&key==='tiangong'?'待适配':extensionCount(kind)?extensionCount(kind)+' 项使用中':'未使用';
  const exec=key==='codex'?(s.entries.some(v=>v.tone==='ok')?{label:'App 已连接',tone:'ok'}:{label:unknown?'当前未知':'未连接',tone:'muted'}):key==='workbuddy'?{label:'暂未接通',tone:'muted'}:{label:unknown?'当前未知':missing?'未使用':'待授权',tone:'muted'};
  const node=(id,label,value,icon,tone,x,y,action='v3-client-node',valueKey=key+':'+id)=>`<button class="client-map-node node-${id} tone-${tone}" style="--x:${x}%;--y:${y}%" data-action="${action}" data-value="${e(valueKey)}"><span class="client-map-symbol">${id==='client'?appIcon(key):svg(icon)}<i aria-hidden="true"></i></span><strong>${label}</strong><span class="client-map-value">${e(value)}</span></button>`;
  const line=(path,flow=false)=>`<path class="client-map-line ${flow?'is-flowing':''}" d="${path}"/>`;
  return `<section class="client-status-overview" aria-label="${e(clients[key])} 状态概览"><div class="section-heading"><h2>状态概览</h2>${s.active?btn('进行中请求（'+s.active+'）','v3-client-requests',key+':running','link'):''}</div><div class="client-map" data-client-map="${key}"><svg class="client-map-links" viewBox="0 0 900 440" preserveAspectRatio="none" aria-hidden="true">${line('M176 115 H418',active)}${line('M482 115 H724',active)}<path class="client-map-config" d="M112 115 C60 115 60 330 112 330 M176 115 C330 115 300 330 418 330"/><path class="client-map-execution ${exec.tone==='ok'?'is-ready':''}" d="M482 115 C630 115 610 330 724 330"/></svg>${node('client',review3Short[key],s.running.label,'plug',clientTone,16,26)}${node('hub','天才 Hub',s.hub.tone==='error'?'服务不可用':state.scenario==='stale'?'当前未知':'运行中','activity',hubTone,50,26)}${node('models','模型',modelLabel,'layers',modelTone,84,26,'client-tab','models')}${node('skills','Skill',used('skills'),'book',!unknown&&capabilityCount('skills',key)?'ok':'muted',16,75)}${node('mcp','MCP',used('mcp'),'plug',!unknown&&extensionCount('mcp')?'ok':'muted',50,75)}${node('execution','聊天与执行',exec.label,'chat',exec.tone,84,75)}</div>${missing?`<div class="client-map-next">${btn('选择模型并接入','setup',key,'primary')}</div>`:s.hub.tone==='error'?`<div class="client-map-next">${btn('重新连接 Hub','v3-runtime-reconnect','','primary')}</div>`:state.scenario==='stale'?`<div class="client-map-next">${btn('重新读取','v3-reload','','primary')}</div>`:s.mode==='mismatch'?`<div class="client-map-next">${btn('更新接入设置','client-tab','models','primary')}</div>`:''}</section>`;
}
function review3ClientNode(key,id){
  if(!clients[key])return;
  const s=review3ClientEvidence(key);
  if(id==='skills'||id==='mcp'){
    go(key);state.clientTab='extensions';render();return;
  }
  if(id==='execution'){
    if(key==='workbuddy'){openDialog('聊天与执行',`<p>WorkBuddy 的外部任务通道尚未接通。</p>`,btn('关闭','close-dialog'));return;}
    review3RuntimeSupport();return;
  }
  if(id==='hub'){
    openDialog('天才 Hub',`${status('本机服务',review3Fact(s.hub))}`,btn('关闭','close-dialog')+(s.hub.tone==='error'?btn('重新连接','v3-runtime-reconnect','','primary'):state.scenario==='stale'?btn('重新读取','v3-reload','','primary'):''));return;
  }
  openDialog(clients[key],`${status('运行',review3Fact(s.running))}${status('接入',review3Fact(s.configured))}${key==='codex'?codexEntries():status('最近请求',review3Fact(s.result))}${key==='codex'?hint('普通入口暂时无法确认具体客户端。'):''}`,btn('关闭','close-dialog')+btn(key==='codex'?'接入设置':'管理模型','client-tab',key==='codex'?'connection':'models','primary'));
}

// Exact synthetic bindings. A name alone never identifies a dedicated channel.
const review3ChannelBindings=[
  {scope:'workbuddy',entry:'model-a',model:'gpt-6',source:'team',profile:'gpt',protocol:'openai-responses',url:'https://api.example.com'},
  {scope:'workbuddy',entry:'model-b',model:'claude-sonnet · 示例',source:'backup',profile:'claude',protocol:'anthropic-messages',url:'https://backup.example.net/v1'},
  {scope:'tiangong',entry:'model-b',model:'claude-sonnet · 示例',source:'backup',profile:'claude',protocol:'anthropic-messages',url:'https://backup.example.net/v1'}
];
function review3ChannelRef(key){
  return review3ChannelBindings.find(r=>review3RouteKey(r)===key)||ReviewUsage.records.find(r=>review3RouteKey(r)===key)||null;
}
function review3OpenChannel(key){
  const ref=review3ChannelRef(key);if(!ref)return;
  closeDialog();review3.sourceTab='channels';
  if(ref.scope==='ordinary'){review3.channelTarget=null;state.source=ref.source;}
  else{review3.channelTarget=key;state.sourceScope='all';}
  go('sources');
}
function review3ChannelDetail(key){
  const ref=review3ChannelRef(key);if(!ref)return empty('未找到此渠道','请重新选择活动路由。',btn('查看全部渠道','v3-channel-clear'));
  const config=review3ChannelBindings.find(r=>review3RouteKey(r)===key),a=config&&review3Assignments[config.profile][config.scope],old=state.scenario==='stale';
  return `<section class="usage-panel channel-detail"><div class="section-heading"><div><h2>${e(ReviewUsage.sources[ref.source])}</h2>${badge(clients[ref.scope]+' 专用')}</div>${btn('全部渠道','v3-channel-clear')}</div>${status('所属客户端',e(clients[ref.scope]))}${status('对应模型',e(ref.model))}${status('渠道配置',badge(old?'快照待核对':config?'已保存 · 合成配置':'尚未读取此条目配置',!old&&config?'ok':''))}${config?status('访问地址',`<span class="mono">${e(state.scenario==='source-update'&&config.source==='team'&&a.version>=2?'https://api.example.com/gateway':config.url)}</span>`)+status('接口类型',e(config.protocol))+status('当前参数','思考 '+e(a.effort)+' · 输出 '+review3Number(a.output)):banner('warn','缺少此条目的配置快照','保留当前路由身份，不使用普通来源或其他客户端的同名配置代替。')}<details class="subsection"><summary>路由身份与高级维护</summary>${status('接入点',e(ref.scope))}${status('条目',e(ref.entry))}${status('来源标识',e(ref.source))}${hint('仅处理这个专用条目；来源更新、启停和删除继续核对 revision、备份及外部后改。')}${config?btn('预览此渠道维护范围','v3-channel-maintain',key):''}</details><div class="form-footer">${btn('查看此路由记录','v3-channel-records',key)}${config?btn('调整此端参数','v3-adjust',config.profile+':'+config.scope):''}</div></section>`;
}
function review3RequestSetRows(set=review3.requestSet){
  if(!set||!Object.hasOwn(ReviewUsage.scopes,set.scope))return [];
  if(set.status==='running')return review3RouteRows().filter(r=>r.scope===set.scope&&(!set.key||r.key===set.key)).flatMap(r=>r.requests).sort((a,b)=>b.time-a.time);
  return ReviewUsage.records.filter(r=>r.scope===set.scope&&(!set.key||review3RouteKey(r)===set.key)&&(set.status==='all'||r.status===set.status)).sort((a,b)=>b.time-a.time);
}
function review3OpenRequestSet(set){
  if(set)review3.requestSet={...set,page:0};
  const context=review3.requestSet;if(!context)return;
  const rows=review3RequestSetRows(),pages=Math.max(1,Math.ceil(rows.length/8));
  context.page=Math.max(0,Math.min(context.page,pages-1));
  const ref=context.key&&review3ChannelRef(context.key);
  const caption=context.status==='running'?'进行中请求':context.status==='failed'?'失败请求':'请求记录';
  const range=`<div class="request-set-scope">${badge(ReviewUsage.scopes[context.scope])}${ref?`<strong>${e(ref.model)}</strong><span>${e(ReviewUsage.sources[ref.source])}</span><span class="mono">${e(ref.entry||'普通路由')}</span>`:''}</div>`;
  const table=`<div class="table-wrap"><table class="data-table"><thead><tr><th>请求 / 时间</th><th>模型 / 渠道</th><th>状态</th><th></th></tr></thead><tbody>${rows.slice(context.page*8,(context.page+1)*8).map(r=>`<tr><td><strong>${e(r.id)}</strong><small>${ReviewUsage.date(r.time,'full')}</small></td><td>${e(r.model)}<small>${e(ReviewUsage.sources[r.source])} · ${e(r.entry||'普通路由')}</small></td><td>${badge(ReviewUsage.results[r.status],r.status==='failed'?'error':r.status==='running'?'active':'')}</td><td>${btn('详情','v3-set-request',r.id,'link')}</td></tr>`).join('')||'<tr><td colspan="4">当前范围没有请求</td></tr>'}</tbody></table></div>`;
  const pagination=`<div class="pagination"><span>${rows.length} 条</span><div>${btn('上一页','v3-set-page','-1').replace('<button ',context.page?'<button ':'<button disabled ')}<span>${context.page+1} / ${pages}</span>${btn('下一页','v3-set-page','1').replace('<button ',context.page+1<pages?'<button ':'<button disabled ')}</div></div>`;
  openDialog(caption+' · '+ReviewUsage.scopes[context.scope],range+table+pagination,btn('关闭','close-dialog')+(ref?btn('查看渠道配置','v3-route-channel',context.key):''));
}
function review3RequestSetDetail(id){
  const r=review3RequestSetRows().find(r=>r.id===id);if(!r){review3OpenRequestSet();return;}
  review3Request(id);
  document.getElementById('dialog-footer').innerHTML=btn('返回相关请求','v3-set-back')+btn('查看渠道配置','v3-route-channel',review3RouteKey(r))+btn('关闭','close-dialog');
}
// Read-only route map for the synthetic review. No client adapter is added.
function review3RouteKey(r){return encodeURIComponent([r.scope,r.entry||'ordinary',r.model,r.source].join('|'));}
function review3RouteRows(){
  const rows=[];
  for(const scope of ['workbuddy','tiangong','ordinary']){
    const s=review3RuntimeState(scope);if(s.mode!=='active')continue;
    const groups=new Map();
    for(const r of ReviewUsage.records.filter(r=>r.scope===scope&&r.status==='running')){
      const key=review3RouteKey(r);
      if(!groups.has(key))groups.set(key,{key,scope,requests:[],record:r,mode:'active',active:true,model:r.model,source:ReviewUsage.sources[r.source],tone:'active',label:'处理中'});
      const row=groups.get(key);row.requests.push(r);row.record=r;
    }
    rows.push(...groups.values());
  }
  return rows;
}
function review3RouteGraph(){
  const missing=state.scenario==='empty',stale=state.scenario==='stale',offline=review3FlowEvidence().mode==='hub-offline';
  const allClosed=review3.runtimeDemo==='closed'&&!['empty','stale','error'].includes(state.scenario);
  const capacity=review3.runtimeDemo==='capacity'&&!['empty','stale','error'].includes(state.scenario);
  const routes=review3RouteRows();
  // Codex process liveness is its own synthetic observation, never inferred from ordinary model requests.
  const candidates=[{key:'codex',name:'ChatGPT / Codex',tone:'ok',label:'已打开 · 合成观察',open:true},
    ...['workbuddy','tiangong'].map(key=>{
      const s=review3RuntimeState(key);
      return {key,name:review3Short[key],tone:s.running.tone,open:s.running.tone==='ok',label:s.mode==='mismatch'?'接入需更新':s.mode==='active'?'请求处理中':s.mode==='waiting'?'等待请求':offline?'Hub 不可用':'已打开 · 空闲',s};
    }),...(capacity?['A','B','C','D','E'].map(name=>({key:'planned-'+name,name:'客户端 '+name,planned:true,open:true,tone:'muted',label:'已打开 · 容量演示'})):[])];
  const clients=missing||stale||allClosed?[]:candidates.filter(c=>c.open);
  if(routes.some(r=>r.scope==='ordinary'))clients.push({key:'ordinary',name:'普通接入',auxiliary:true,tone:'muted',label:'正在使用 · 客户端未知',s:review3RuntimeState('ordinary')});
  const wires=clients.filter(c=>!c.planned).map(c=>c.key==='codex'?{from:'client:codex',to:'hub',scope:'codex',tone:'muted',active:false,label:'已打开；普通模型请求的具体客户端未知'}:{from:'client:'+c.key,to:'hub',scope:c.key,tone:c.s.ingress.tone,active:c.s.mode==='active',label:c.s.ingress.label,request:c.s.mode==='active'?c.s.record.id:null});
  for(const r of routes){
    wires.push({from:'hub',to:'route:'+r.key,scope:r.scope,tone:'active',active:true,request:r.record.id,label:r.label});
    wires.push({from:'route:'+r.key,to:'channel:'+r.key,scope:r.scope,tone:'active',active:true,request:r.record.id,label:r.model+' → '+r.source});
  }
  return {clients,routes,wires,stale,offline,missing,allClosed,capacity,active:routes.reduce((n,r)=>n+r.requests.length,0)};
}

function review3RouteColor(scope){return {codex:'var(--muted)',workbuddy:'var(--route-wb)',tiangong:'var(--route-tg)',ordinary:'var(--route-ordinary)'}[scope]||'var(--muted)';}
function review3RouteRelated(key,graph){
  const focus=review3.routeFocus;
  if(!focus||focus==='hub')return true;
  if(key===focus||key==='hub')return true;
  const route=graph.routes.find(r=>'route:'+r.key===focus||'channel:'+r.key===focus);
  if(route)return key==='client:'+route.scope||key==='route:'+route.key||key==='channel:'+route.key;
  if(focus.startsWith('client:'))return graph.routes.some(r=>('route:'+r.key===key||'channel:'+r.key===key)&&'client:'+r.scope===focus);
  return false;
}
function review3CodexMini(graph){
  return `<span class="route-execution" aria-label="执行通道，与模型请求归属分别记录"><span>执行</span>${review3CodexEvidence().entries.map(v=>`<span class="tone-${v.tone}" title="${e(v.name+' 执行通道：'+v.label)}"><i aria-hidden="true"></i>${v.name.replace('Codex ','')}</span>`).join('')}</span>`;
}
function review3Flow(){
  const g=review3RouteGraph(),disabled=['empty','stale','error'].includes(state.scenario);
  if(review3.routeFocus&&!['hub',...g.clients.map(c=>'client:'+c.key),...g.routes.flatMap(r=>['route:'+r.key,'channel:'+r.key])].includes(review3.routeFocus))review3.routeFocus=null;
  const clientRows=g.clients.map(c=>{
    const key='client:'+c.key,related=review3RouteRelated(key,g),icon=c.planned?'<span class="route-plan-mark">'+c.name.slice(-1)+'</span>':c.auxiliary?svg('plug'):mark(c.key);
    return `<button class="route-client ${c.planned?'planned':''} ${c.auxiliary?'auxiliary':''} ${related?'':'route-dim'}" data-route-node="${key}" data-action="v3-route-select" data-value="${key}" aria-pressed="${review3.routeFocus===key}" aria-controls="route-inspector" style="--scope-color:${review3RouteColor(c.key)}"><span class="route-client-icon">${icon}</span><span class="route-client-copy"><span class="route-client-name">${e(c.name)}${c.planned?'<span class="route-plan-label">规划</span>':''}</span>${c.key==='codex'?review3CodexMini(g):`<small class="tone-${c.s?.configured.tone==='warn'?'warn':'muted'}">${e(c.planned?'待新增适配':c.label)}</small>`}</span>${c.planned?'':`<i class="route-state-dot tone-${c.tone}" title="${e(c.key==='codex'?'已打开 · 独立合成观察':c.s?.running.label||c.label)}" aria-label="${e(c.key==='codex'?'已打开 · 独立合成观察':c.s?.running.label||c.label)}"></i>`}</button>`;
  }).join('');
  const routeRows=g.routes.map(r=>{
    const key='route:'+r.key;
    return `<button class="route-target ${r.active?'is-active':''} ${review3RouteRelated(key,g)?'':'route-dim'}" data-route-node="${e(key)}" data-action="v3-route-select" data-value="${e(key)}" aria-pressed="${review3.routeFocus===key}" aria-controls="route-inspector" style="--scope-color:${review3RouteColor(r.scope)}"><span class="route-target-copy"><strong title="${e(r.model)}">${e(r.model)}</strong><small>${r.scope==='ordinary'?'普通接入':review3Short[r.scope]} · ${r.requests.length} 个请求</small></span><i class="route-state-dot tone-active" aria-label="请求进行中"></i></button>`;
  }).join('');
  const channelRows=g.routes.map(r=>{
    const key='channel:'+r.key;
    return `<button class="route-channel is-active ${review3RouteRelated(key,g)?'':'route-dim'}" data-route-node="${e(key)}" data-action="v3-route-select" data-value="${e(key)}" aria-pressed="${review3.routeFocus===key}" aria-controls="route-inspector" style="--scope-color:${review3RouteColor(r.scope)}"><span class="route-source-mark" aria-hidden="true">${svg(r.record.source==='account'?'shield':'layers')}</span><span class="route-target-copy"><strong title="${e(r.source)}">${e(r.source)}</strong><small class="tone-active">${e(r.label)}</small></span></button>`;
  }).join('');
  const hubTone=g.offline?'error':g.stale?'muted':'ok',headline=g.stale?'读取失败 · 保留快照':g.offline?'本机服务不可用':g.active?g.active+' 个请求处理中':g.missing?'等待接入':state.scenario==='error'?'最近请求失败':g.clients.length?'等待请求':'当前待用';
  const recovery=g.stale?btn('重新读取','v3-reload','','link'):g.offline?btn('重新连接（演示）','v3-runtime-reconnect','','link'):g.missing?btn('从已有模型接入','v3-open-models','','link'):state.scenario==='error'?btn('查看失败请求','v3-result','failed','link'):'';
  return `<section class="route-overview" aria-label="客户端、Hub、使用中的模型与渠道，合成演示"><div class="route-heading"><div><h2>路由运行</h2><span class="route-headline tone-${g.offline?'error':g.stale?'warn':g.active?'active':'muted'}">${headline}</span></div></div><div class="route-map" id="v3-route-map"><svg class="route-wires" id="v3-route-wires" aria-hidden="true"></svg><div class="route-grid ${!g.clients.length?'hub-only':!g.routes.length?'no-active-models':''}">${g.clients.length?`<div class="route-client-column"><div class="route-clients">${clientRows}</div></div>`:''}<div class="route-hub-column"><button class="route-hub ${g.active?'working':''}" data-route-node="hub" data-action="v3-route-select" data-value="hub" aria-pressed="${review3.routeFocus==='hub'}" aria-controls="route-inspector"><span class="route-hub-mark">T</span><strong>Hub</strong>${review3Fact({label:g.offline?'本机服务不可用':g.stale?'当前状态未知':'本机服务运行中',tone:hubTone})}</button></div>${g.routes.length?`<div class="route-target-column"><div class="route-targets">${routeRows}</div></div><div class="route-channel-column"><div class="route-targets">${channelRows}</div></div>`:''}</div></div><div class="route-footer">${recovery}<details class="route-demo"><summary>演示状态</summary><div><label>接入点<select id="v3-demo-scope">${review3Options({workbuddy:'WorkBuddy',tiangong:'天工 Work',ordinary:'普通接入'},review3.flowScope)}</select></label><label>状态<select id="v3-runtime-demo" ${disabled?'disabled':''}>${Object.entries(review3RuntimeScenes).map(([key,label])=>`<option value="${key}" ${review3.runtimeDemo===key?'selected':''} ${review3.flowScope==='ordinary'&&['stopped','waiting','mismatch'].includes(key)?'disabled':''}>${label}</option>`).join('')}</select></label></div></details></div>${review3RouteInspector(g)}</section>`;
}
function review3RouteInspector(g){
  const focus=review3.routeFocus;if(!focus)return '<div id="route-inspector"></div>';
  let name='',body='',actions='';
  if(focus==='hub'){
    name='Hub · 本机服务';body=status('本机服务',g.offline?'不可用':g.stale?'当前状态未知':'运行中')+status('模型请求',g.active+' 个进行中 · 合成快照');
    actions=g.offline?btn('重新连接（演示）','v3-runtime-reconnect','','primary'):g.stale?btn('重新读取','v3-reload','','primary'):navBtn('服务设置','settings');
  }else if(focus.startsWith('client:')){
    const c=g.clients.find(c=>'client:'+c.key===focus);if(!c)return '';
    name=c.name;
    if(c.planned)body='<p class="note">仅用于检查 8 个客户端的布局容量。尚未新增这个客户端的适配或配置能力。</p>';
    else if(c.key==='codex'){
      body=status('客户端进程','已打开 · 独立合成观察')+status('受管配置',g.missing?'尚未初始化':g.stale?'待重新确认':'已保存')+status('模型请求','普通路由无法确认来自哪个客户端')+codexEntries();
      actions=btn('接入设置','go-codex-settings','','primary')+btn('打开客户端','open-client','codex');
    }else{
      const s=c.s;body=`<div class="route-detail-facts">${[s.running,s.configured,s.ingress,s.result].map(review3Fact).join('')}</div>`;
      body+=status('进行中请求',g.routes.filter(r=>r.scope===c.key).reduce((n,r)=>n+r.requests.length,0)+' 个');
      actions=review3RouteRecovery(c.key,s,s.record)+(c.key==='ordinary'?btn('查看模型来源','v3-open-models'):navBtn('管理客户端',c.key));
    }
  }else{
    const r=g.routes.find(r=>'route:'+r.key===focus||'channel:'+r.key===focus);if(!r)return '';
    const channel=focus.startsWith('channel:');name=channel?'渠道 · '+r.source:r.model;
    body=`<div class="route-detail-facts">${review3Fact({label:r.label,tone:r.tone})}<span>${r.scope==='ordinary'?'普通接入 · 客户端未知':clients[r.scope]}</span></div>`;
    body+=status(channel?'对应模型':'对应渠道',e(channel?r.model:r.source))+status('进行中请求',r.requests.length+' 个');
    body+=`<details class="route-identity"><summary>路由身份</summary>${status('条目',e(r.record.entry||'普通路由'))}${status('接入点',e(r.scope))}${status('来源',e(r.record.source))}</details>`;
    actions=btn('查看进行中请求（'+r.requests.length+'）','v3-route-requests',r.key)+btn('查看渠道配置','v3-route-channel',r.key,channel?'primary':'link');
  }
  return `<aside id="route-inspector" class="route-inspector" aria-label="选中节点详情"><div class="section-heading"><h3>${e(name)}</h3>${btn('收起','v3-route-clear','','link')}</div>${body}<div class="actions">${actions}</div></aside>`;
}
function review3RouteRecovery(scope,s,r){
  let html=s.mode==='stale'?btn('重新读取','v3-reload'):s.mode==='hub-offline'?btn('重新连接（演示）','v3-runtime-reconnect'):s.mode==='mismatch'?btn('更新接入设置','v3-runtime-config',scope,'primary'):s.mode==='stopped'&&scope!=='ordinary'?btn('打开 '+clients[scope],'open-client',scope):'';
  const active=review3RouteRows().filter(row=>row.scope===scope).reduce((n,row)=>n+row.requests.length,0);
  if(active)html+=btn('查看进行中请求（'+active+'）','v3-client-requests',scope+':running','link');
  else if(r)html+=btn(r.status==='failed'?'查看失败请求':'查看本端记录','v3-client-requests',scope+(r.status==='failed'?':failed':':all'),'link');
  if(r?.status==='failed')html+=btn('检查来源','v3-route-channel',review3RouteKey(r));
  return html;
}
function review3RoutePaths(graph,rects){
  const hub=rects.get('hub');if(!hub)return '';
  return graph.wires.map(w=>{
    const a=rects.get(w.from),b=rects.get(w.to);if(!a||!b)return '';
    const x1=a.right,y1=(a.top+a.bottom)/2,x2=b.left,y2=(b.top+b.bottom)/2,dx=Math.max(16,(x2-x1)*.55);
    const d=`M${x1} ${y1} C${x1+dx} ${y1} ${x2-dx} ${y2} ${x2} ${y2}`;
    const selected=review3RouteRelated(w.from,graph)&&review3RouteRelated(w.to,graph);
    return `<g class="route-wire-group ${selected?'':'route-dim'}" data-wire-scope="${w.scope}" style="--scope-color:${review3RouteColor(w.scope)}"><title>${e(w.label)}</title><path d="${d}" class="route-wire tone-${w.tone}"/>${w.active?`<path d="${d}" class="flow-carry" data-request="${e(w.request)}"/>`:''}</g>`;
  }).join('');
}
function review3MountRoutes(){
  if(state.page!=='home'||review3.homeTab!=='runtime')return;
  const map=document.getElementById('v3-route-map'),svgMap=document.getElementById('v3-route-wires');
  if(!map||!svgMap||typeof map.getBoundingClientRect!=='function')return;
  const draw=()=>{
    if(!map.isConnected)return;
    const base=map.getBoundingClientRect(),rects=new Map();
    for(const n of map.querySelectorAll('[data-route-node]')){
      const b=n.getBoundingClientRect();rects.set(n.dataset.routeNode,{left:b.left-base.left,right:b.right-base.left,top:b.top-base.top,bottom:b.bottom-base.top});
    }
    svgMap.setAttribute('viewBox',`0 0 ${base.width} ${base.height}`);svgMap.innerHTML=review3RoutePaths(review3RouteGraph(),rects);
  };
  draw();
  if(typeof ResizeObserver!=='undefined'){review3.routeObserver=new ResizeObserver(draw);review3.routeObserver.observe(map);for(const n of map.querySelectorAll('[data-route-node]'))review3.routeObserver.observe(n);}
}

function review3RecordRows(){
  let rows=review3Rows();
  if(review3.bucket)rows=rows.filter(r=>r.time>=review3.bucket.from&&r.time<review3.bucket.to);
  if(review3.recordStatus!=='all')rows=rows.filter(r=>r.status===review3.recordStatus);
  const query=review3.recordSearch.toLowerCase();
  return rows.filter(r=>(r.id+' '+r.model+' '+ReviewUsage.scopes[r.scope]+' '+ReviewUsage.sources[r.source]).toLowerCase().includes(query)).reverse();
}
function review3Records(){
  const rows=review3RecordRows(),size=12,pages=Math.max(1,Math.ceil(rows.length/size));
  review3.recordPage=Math.min(review3.recordPage,pages-1);
  const slice=rows.slice(review3.recordPage*size,(review3.recordPage+1)*size);
  let html=title('REQUESTS','请求记录','筛选与统计保持一致，查看每次请求的实际结果。',btn('记录设置','log-settings'))+review3Filters();
  if(review3.bucket)html+=`<div class="drilldown-strip">${svg('activity')}<span>来自趋势：${ReviewUsage.date(review3.bucket.from,'full')} — ${ReviewUsage.date(review3.bucket.to,'full')}</span>${btn('移除此时段','v3-clear-bucket','','link')}${btn('返回统计','v3-back-usage','','link')}</div>`;
  html+=`<section class="usage-panel records-panel"><div class="toolbar"><label class="search">${svg('search')}<input id="v3-record-search" value="${e(review3.recordSearch)}" placeholder="搜索模型、来源或请求编号" aria-label="搜索请求记录"></label><select id="v3-record-status" aria-label="请求结果">${review3Options({all:'全部结果',...ReviewUsage.results},review3.recordStatus)}</select><span class="small-label">${review3Number(rows.length)} 条</span></div><div class="table-wrap"><table class="data-table"><thead><tr><th>时间 / 接入点</th><th>模型 / 来源</th><th>结果</th><th>Token</th><th>首字 / 总耗时</th><th></th></tr></thead><tbody>${slice.map(r=>`<tr><td>${ReviewUsage.date(r.time,'full')}<small>${ReviewUsage.scopes[r.scope]}</small></td><td><strong>${e(r.model)}</strong><small>${ReviewUsage.sources[r.source]}</small></td><td>${badge(ReviewUsage.results[r.status],r.status==='completed'?'ok':r.status==='failed'?'error':'')}</td><td class="tabular">${review3Number(r.tokens)}</td><td>${review3Wait(r.ttft)}<small>${review3Wait(r.latency)}</small></td><td>${btn('详情','v3-request',r.id,'link')}</td></tr>`).join('')||'<tr><td colspan="6" class="table-empty">当前筛选没有请求记录</td></tr>'}</tbody></table></div><div class="pagination"><span>仅合成记录 · 不读取真实日志</span><div>${btn('上一页','v3-record-page','-1').replace('<button ',review3.recordPage?'<button ':'<button disabled ')}<span>${review3.recordPage+1} / ${pages}</span>${btn('下一页','v3-record-page','1').replace('<button ',review3.recordPage+1<pages?'<button ':'<button disabled ')}</div></div></section>`;
  return html;
}
function review3Request(id){
  const r=ReviewUsage.records.find(x=>x.id===id);if(!r)return;
  const note=r.status==='failed'?'合成示例：来源返回失败，查看来源配置后可继续处理。':r.status==='client_disconnected'?'客户端已断开；无法仅凭断开判断上游任务是否停止。':r.status==='cancelled'?'请求被取消，不计作上游服务故障。':r.status==='running'?'请求尚无终态，不进入完成率分母。':'这是本次请求的结果，不推定其他模型能力可用。';
  openDialog('请求 '+id,`<div class="request-result">${badge(ReviewUsage.results[r.status],r.status==='completed'?'ok':r.status==='failed'?'error':'')}<span>${ReviewUsage.date(r.time,'full')} · ${ReviewUsage.scopes[r.scope]}</span></div><p>${note}</p><details open class="subsection"><summary>用量与耗时</summary>${status('输入 / 输出',review3Number(r.input)+' / '+review3Number(r.output))}${status('缓存读取',review3Number(r.cache),'属于输入用量，不重复加到总 Token')}${status('首字 / 总耗时',review3Wait(r.ttft)+' / '+review3Wait(r.latency))}</details><details class="subsection"><summary>客户端请求与实际上游</summary>${status('请求模型',e(r.model))}${status('实际来源',ReviewUsage.sources[r.source])}${status('客户端 / 上游流式',String(r.stream)+' / '+(r.upstreamStream===null?'未知':String(r.upstreamStream)))}${status('接入条目',r.entry||'普通接入，具体客户端未知')}${hint('这里只展示合成字段；没有捕获真实请求正文或凭据。')}</details>`,btn('关闭','close-dialog')+(r.status==='failed'?btn('查看此来源','v3-request-source',r.source,'primary'):''));
}
function review3SourceTabs(){return review3Tabs([['models','我的模型'],['channels','账号与渠道']],review3.sourceTab,'v3-source-tab','模型资源');}
function review3Profile(id){const p=review3Profiles[id];return {...p,version:state.scenario==='source-update'&&id==='gpt'?2:p.version};}
function review3SourceVersions(id){const current=review3Profile(id);return Object.entries(review3Assignments[id]).filter(([key,a])=>key!=='codex'&&a.version<current.version);}
function review3Library(){
  let html=title('MODELS','模型与渠道','配置一次，给需要的客户端使用；每端都可以独立调整。',btn('+ 添加模型来源','new-source','','primary'))+review3SourceTabs();
  if(state.scenario==='empty')return html+empty('从一个模型来源开始','添加 API 来源或账号后，就能选择模型并给客户端使用。',btn('添加来源','new-source','','primary'));
  const list=Object.values(review3Profiles).filter(p=>(p.label+' '+ReviewUsage.sources[p.source]).toLowerCase().includes(review3.modelQuery.toLowerCase()));
  if(state.scenario==='source-update'&&review3SourceVersions('gpt').length)html+=banner('info','团队 API 有更新，客户端副本保持原样','可选择要同步的客户端。你在各端调整的模型参数会保留。',btn('查看差异','v3-source-update','gpt'));
  html+=`<section class="usage-panel model-library"><div class="toolbar"><label class="search">${svg('search')}<input id="v3-model-search" value="${e(review3.modelQuery)}" aria-label="搜索我的模型" placeholder="搜索模型或来源"></label><span class="small-label">${list.length} 个模型</span></div>${list.map(p=>`<div class="model-resource-row"><span class="model-resource-icon">${svg(p.source==='account'?'shield':'layers')}</span><div class="model-resource-main"><strong>${e(p.label)}</strong><small>${e(ReviewUsage.sources[p.source])} · ${e(p.description)}</small></div><div class="resource-actions">${btn('模型设置','v3-profile-settings',p.id)}${btn('查看来源','v3-profile-source',p.id,'link')}</div></div>`).join('')||'<div class="chart-empty-block">没有匹配的模型</div>'}</section>`;
  return html;
}
function review3ModelPicker(target){
  if(!clients[target])return;
  review3.preferredTarget=target;
  openDialog('给 '+clients[target]+' 添加模型',`<div class="model-picker">${Object.values(review3Profiles).map(p=>`<div class="client-model-row"><div class="model-resource-main"><strong>${e(p.label)}</strong><small>${e(ReviewUsage.sources[p.source])}</small></div>${btn('选择','v3-pick-profile',p.id)}</div>`).join('')}</div>`,btn('取消','close-dialog'));
}
function review3ModelEffect(target){
  return target==='tiangong'?'无需重启':target==='workbuddy'?'如客户端仍未显示，重新加载配置':'通过 Hub 增强启动 Codex';
}
function review3ClientModels(key){
  const rows=Object.values(review3Profiles).filter(p=>review3Assignments[p.id][key]);
  return `${review3.modelNext===key&&key!=='tiangong'?`<div class="model-use-next"><span>${e(review3ModelEffect(key))}</span>${btn(key==='codex'?'通过 Hub 打开':'打开 WorkBuddy','open-client',key,'link')}</div>`:''}<section class="usage-panel"><div class="section-heading"><div><h2>${key==='codex'?'接入方案与候选来源':'使用中的模型'} <small> / ${rows.length}</small></h2></div>${btn('+ 从我的模型加入','v3-library-target',key,'primary')}</div>${rows.map(p=>{const a=review3Assignments[p.id][key];return `<div class="client-model-row"><span class="model-resource-icon">${svg('layers')}</span><div class="model-resource-main"><strong>${p.label}</strong><small>${ReviewUsage.sources[p.source]} · 思考 ${a.effort} · 输出 ${review3Number(a.output)}</small><small>${a.adjusted?'已在此客户端独立调整':'独立副本'}${key!=='codex'&&a.version<review3Profile(p.id).version?' · 来源有更新':''}</small></div><div class="resource-actions">${badge('使用中','ok')}${btn('调整参数','v3-adjust',p.id+':'+key)}${btn('用于其他客户端','v3-reuse',p.id+':'+key,'link')}${btn('停用','v3-model-stop',p.id+':'+key,'link')}</div></div>`;}).join('')||'<div class="chart-empty-block">尚未使用模型，选择已有模型即可。</div>'}${key!=='codex'?`<div class="form-footer">${btn('手动配置模型','edit-model','new','link')}</div>`:''}</section>`;
}
function review3ReuseChooser(id,origin='',chooseTarget=false){
  review3.reuse={id,origin,target:null};
  if(review3.preferredTarget&&!origin&&!chooseTarget){review3ReuseTarget(review3.preferredTarget);return;}
  const p=review3Profile(id),a=origin&&review3Assignments[id][origin];
  const keys=Object.keys(clients).filter(key=>key!==origin).sort((a,b)=>Number(!!review3Assignments[id][b])-Number(!!review3Assignments[id][a])),tiles=keys.map(key=>{const exists=!!review3Assignments[id][key],blocked=key==='tiangong'&&p.source==='account';return reviewClientTile(key,{on:exists,disabled:state.scenario==='stale',unsupported:blocked,status:blocked?'此来源待支持':state.scenario==='stale'?'读取失败':'',action:'ui-model-target',value:id+':'+origin+':'+key,label:clients[key]+' · '+p.label+' · '+(blocked?'查看来源限制':exists?'使用中，点击停用':'未使用，点击使用')});}).join('');
  openDialog('将 '+p.label+' 用于客户端',`<div class="reuse-origin"><span class="model-resource-icon">${svg('layers')}</span><div><strong>${p.label}</strong><small>${ReviewUsage.sources[p.source]}${origin?' · 来自 '+clients[origin]+' 当前配置':' · 当前模型配置'}</small></div>${badge('独立副本')}</div>${reviewClientGrid(tiles,keys.length)}`,btn('完成','close-dialog'),'跨端复用 · 选择目标');
}
function review3ReuseTarget(target,rechecked=false){
  const plan=review3.reuse;if(!plan)return;plan.target=target;
  const p=review3Profile(plan.id),origin=plan.origin&&review3Assignments[plan.id][plan.origin];
  if(target==='tiangong'&&p.source==='account'){
    openDialog('此来源暂不能用于天工',banner('warn','请选择普通 API 来源','天工接入保留现有账号来源限制。其他已配置的客户端不受影响。'),btn('返回选择','v3-reuse-return')+btn('选择其他模型','v3-library-target',target,'primary'));return;
  }
  const selected=origin||p,targetConfig=review3Assignments[plan.id][target];
  const changed=targetConfig&&(targetConfig.effort!==selected.effort||targetConfig.output!==selected.output||targetConfig.temperature!==selected.temperature);
  const stale=origin&&plan.origin!=='codex'&&origin.version<p.version;
  const targetBehind=targetConfig&&target!=='codex'&&targetConfig.version<p.version;
  if(state.scenario==='conflict'&&!rechecked){
    openDialog('目标配置在其他位置变化',banner('warn','你的选择已保留，先重新核对','只重新读取所选目标并比较差异；不会覆盖其他客户端或强行重试写入。'),btn('取消','close-dialog')+btn('演示重新核对','v3-reuse-recheck','','primary'));return;
  }
  if(changed||stale||targetBehind){
    openDialog('确认 '+clients[target]+' 的配置差异',`<p>只处理这个客户端，其他端保持原样。</p><div class="compare-grid"><section><small>目标端当前配置</small><strong>${targetConfig?'思考 '+targetConfig.effort:'尚未加入'}</strong><span>输出 ${targetConfig?review3Number(targetConfig.output):'—'}</span></section><section><small>本次选择的配置</small><strong>思考 ${selected.effort}</strong><span>输出 ${review3Number(selected.output)}</span></section></div>${stale||targetBehind?banner('warn','普通来源已更新','本次同步所选目标的来源连接信息。其他客户端的专用副本保持原样；如需保留旧来源，请先显式建立可用的普通来源。'):''}${changed?`<label class="reuse-radio"><input type="radio" name="v3-params-mode" value="keep" checked>保留 ${clients[target]} 已调整的参数</label><label class="reuse-radio"><input type="radio" name="v3-params-mode" value="replace">将目标端参数替换为本次选择</label>`:''}${hint('应用前仍需校验目标身份、revision 和备份。这里仅改变演示状态。')}`,btn('返回选择','v3-reuse-return')+btn('应用所选变化','v3-reuse-confirm','','primary'));return;
  }
  if(state.scenario==='conflict'&&rechecked&&!targetConfig){
    openDialog('重新核对完成，确认此目标',`${status('目标',clients[target])}${status('最新变化','未发现同一受管条目，可新增')}${status('保留范围','原客户端与其他条目保持原样')}${hint('重新核对本身不写入。确认后才演示保存所选目标。')}`,btn('取消','close-dialog')+btn('确认添加此目标','v3-reuse-confirm','','primary'));return;
  }
  if(targetConfig){review3ReuseResult(target,true);return;}
  review3ApplyReuse(false);
}
function review3ApplyReuse(keep){
  const plan=review3.reuse;if(!plan||!plan.target)return;
  const p=review3Profile(plan.id),old=review3Assignments[plan.id][plan.target],origin=plan.origin&&review3Assignments[plan.id][plan.origin];
  const chosen=keep&&old?old:origin||p;
  review3Assignments[plan.id][plan.target]={effort:chosen.effort,output:chosen.output,temperature:chosen.temperature,version:p.version,adjusted:!!chosen.adjusted};
  review3ReuseResult(plan.target,false);
}
function review3ReuseResult(target,skipped){
  const plan=review3.reuse,p=review3Profile(plan.id);closeDialog();
  if(plan.returnChooser){render();review3ReuseChooser(plan.id,plan.origin,true);showToast(p.label+' · '+clients[target]+' · 使用中');return;}
  go(target);state.clientTab='models';review3.modelNext=target;render();
  showToast(p.label+' · '+clients[target]+' · 使用中');
}
function review3StopModel(id,key,step='request'){
  const current=review3Assignments[id]?.[key];if(!current||!clients[key])return;
  if(state.scenario==='stale'){showToast('无法读取当前配置，请重新读取后继续。');return;}
  if(state.scenario==='error'&&key==='workbuddy'){
    review3.stopModel={id,key};
    openDialog('未能停用模型',banner('warn','WorkBuddy 配置暂时被占用','原来的选择已保留。关闭占用配置的编辑器后重试。'),btn('返回','close-dialog')+btn('重试','v3-model-stop-retry','','primary'));return;
  }
  const pending=review3.stopModel,changed=step==='confirm'&&(!pending||pending.id!==id||pending.key!==key||pending.signature!==JSON.stringify(current));
  if(changed||(state.scenario==='conflict'&&step==='request')){
    review3.stopModel={id,key};
    openDialog('目标配置在其他位置变化',banner('warn','原来的选择已保留，先重新核对','只处理 '+clients[key]+' 中的此模型，不取消正在处理的请求。'),btn('取消','close-dialog')+btn('重新核对','v3-model-stop-recheck','','primary'));return;
  }
  if(step==='recheck'){
    review3.stopModel={id,key,signature:JSON.stringify(current)};
    openDialog('在 '+clients[key]+' 停用此模型？',`<p>已重新核对「${e(review3Profile(id).label)}」。只移除这个客户端的使用配置，其他客户端和来源保留。</p><p>正在处理的请求仍继续。</p>`,btn('取消','close-dialog')+btn('停用此模型','v3-model-stop-confirm','','danger'));return;
  }
  const returnGrid=review3.stopModelReturn?.id===id&&review3.stopModelReturn.key===key?review3.stopModelReturn:null;
  delete review3Assignments[id][key];review3.stopModel=null;review3.stopModelReturn=null;closeDialog();render();
  if(returnGrid)review3ReuseChooser(id,returnGrid.origin,true);
  showToast('已在 '+clients[key]+' 停用此模型');
}
function review3Adjust(id,key){
  const p=review3Profile(id),a=review3Assignments[id][key];if(!a)return;
  review3.adjust={id,key};
  openDialog('调整 '+clients[key]+' 的模型参数',`<div class="reuse-origin"><span class="model-resource-icon">${mark(key)}</span><div><strong>${p.label}</strong><small>${ReviewUsage.sources[p.source]} · 独立配置</small></div></div><form id="v3-adjust-form"><div class="form-grid"><label class="field full">思考强度<select id="v3-effort">${review3Options(Object.fromEntries(['自动','none','low','medium','high','xhigh','max'].map(x=>[x,x])),a.effort)}</select><small>演示档位，真实支持按客户端、模型和协议确认。</small></label><label class="field">最大输出 Token<input id="v3-output" type="number" min="1" max="1000000" step="1" required value="${a.output}"></label><label class="field">温度<input id="v3-temperature" type="number" min="0" max="2" step="0.1" required value="${a.temperature}"></label></div></form>${hint('只修改 '+clients[key]+' 的当前条目。库和其他客户端保持原样。')}`,btn('取消','close-dialog')+btn('保存此端参数','v3-save-adjustment','','primary'));
}
function review3SourceUpdate(id){
  const p=review3Profile(id),rows=review3SourceVersions(id);
  openDialog('来源更新 · 选择同步范围',`<div class="reuse-origin"><span class="model-resource-icon">${svg('layers')}</span><div><strong>${p.label} · ${ReviewUsage.sources[p.source]}</strong><small>来源版本 ${p.version} · 客户端副本不会自动更新</small></div></div>${p.version>1?'<div class="source-change"><span>连接地址</span><div><del>api.example.com</del><strong>api.example.com/gateway</strong></div></div>':'<p class="note">当前来源没有新的连接变更。</p>'}<p>只同步专用副本的来源连接信息，保留各端已经调整的模型参数。Codex 继续使用普通路由，不生成专用渠道。</p><div class="target-list">${rows.map(([key,a])=>`<label class="target-choice"><input type="checkbox" name="v3-sync-target" value="${key}"><span class="app-icon">${mark(key)}</span><div><strong>${clients[key]}</strong><small>思考 ${a.effort} · 输出 ${review3Number(a.output)} · 保留</small></div>${badge('待同步')}</label>`).join('')||'<p class="note">当前客户端均已是此来源版本，无需重复同步。</p>'}</div>${hint('同步前仍核对 revision 与外部后改。未选择的客户端保持原样。')}`,btn('取消','close-dialog')+(rows.length?btn('同步所选客户端','v3-apply-source-update',id,'primary'):''));
}
function review3Action(action,value){
  if(!action.startsWith('v3-'))return false;
  switch(action){
    case 'v3-route-select':review3.routeFocus=review3.routeFocus===value?null:value;render();break;
    case 'v3-route-clear':review3.routeFocus=null;render();break;
    case 'v3-route-requests':{const r=review3RouteGraph().routes.find(r=>r.key===value);if(r)review3OpenRequestSet({scope:r.scope,key:r.key,status:'running'});break;}
    case 'v3-client-requests':{const [scope,status]=value.split(':');if(Object.hasOwn(ReviewUsage.scopes,scope)&&['running','failed','all'].includes(status))review3OpenRequestSet({scope,status});break;}
    case 'v3-route-channel':review3OpenChannel(value);break;
    case 'v3-channel-clear':review3.channelTarget=null;render();break;
    case 'v3-channel-records':{const r=review3ChannelRef(value);if(r)review3OpenRequestSet({scope:r.scope,key:value,status:'all'});break;}
    case 'v3-channel-maintain':{const r=review3ChannelRef(value);if(r)openDialog('此渠道的维护范围',status('客户端',e(clients[r.scope]))+status('条目',e(r.entry))+status('模型',e(r.model))+hint('启停、路由编辑和删除只处理此条目；继续原 revision、备份及后改保护。此处仅预览，不执行写入。'),btn('返回','close-dialog'));break;}
    case 'v3-set-request':review3RequestSetDetail(value);break;
    case 'v3-set-back':review3OpenRequestSet();break;
    case 'v3-set-page':if(review3.requestSet){review3.requestSet.page+=Number(value);review3OpenRequestSet();}break;
    case 'v3-pick-profile':if(review3Profiles[value])review3ReuseChooser(value);break;
    case 'v3-profile-settings':{const p=review3Profiles[value];if(!p)break;openDialog('模型设置 · '+p.label,status('模型',e(p.model))+status('来源',e(ReviewUsage.sources[p.source]))+status('基础参数','思考 '+e(p.effort)+' · 输出 '+review3Number(p.output))+hint('客户端分配与独立参数在各客户端的“模型”页管理。'),btn('关闭','close-dialog')+btn('管理来源模型','edit-source',p.source));break;}
    case 'v3-runtime-config':review3Action('v3-show-target',value||review3.flowScope);break;
    case 'v3-runtime-reconnect':review3.runtimeDemo='auto';review3.runtimeRecovered=true;render();showToast('演示：本机服务恢复，旧请求不继续流动，等待新的请求证据。');break;
    case 'v3-runtime-help':openPageGuide('home/runtime');break;
    case 'v3-runtime-support':review3RuntimeSupport();break;
    case 'v3-motion-toggle':{
      review3.motion=!review3.motion;document.documentElement.dataset.motion=review3.motion?'on':'off';
      const toggle=document.getElementById('motion-toggle');toggle.textContent=review3.motion?'暂停动效':'开启动效';toggle.setAttribute('aria-pressed',String(review3.motion));break;
    }
    case 'v3-home-tab':review3.homeTab=value;render();break;
    case 'v3-runtime':review3.homeTab='runtime';go('home');break;
    case 'v3-range':review3.range=value;review3.bucket=null;review3.recordPage=0;render();break;
    case 'v3-metric':review3.metric=value==='requests'?'requests':'tokens';render();break;
    case 'v3-all-records':review3.recordSearch='';review3.bucket=null;review3.recordStatus='all';review3.recordPage=0;go('logs');break;
    case 'v3-result':review3.recordSearch='';review3.recordStatus=value;review3.bucket=null;review3.recordPage=0;go('logs');break;
    case 'v3-clear-bucket':review3.bucket=null;review3.recordPage=0;render();break;
    case 'v3-back-usage':review3.homeTab='usage';go('home');break;
    case 'v3-record-page':review3.recordPage=Math.max(0,Math.min(Math.ceil(review3RecordRows().length/12)-1,review3.recordPage+Number(value)));render();break;
    case 'v3-breakdown':{review3.recordSearch='';const type=value.split(':')[0],key=value.slice(type.length+1);if(type==='scope')review3.scope=key;else review3.model=key;review3.bucket=null;review3.recordStatus='all';review3.recordPage=0;go('logs');break;}
    case 'v3-request':review3Request(value);break;
    case 'v3-request-source':review3.channelTarget=null;review3.sourceTab='channels';state.source=value;go('sources');break;
    case 'v3-reload':state.scenario='ready';document.getElementById('scenario').value='ready';render();showToast('已恢复合成统计视图，没有访问服务。');break;
    case 'v3-coverage':openDialog('统计口径',`${status('范围','经 Hub 记录且仍保留的数据')}${status('请求数','每个逻辑请求计一次，不把上游重试当新请求')}${status('完成率','完成数 ÷ 终态请求数，进行中不进入分母')}${status('输入用量','按上游用量归一，包含缓存，不重复相加或单列缓存数量')}${status('输出用量','仅汇总已上报的输出 Token，缺失用量保留未知')}${status('接入点','普通入口不推定为 Codex；专用入口按条目归属')}${status('费用','尚未估算，不把未知价格显示成免费')}`,btn('知道了','close-dialog','','primary'));break;
    case 'v3-source-tab':review3.sourceTab=value;if(value==='channels')review3.channelTarget=null;render();break;
    case 'v3-open-models':review3.preferredTarget=null;review3.sourceTab='models';closeDialog();go('sources');break;
    case 'v3-library-target':review3ModelPicker(value);break;
    case 'v3-clear-preferred':review3.preferredTarget=null;render();break;
    case 'v3-profile-source':review3.channelTarget=null;review3.sourceTab='channels';state.source=review3Profiles[value].source;render();break;
    case 'v3-client-node':{const [key,id]=value.split(':');review3ClientNode(key,id);break;}
    case 'v3-model-stop':{const [id,key]=value.split(':');review3.stopModelReturn=null;review3StopModel(id,key);break;}
    case 'v3-model-stop-retry':if(review3.stopModel)review3StopModel(review3.stopModel.id,review3.stopModel.key);break;
    case 'v3-model-stop-recheck':if(review3.stopModel)review3StopModel(review3.stopModel.id,review3.stopModel.key,'recheck');break;
    case 'v3-model-stop-confirm':if(review3.stopModel)review3StopModel(review3.stopModel.id,review3.stopModel.key,'confirm');break;
    case 'v3-reuse':{const [id,origin]=value.split(':');review3ReuseChooser(id,origin||'');break;}
    case 'v3-reuse-return':review3ReuseChooser(review3.reuse.id,review3.reuse.origin,true);break;
    case 'v3-reuse-target':review3ReuseTarget(value);break;
    case 'v3-reuse-recheck':review3ReuseTarget(review3.reuse.target,true);break;
    case 'v3-reuse-confirm':review3ApplyReuse(document.querySelector('input[name="v3-params-mode"]:checked')?.value==='keep');break;
    case 'v3-show-target':review3.preferredTarget=null;closeDialog();go(value);state.clientTab='models';render();break;
    case 'v3-adjust':{const [id,key]=value.split(':');review3Adjust(id,key);break;}
    case 'v3-save-adjustment':{
      const form=document.getElementById('v3-adjust-form');if(!form.reportValidity())break;
      const {id,key}=review3.adjust,a=review3Assignments[id][key];
      a.effort=document.getElementById('v3-effort').value;a.output=Number(document.getElementById('v3-output').value);a.temperature=Number(document.getElementById('v3-temperature').value);a.adjusted=true;
      closeDialog();render();showToast('演示：只保存 '+clients[key]+'，其他客户端和库配置保持原样。');break;
    }
    case 'v3-source-update':review3SourceUpdate(value);break;
    case 'v3-apply-source-update':{
      const chosen=[...document.querySelectorAll('input[name="v3-sync-target"]:checked')].map(x=>x.value);
      if(!chosen.length){showToast('先选择需要同步的客户端。');break;}
      if(state.scenario==='conflict'){openDialog('外部配置已变化',banner('warn','未写入，选择仍需重新核对','不会覆盖后改。返回来源更新时重新选择目标。'),btn('返回查看差异','v3-source-update',value,'primary'));break;}
      chosen.forEach(key=>{review3Assignments[value][key].version=review3Profile(value).version;});
      closeDialog();render();showToast('演示：已同步 '+chosen.map(k=>review3Short[k]).join('、')+' 的来源连接信息，保留各端参数。');break;
    }
    case 'v3-reuse-policy':openDialog('跨端复用规则',`${status('首次加入','兼容且无争议时直接创建目标端独立配置')}${status('独立调整','各端参数分别保存；Codex 继续普通路由，不锁定单一来源')}${status('来源更新','提示差异，由你选择同步目标与内容')}${status('自动跟随','保留为后续显式选项，目前不启用')}${hint('普通来源、客户端专用路由和凭据归属保持隔离。')}`,btn('知道了','close-dialog','','primary'));break;
    default:return false;
  }
  return true;
}
document.addEventListener('change',ev=>{
  if(ev.target.id==='v3-demo-scope'){
    if(!['workbuddy','tiangong','ordinary'].includes(ev.target.value))return;
    review3.flowScope=ev.target.value;review3.runtimeDemo='auto';review3.runtimeRecovered=false;render();return;
  }
  if(ev.target.id==='v3-runtime-demo'){
    const value=ev.target.value;
    if(!Object.hasOwn(review3RuntimeScenes,value)||['empty','stale','error'].includes(state.scenario)||(review3.flowScope==='ordinary'&&['stopped','waiting','mismatch'].includes(value)))return;
    review3.runtimeDemo=value;review3.runtimeRecovered=false;render();return;
  }
  const map={'v3-scope':'scope','v3-source':'source','v3-model':'model','v3-model-target':'modelTarget','v3-record-status':'recordStatus'};
  const key=map[ev.target.id];if(!key)return;
  review3[key]=ev.target.value;review3.bucket=null;review3.recordPage=0;render();
});
document.addEventListener('input',ev=>{
  const key={'v3-record-search':'recordSearch','v3-model-search':'modelQuery'}[ev.target.id];if(!key)return;
  const id=ev.target.id,position=ev.target.selectionStart;review3[key]=ev.target.value;review3.recordPage=0;render();
  const input=document.getElementById(id);input.focus();input.setSelectionRange(position,position);
});
function review3ChooseBucket(target){
  const index=Number(target.dataset.usageBucket),point=ReviewUsage.series(review3Filter())[index];if(!point)return;
  review3.recordSearch='';review3.bucket={from:point.from,to:point.to};review3.recordStatus='all';review3.recordPage=0;go('logs');
}
document.addEventListener('click',ev=>{const target=ev.target.closest('[data-usage-bucket]');if(target)review3ChooseBucket(target);});
document.addEventListener('keydown',ev=>{const target=ev.target.closest('[data-usage-bucket]');if(target&&(ev.key==='Enter'||ev.key===' ')){ev.preventDefault();review3ChooseBucket(target);}});
