'use strict';

// Synthetic review controls only. No filesystem, process, network, or persisted credentials.
const reviewClientNav={recent:['codex','workbuddy','tiangong'],expanded:false,sequence:0,lastUsed:{codex:-3,workbuddy:-2,tiangong:-1}};
const reviewTargetPickers={};
let reviewSelectSequence=0,reviewSelectPopup=null;
function reviewRememberClient(key){
  if(!clients[key])return;
  const nav=reviewClientNav;
  nav.recent=[...new Set(nav.recent.filter(k=>clients[k]))].slice(0,3);
  if(!nav.recent.length)nav.recent=['codex','workbuddy','tiangong'].filter(k=>clients[k]);
  nav.lastUsed[key]=++nav.sequence;
  if(nav.recent.includes(key))return;
  if(nav.recent.length<3){nav.recent.push(key);return;}
  const oldest=nav.recent.reduce((pick,k)=>(nav.lastUsed[k]??-Infinity)<(nav.lastUsed[pick]??-Infinity)?k:pick);
  nav.recent[nav.recent.indexOf(oldest)]=key;
}
function reviewNavDirectory(){
  const known=reviewClientNav.recent.filter(k=>clients[k]).map(key=>({key,name:clients[key]}));
  for(const key of Object.keys(clients))if(!known.some(c=>c.key===key))known.push({key,name:clients[key]});
  if(review3.runtimeDemo==='capacity')for(const letter of ['A','B','C','D','E'])known.push({key:'planned-'+letter,name:'客户端 '+letter,planned:true});
  return known;
}
function reviewRenderClientNav(){
  const host=document.getElementById('client-nav-list');if(!host)return;
  const rows=reviewNavDirectory(),visible=reviewClientNav.expanded?rows:rows.slice(0,3);
  host.innerHTML=visible.map(c=>`<button ${c.planned?`data-action="ui-client-planned" data-value="${c.key}"`:`data-nav="${c.key}"`} class="nav-item" data-client-nav="${c.key}" ${c.key===state.page?'aria-current="page"':''}>${c.planned?svg('plug'):mark(c.key)}<span>${e(c.name)}</span>${c.planned?'<small>规划</small>':''}</button>`).join('');
  const toggle=document.getElementById('client-nav-toggle'),more=document.getElementById('client-nav-more');
  toggle.hidden=!reviewClientNav.expanded;more.hidden=reviewClientNav.expanded;
  toggle.setAttribute('aria-expanded',String(reviewClientNav.expanded));more.setAttribute('aria-expanded',String(reviewClientNav.expanded));
  toggle.setAttribute('aria-label','收起，仅显示最近使用的 3 个客户端');
  toggle.innerHTML='收起 '+svg('chevron');
}

function reviewClientTile(key,options={}){
  const {on=false,action='',value='',role='switch',status='',disabled=false,unsupported=false,locked=false,id=''}=options;
  const label=options.label||(clients[key]+' · '+(on?'使用中，点击停用':'未使用，点击使用'));
  return `<button type="button" ${id?`id="${e(id)}"`:''} class="client-use-tile ${on?'is-on':''} ${unsupported?'is-unsupported':''}" data-target-filter="${e(clients[key])}" data-action="${action}" data-value="${e(value)}" role="${role}" aria-checked="${on}" aria-label="${e(label)}" ${disabled||locked?'disabled':''}>${appIcon(key)}<strong>${e(clients[key])}</strong>${status?`<small>${e(status)}</small>`:''}<i class="client-use-check" aria-hidden="true">${on?'✓':unsupported?'?':''}</i></button>`;
}
function reviewClientGrid(tiles,count){return `<div class="client-targets"><div class="client-target-grid ${count===2?'target-two':count<=3?'target-three':''}">${tiles}</div></div>`;}
function reviewTargetField(id,selected,options={}){
  const keys=Object.keys(clients).filter(k=>!options.exclude?.includes(k));
  reviewTargetPickers[id]={selected:new Set(selected.filter(k=>keys.includes(k))),keys,open:keys.length<=4,...options};
  return `<div class="target-picker" id="${id}">${reviewTargetFieldBody(id)}</div>`;
}
function reviewTargetFieldBody(id){
  const p=reviewTargetPickers[id],selected=p.keys.filter(k=>p.selected.has(k));
  const keys=p.locked?selected:p.keys,tiles=keys.map(k=>reviewClientTile(k,{id:id+'-'+k,on:p.selected.has(k),action:'ui-target-pick',value:id+':'+k,role:p.single?'radio':'checkbox',locked:p.locked,label:clients[k]+' · '+(p.selected.has(k)?'已选用':'未选用')+(p.locked?' · 当前客户端':'')})).join('');
  if(p.locked)return reviewClientGrid(tiles,keys.length);
  const summary=p.keys.length>4?`<div class="target-selection">${selected.map(k=>`<span class="target-token">${appIcon(k)}${e(review3Short[k]||clients[k])}</span>`).join('')||'<span>选择客户端</span>'}<button class="btn link" data-action="ui-target-open" data-value="${id}" aria-expanded="${!!p.open}" aria-controls="${id}-panel">${p.open?'收起':'更改'}</button></div>`:'';
  return summary+`<div class="target-picker-panel" id="${id}-panel" ${p.open?'':'hidden'}>${p.keys.length>4?searchInput(id+'-search','搜索已接入的客户端'):''}<div class="target-picker-options">${reviewClientGrid(tiles,keys.length)}</div></div>`;
}
function reviewTargetValues(id){return [...(reviewTargetPickers[id]?.selected||[])];}
function reviewTargetPick(id,key){
  const p=reviewTargetPickers[id];if(!p||p.locked||!p.keys.includes(key))return;
  if(p.single){p.selected=new Set([key]);p.open=false;}else if(p.selected.has(key))p.selected.delete(key);else p.selected.add(key);
  if(id==='wizard-target')state.wizardClient=key;
  document.getElementById(id).innerHTML=reviewTargetFieldBody(id);hydrate(document.getElementById(id));
  document.getElementById(id+'-'+key)?.focus();
}

// Native selects retain their values/change handlers; one accessible popup gives a consistent surface.
function reviewCloseSelect(restoreFocus=false){
  const p=reviewSelectPopup;if(!p)return;
  p.trigger.setAttribute('aria-expanded','false');p.trigger.removeAttribute('aria-activedescendant');p.popup.remove();reviewSelectPopup=null;
  if(restoreFocus&&p.trigger.isConnected)p.trigger.focus();
}
function reviewSelectItems(select){return Array.from(select.options).map((o,index)=>({index,label:o.textContent,value:o.value,disabled:o.disabled||o.parentElement?.disabled}));}
function reviewSelectVisible(p){return reviewSelectItems(p.select).filter(o=>!p.query||o.label.toLowerCase().includes(p.query));}
function reviewPaintSelect(){
  const p=reviewSelectPopup;if(!p)return;
  const rows=reviewSelectVisible(p);
  if(!rows.some(o=>o.index===p.active&&!o.disabled))p.active=rows.find(o=>!o.disabled)?.index??-1;
  p.list.innerHTML=rows.map(o=>`<button type="button" id="${p.id}-option-${o.index}" class="review-select-option" role="option" tabindex="-1" aria-selected="${o.index===p.select.selectedIndex}" ${o.disabled?'disabled':''} data-select-index="${o.index}" ${o.index===p.active?'data-highlighted="true"':''}><span>${e(o.label)}</span><i aria-hidden="true">${o.index===p.select.selectedIndex?'✓':''}</i></button>`).join('')||'<div class="select-no-match">没有匹配项</div>';
  if(p.active>=0){const id=p.id+'-option-'+p.active;p.trigger.setAttribute('aria-activedescendant',id);p.input?.setAttribute('aria-activedescendant',id);p.list.querySelector('[data-highlighted]')?.scrollIntoView({block:'nearest'});}
}
function reviewSelectChoose(index){
  const p=reviewSelectPopup;if(!p||p.select.disabled||p.select.options[index]?.disabled)return;
  p.select.selectedIndex=index;p.trigger.querySelector('span').textContent=p.select.options[index].textContent;
  reviewCloseSelect(true);p.select.dispatchEvent(new Event('change',{bubbles:true}));
  if(p.select.id){const current=document.getElementById(p.select.id);current?.parentElement?.querySelector('.review-select-trigger')?.focus();}
}
function reviewSelectKey(ev){
  const p=reviewSelectPopup;if(!p)return;
  if(ev.key==='Escape'){ev.preventDefault();ev.stopPropagation();reviewCloseSelect(true);return;}
  if(ev.key==='Tab'){reviewCloseSelect(ev.target===p.input);return;}
  const rows=reviewSelectVisible(p).filter(o=>!o.disabled);if(!rows.length)return;
  let pos=rows.findIndex(o=>o.index===p.active);
  if(['ArrowDown','ArrowUp','Home','End'].includes(ev.key)){
    ev.preventDefault();ev.stopPropagation();pos=ev.key==='Home'?0:ev.key==='End'?rows.length-1:Math.max(0,Math.min(rows.length-1,pos+(ev.key==='ArrowDown'?1:-1)));p.active=rows[pos].index;reviewPaintSelect();
  }else if(ev.key==='Enter'||ev.key===' '&&ev.target===p.trigger){ev.preventDefault();ev.stopPropagation();reviewSelectChoose(p.active);}
}
function reviewOpenSelect(select,trigger){
  if(select.disabled)return;
  if(reviewSelectPopup?.select===select){reviewCloseSelect();return;}
  reviewCloseSelect();
  const popup=document.createElement('div'),list=document.createElement('div'),id='review-select-'+(++reviewSelectSequence),searchable=select.options.length>=6;
  popup.className='review-select-popup';list.className='review-select-list';list.id=id;list.setAttribute('role','listbox');list.setAttribute('aria-label',trigger.getAttribute('aria-label'));
  const parent=trigger.closest('dialog')||document.body;parent.appendChild(popup);
  let input=null;
  if(searchable){input=document.createElement('input');input.type='search';input.className='review-select-search';input.placeholder='搜索选项';input.setAttribute('aria-label','搜索选项');input.setAttribute('role','combobox');input.setAttribute('aria-controls',id);input.setAttribute('aria-expanded','true');popup.appendChild(input);}
  popup.appendChild(list);reviewSelectPopup={select,trigger,popup,list,input,id,query:'',active:select.selectedIndex};
  const r=trigger.getBoundingClientRect(),width=Math.min(Math.max(r.width,200),window.innerWidth-24),spaceBelow=window.innerHeight-r.bottom-12,spaceAbove=r.top-12,maxHeight=Math.min(310,Math.max(spaceAbove,spaceBelow));
  popup.style.width=width+'px';popup.style.maxHeight=maxHeight+'px';popup.style.left=Math.max(12,Math.min(r.left,window.innerWidth-width-12))+'px';
  if(spaceBelow>=Math.min(260,spaceAbove))popup.style.top=(r.bottom+6)+'px';else popup.style.bottom=(window.innerHeight-r.top+6)+'px';
  trigger.setAttribute('aria-controls',id);trigger.setAttribute('aria-expanded','true');reviewPaintSelect();
  popup.addEventListener('click',ev=>{const option=ev.target.closest('[data-select-index]');if(option){ev.stopPropagation();reviewSelectChoose(Number(option.dataset.selectIndex));}});
  popup.addEventListener('keydown',reviewSelectKey);
  if(input){input.addEventListener('input',()=>{if(!reviewSelectPopup)return;reviewSelectPopup.query=input.value.trim().toLowerCase();reviewPaintSelect();});input.focus();}
}
function reviewEnhanceSelects(root=document){
  root.querySelectorAll('select').forEach(select=>{
    if(select.multiple)return;
    if(select.dataset.reviewSelect){const trigger=select.parentElement.querySelector('.review-select-trigger');if(trigger){trigger.disabled=select.disabled;trigger.querySelector('span').textContent=select.selectedOptions[0]?.textContent||'请选择';}return;}
    select.dataset.reviewSelect='true';
    const wrapper=document.createElement('span'),trigger=document.createElement('button'),label=select.getAttribute('aria-label')||Array.from(select.labels?.[0]?.childNodes||[]).filter(n=>n.nodeType===3).map(n=>n.textContent).join(' ').trim()||select.closest('.toggle-row')?.querySelector('div')?.textContent||'选择选项';
    wrapper.className='review-select';select.before(wrapper);wrapper.appendChild(select);select.classList.add('review-native-select');select.tabIndex=-1;select.setAttribute('aria-hidden','true');
    trigger.type='button';trigger.className='review-select-trigger';trigger.disabled=select.disabled;trigger.setAttribute('role','combobox');trigger.setAttribute('aria-label',label);trigger.setAttribute('aria-haspopup','listbox');trigger.setAttribute('aria-expanded','false');
    trigger.innerHTML=`<span>${e(select.selectedOptions[0]?.textContent||'请选择')}</span>${svg('chevron')}`;wrapper.appendChild(trigger);
    trigger.addEventListener('click',()=>reviewOpenSelect(select,trigger));
    trigger.addEventListener('keydown',ev=>{if(reviewSelectPopup?.select===select){reviewSelectKey(ev);return;}if(['ArrowDown','ArrowUp','Home','End'].includes(ev.key)){ev.preventDefault();reviewOpenSelect(select,trigger);}});
    select.addEventListener('change',()=>{trigger.querySelector('span').textContent=select.selectedOptions[0]?.textContent||'请选择';});
  });
}

const reviewDumpling={
  selected:'example',editing:false,draft:null,error:'',scene:'ready',
  profiles:{example:{id:'example',name:'产品环境 · 示例',productBaseUrl:'https://nvwa.example.com/product',authMode:'password',username:'demo-user',password:'DEMO-ONLY',rememberPassword:false,clientId:'',clientSecret:'',rememberSecret:false,certificationBaseUrl:'',mcpUrl:'/mcp',mcpAuthHeader:'Authorization',tenant:'',loginUnit:'',signatureAlgorithm:'SHA-256',authorized:true,connections:{codex:true,tiangong:true}}}
};
function reviewNewEnvironment(){return {id:'',name:'新环境 · 示例',productBaseUrl:'https://nvwa.example.com/product',authMode:'password',username:'',password:'',rememberPassword:false,clientId:'',clientSecret:'',rememberSecret:false,certificationBaseUrl:'',mcpUrl:'/mcp',mcpAuthHeader:'Authorization',tenant:'',loginUnit:'',signatureAlgorithm:'SHA-256',authorized:false,connections:{}};}
const reviewDumplingIds={name:'mcp-name',productBaseUrl:'mcp-url',username:'mcp-username',password:'mcp-password',clientId:'mcp-client-id',clientSecret:'mcp-client-secret',certificationBaseUrl:'mcp-certification',mcpUrl:'mcp-path',mcpAuthHeader:'mcp-header',tenant:'mcp-tenant',loginUnit:'mcp-unit',signatureAlgorithm:'mcp-signature'};
function reviewDumplingProfile(){return reviewDumpling.profiles[reviewDumpling.selected];}
function reviewDumplingUsed(key){return state.scenario!=='empty'&&Object.values(reviewDumpling.profiles).some(p=>p.authorized&&p.connections[key]);}
function reviewDumplingCapture(){
  const d=reviewDumpling.draft;if(!d)return;
  for(const [key,id] of Object.entries(reviewDumplingIds)){const n=document.getElementById(id);if(n)d[key]=n.value;}
  for(const [key,id] of [['rememberPassword','mcp-remember-password'],['rememberSecret','mcp-remember-secret']]){const n=document.getElementById(id);if(n)d[key]=n.checked;}
}
function reviewDumplingMode(mode){
  reviewDumplingCapture();const d=reviewDumpling.draft;if(!d)return;
  if(d.authMode===mode)return;
  const sameFamily=d.authMode!=='password'&&mode!=='password',kept=sameFamily?{clientId:d.clientId,clientSecret:d.clientSecret,rememberSecret:d.rememberSecret}:{};
  Object.assign(d,{authMode:mode,username:'',password:'',clientId:'',clientSecret:'',rememberSecret:false,rememberPassword:false,tenant:'',loginUnit:'',signatureAlgorithm:'SHA-256',mcpAuthHeader:mode==='password'?'Authorization':'authorization-ticket-token'},kept);
  reviewDumpling.error='';render();
}
function reviewEnvironmentEditor(){
  const d=reviewDumpling.draft,password=d.authMode==='password',shared=d.authMode==='application';
  const field=(label,key,attrs='')=>`<label class="field">${label}<input id="${reviewDumplingIds[key]}" value="${e(d[key]||'')}" ${attrs}></label>`;
  return `<section class="usage-panel environment-editor"><div class="section-heading"><h2>${d.id?'编辑环境':'配置环境'}</h2></div><div class="form-grid">${field('环境名称','name','maxlength="80"')}${field('NVWA 服务地址','productBaseUrl','placeholder="https://nvwa.example.com/product"')}<div class="field full"><span>认证方式</span><div class="choice-strip">${[['password','账号密码'],['application','认证服务连接']].map(([v,t])=>`<button class="choice-pill ${password===(v==='password')?'selected':''}" aria-pressed="${password===(v==='password')}" data-action="ui-dumpling-mode" data-value="${v==='password'?'password':password?'application':d.authMode}">${t}</button>`).join('')}</div></div>${password?`${field('账号','username')}${field('密码','password','autocomplete="off"')}<label class="checkbox full"><input id="mcp-remember-password" type="checkbox" ${d.rememberPassword?'checked':''}>记住密码</label>`:`<label class="checkbox full"><input id="mcp-shared" type="checkbox" ${shared?'checked':''}>使用共享应用代表指定用户</label>${field('ClientID','clientId')}${field('ClientSecret','clientSecret','autocomplete="off"')}<label class="checkbox full"><input id="mcp-remember-secret" type="checkbox" ${d.rememberSecret?'checked':''}>保存应用密钥</label>${shared?field('代表的账号','username'):''}<div class="full auth-application-help">在认证服务管理添加应用服务，获取 ClientID 和 ClientSecret。${shared?'':'个人登录将在浏览器中完成。'}</div>`}</div><details class="subsection"><summary>高级设置</summary><div class="form-grid advanced-fields">${field('独立认证服务地址','certificationBaseUrl','placeholder="留空使用 NVWA 服务地址"')}${field('MCP 地址','mcpUrl')}${field('MCP 认证头','mcpAuthHeader')}${field('指定租户（可选）','tenant')}${password?field('登录机构（可选）','loginUnit'):shared?`<label class="field">应用签名算法<select id="mcp-signature">${['SHA-256','SM3','MD5'].map(v=>`<option ${d.signatureAlgorithm===v?'selected':''}>${v}</option>`).join('')}</select></label>`:field('限定授权账号（可选）','username')}</div></details><p id="mcp-form-error" class="field-error" role="alert">${e(reviewDumpling.error)}</p><div class="form-footer">${d.id?btn('取消编辑','ui-dumpling-cancel'):''}${btn('保存环境并继续','mcp-save','','primary')}</div></section>`;
}
function reviewDumplingPage(){
  const m=reviewDumpling;
  if(state.scenario==='empty'&&m.scene!=='empty'){m.scene='empty';m.editing=true;m.draft=reviewNewEnvironment();}
  else if(state.scenario!=='empty'&&m.scene==='empty'){m.scene=state.scenario;if(!m.draft?.id){m.editing=false;m.draft=null;}}
  const p=reviewDumplingProfile(),unread=state.scenario==='stale',offline=state.scenario==='error';
  let html=`<div class="dumpling-toolbar"><label class="field">环境<select id="ui-dumpling-environment">${m.editing&&!m.draft?.id?'<option value="__new__" selected>新建环境</option>':''}${Object.values(m.profiles).map(v=>`<option value="${v.id}" ${v.id===m.selected&&(!m.editing||m.draft?.id)?'selected':''}>${e(v.name)}</option>`).join('')}</select></label><div class="actions">${btn('+ 添加环境','mcp-new')}${!m.editing?btn('编辑环境','mcp-edit'):''}${btn('认证与配置说明','mcp-help','','link')}</div></div>`;
  const step=m.editing?1:!p?.authorized?2:3;
  html+=`<div class="dumpling-progress" aria-label="环境接入进度">${['配置环境','登录 / 授权','连接客户端'].map((label,i)=>`<span class="${step===i+1?'current':step>i+1?'done':''}"><b>${step>i+1?'✓':i+1}</b>${label}</span>`).join('')}</div>`;
  if(m.editing)return html+reviewEnvironmentEditor();
  if(!p)return html;
  const mode=p.authMode==='password'?'账号密码':p.authMode==='application'?'共享应用代表用户':'浏览器授权',connected=!!p.connections[state.mcpClient];
  html+=`<section class="usage-panel environment-summary"><div><h2>${e(p.name)}</h2><span>${e(mode)}</span></div><div class="actions">${badge(unread?'当前未知':p.authorized?'已授权':'待授权',!unread&&p.authorized?'ok':'')}${btn(p.authorized?'重新授权':'登录 / 授权','mcp-login','',p.authorized?'':'primary')}</div></section>`;
  if(!p.authorized)return html+`<details class="usage-panel environment-maintenance"><summary>环境资料管理</summary><div class="form-footer">${btn('删除当前环境','mcp-delete','','danger')}</div></details>`;
  html+=`<section class="usage-panel"><div class="section-heading"><h2>连接客户端</h2></div><div class="toolbar"><label class="field dumpling-client-field">客户端<select id="mcp-client">${Object.keys(clients).map(k=>`<option value="${k}" ${state.mcpClient===k?'selected':''}>${e(clients[k])}</option>`).join('')}</select></label>${btn('连接并检测','mcp-connect','','primary')}${badge(unread?'读取失败':offline?'检测未完成':connected?'使用中':'未使用',!unread&&!offline&&connected?'ok':'')}</div>${unread?banner('warn','暂时无法读取环境状态','保留原有连接。',btn('重新读取','v3-reload')):offline?banner('warn','本次检测未完成','已有配置保留，可恢复后重试。'):''}<div class="tool-summary">${connected&&!unread&&!offline?['文档查询','业务信息读取','任务处理'].map(v=>badge(v)).join(''):'尚无工具摘要'}</div><details class="subsection"><summary>连接详情</summary>${status('当前身份',e(p.username||'demo-user')+' · 示例默认租户')}${status('本机桥',unread?'当前未知':offline?'本次未确认':connected?'最近一次检测通过':'尚未检测')}${status('工具目录',!unread&&!offline&&connected?'6 个模块 · 24 个工具':'尚未读取')}${status('工具调用','尚未验证')}<label class="field">客户端配置位置<input placeholder="自动查找" ${state.mcpClient==='tiangong'?'disabled value="官方本机设置"':''}></label></details><details class="subsection"><summary>当前客户端维护</summary><div class="actions">${btn('断开当前客户端','mcp-disconnect')}${btn('撤销上次连接更改','mcp-undo')}</div></details></section><details class="usage-panel environment-maintenance"><summary>环境资料管理</summary><div class="form-footer">${btn('删除当前环境','mcp-delete','','danger')}</div></details>`;
  return html;
}
function reviewValidHttp(value){try{const u=new URL(value);return ['http:','https:'].includes(u.protocol)&&!u.username&&!u.password&&!u.search&&!u.hash;}catch{return false;}}
function reviewDumplingSave(){
  reviewDumplingCapture();const m=reviewDumpling,d=m.draft;if(!d)return;
  let error=!d.name?.trim()?'请填写环境名称。':!reviewValidHttp(d.productBaseUrl)?'请填写有效的 NVWA HTTP(S) 服务地址。':d.certificationBaseUrl&&!reviewValidHttp(d.certificationBaseUrl)?'请核对独立认证服务地址。':d.mcpUrl&&!d.mcpUrl.startsWith('/')&&!reviewValidHttp(d.mcpUrl)?'MCP 地址应为路径或完整 HTTP(S) 地址。':d.authMode!=='password'&&(!d.clientId||!d.clientSecret)?'请填写 ClientID 和 ClientSecret。':d.authMode==='application'&&!d.username?'请填写代表的账号。':'';
  if(state.scenario==='stale')error='无法读取当前环境，已保留输入。恢复后再保存。';
  if(state.scenario==='conflict')error='环境已在其他位置变化，已保留输入。请先重新核对。';
  if(error){m.error=error;document.getElementById('mcp-form-error').textContent=error;return false;}
  const old=m.profiles[d.id],keys=['productBaseUrl','authMode','username','clientId','clientSecret','password','certificationBaseUrl','mcpUrl','mcpAuthHeader','tenant','loginUnit','signatureAlgorithm','rememberSecret','rememberPassword'];
  const changed=!old||keys.some(k=>old[k]!==d[k]);
  const id=d.id||'demo-environment-'+(Object.keys(m.profiles).length+1);
  m.profiles[id]={...d,id,authorized:!changed&&!!old.authorized,connections:{...old?.connections}};
  m.selected=id;m.draft=null;m.editing=false;m.error='';state.mcpStage=m.profiles[id].authorized?'connected':'auth';render();showToast('环境已保存到原型内存');return true;
}
function reviewDumplingLogin(){
  const p=reviewDumplingProfile();if(!p)return;
  p.authorized=false;render();
  const password=p.authMode==='password';
  const fields=password?`<div class="form-grid"><label class="field">账号<input id="ui-login-user" value="${e(p.username)}"></label><label class="field">密码<input id="ui-login-password" value="${e(p.password)}" autocomplete="off"></label></div>`:`${status('认证方式',p.authMode==='application'?'共享应用代表用户':'浏览器个人授权')}${status('ClientID',e(p.clientId))}${p.authMode==='application'?status('代表账号',e(p.username)):''}`;
  openDialog('登录 / 授权 · '+p.name,fields+'<p id="ui-login-error" role="alert" class="field-error"></p>',btn('取消','close-dialog')+btn(password?'演示登录成功':p.authMode==='application'?'演示签名授权成功':'演示浏览器授权成功','mcp-auth-done','','primary'));
}
function reviewDumplingAction(action,value){
  const m=reviewDumpling,p=reviewDumplingProfile();
  switch(action){
    case 'mcp-new':m.editing=true;m.draft=reviewNewEnvironment();m.error='';state.mcpView='dumpling';render();break;
    case 'mcp-edit':m.editing=true;m.draft={...p};m.error='';render();break;
    case 'ui-dumpling-cancel':m.editing=false;m.draft=null;m.error='';render();break;
    case 'ui-dumpling-mode':reviewDumplingMode(value);break;
    case 'mcp-save':reviewDumplingSave();break;
    case 'mcp-login':reviewDumplingLogin();break;
    case 'mcp-auth-done':{
      if(!p)return true;
      if(p.authMode==='password'){const user=document.getElementById('ui-login-user')?.value.trim(),password=document.getElementById('ui-login-password')?.value;if(!user||!password){document.getElementById('ui-login-error').textContent='请填写示例账号和密码。';return true;}p.username=user;p.password=password;}
      if(state.scenario==='stale'||state.scenario==='error'){document.getElementById('ui-login-error').textContent='本次授权未完成，保留原环境后重试。';return true;}
      p.authorized=true;state.mcpStage='connected';closeDialog();render();showToast('演示授权成功，可以连接客户端');break;
    }
    case 'mcp-connect':
      if(!p?.authorized){reviewDumplingLogin();break;}
      if(state.scenario==='stale'||state.scenario==='conflict'){openDialog('暂时不能连接',banner('warn','当前配置需要重新核对','已有连接保持原样。'),btn('返回','close-dialog')+btn('重新读取','v3-reload','','primary'));break;}
      if(state.scenario==='error'){openDialog('本次连接检测未完成',banner('warn','原配置已保留','恢复连接后可重试。'),btn('返回','close-dialog')+btn('重试','mcp-connect','','primary'));break;}
      openDialog('连接并检测 · '+clients[state.mcpClient],status('环境',e(p.name))+status('客户端',e(clients[state.mcpClient]))+hint('程序核对所选连接，按需备份配置并读取工具目录。此处仅演示。'),btn('取消','close-dialog')+btn('演示确认并继续','mcp-done','','primary'));break;
    case 'mcp-done':if(p?.authorized&&!['stale','conflict','error'].includes(state.scenario)){p.connections[state.mcpClient]=true;closeDialog();render();showToast('当前客户端已使用此环境 · 合成演示');}break;
    case 'mcp-disconnect':openDialog('断开 '+clients[state.mcpClient]+'？','<p>只断开当前环境在此客户端的连接，环境及其他客户端保留。</p>',btn('取消','close-dialog')+btn('断开当前客户端','ui-dumpling-disconnect-confirm','','danger'));break;
    case 'ui-dumpling-disconnect-confirm':if(p&&!['stale','conflict'].includes(state.scenario)){delete p.connections[state.mcpClient];closeDialog();render();}break;
    case 'mcp-delete':if(p&&Object.values(p.connections).some(Boolean)){openDialog('此环境仍在客户端使用','<p>先在当前客户端维护中分别断开连接，再删除环境资料。</p>',btn('返回处理','close-dialog','','primary'));}else openDialog('删除此环境资料？',status('环境',e(p?.name||''))+'<p>删除环境与本原型内存中的认证资料。</p>',btn('取消','close-dialog')+btn('删除环境','ui-dumpling-delete-confirm','','danger'));break;
    case 'ui-dumpling-delete-confirm':if(p&&!Object.values(p.connections).some(Boolean)){delete m.profiles[p.id];m.selected=Object.keys(m.profiles)[0];m.editing=!m.selected;m.draft=m.editing?reviewNewEnvironment():null;closeDialog();render();}break;
    default:return false;
  }
  return true;
}
function reviewRefinementAction(action,value){
  if(reviewDumplingAction(action,value))return true;
  switch(action){
    case 'ui-ready-scene':state.scenario='ready';document.getElementById('scenario').value='ready';render();break;
    case 'ui-client-nav-toggle':reviewClientNav.expanded=!reviewClientNav.expanded;reviewRenderClientNav();break;
    case 'ui-client-planned':openDialog('未来客户端位置','<p>这是显式八端容量场景中的规划占位，用于查看展开布局。</p>',btn('关闭','close-dialog'));break;
    case 'ui-target-open':{const p=reviewTargetPickers[value];if(p){p.open=!p.open;document.getElementById(value).innerHTML=reviewTargetFieldBody(value);hydrate(document.getElementById(value));}break;}
    case 'ui-target-pick':{const [id,key]=value.split(':');reviewTargetPick(id,key);break;}
    case 'ui-model-target':{
      const [id,origin,key]=value.split(':');if(!review3Profiles[id]||!clients[key]||key===origin||state.scenario==='stale')break;
      review3.reuse={id,origin,target:key,returnChooser:true};
      if(review3Assignments[id][key]){review3.stopModelReturn={id,key,origin};review3StopModel(id,key);}
      else{review3.stopModelReturn=null;review3ReuseTarget(key);}
      break;
    }
    default:return false;
  }
  return true;
}
document.addEventListener('pointerdown',ev=>{const p=reviewSelectPopup;if(p&&!p.popup.contains(ev.target)&&!p.trigger.contains(ev.target))reviewCloseSelect();});
document.addEventListener('keydown',ev=>{if(ev.key==='Escape'&&reviewSelectPopup){ev.preventDefault();ev.stopPropagation();reviewCloseSelect(true);}},true);
document.addEventListener('scroll',ev=>{if(reviewSelectPopup&&!reviewSelectPopup.popup.contains(ev.target))reviewCloseSelect();},true);
window.addEventListener('resize',()=>reviewCloseSelect());
document.addEventListener('input',ev=>{
  const picker=ev.target.closest?.('.target-picker');
  if(picker&&ev.target.type==='search'){const q=ev.target.value.toLowerCase();picker.querySelectorAll('[data-target-filter]').forEach(n=>n.hidden=!n.dataset.targetFilter.toLowerCase().includes(q));}
  if(Object.values(reviewDumplingIds).includes(ev.target.id))reviewDumplingCapture();
  if(ev.target.id==='cap-target-search'){const q=ev.target.value.toLowerCase();document.getElementById('dialog-body').querySelectorAll('[data-target-filter]').forEach(n=>n.hidden=!n.dataset.targetFilter.toLowerCase().includes(q));}
});
document.addEventListener('change',ev=>{
  if(ev.target.id==='ui-dumpling-environment'){reviewDumpling.selected=ev.target.value;reviewDumpling.editing=false;reviewDumpling.draft=null;reviewDumpling.error='';render();}
  if(ev.target.id==='mcp-shared')reviewDumplingMode(ev.target.checked?'application':'browser');
  if(ev.target.id==='cap-import-auth'){capabilityCaptureImport();capabilityRenderImport();}
});
