'use strict';

// Synthetic editor only: no commands, client files or service requests.
function reviewMcpBegin(){
  Object.assign(state.capabilityDraft,{entry:'configure',view:'form',name:'',description:'',source:'',command:'',argsRows:[],envPassRows:[],envPairs:[['','']],headerPairs:[['','']],imported:null,importedAuth:null,envelope:{},transport:'http',jsonText:'',candidates:[],issue:''});
  reviewMcpRender();
}
function reviewMcpCapture(){
  const d=state.capabilityDraft;if(d?.kind!=='mcp'||!d.entry)return;
  if(d.view==='json'){
    const node=document.getElementById('cap-import-json');if(node)d.jsonText=node.value;
  }else if(d.entry==='configure'){
    for(const key of ['name','description','source','command','token','headerName','cwd']){const node=document.getElementById('cap-import-'+key);if(node)d[key]=node.value;}
    for(const [key,prefix] of [['envPairs','env'],['headerPairs','header']])d[key]=d[key].map((pair,i)=>[
      document.getElementById(`mcp-${prefix}-key-${i}`)?.value??pair[0],document.getElementById(`mcp-${prefix}-value-${i}`)?.value??pair[1]
    ]);
    for(const [key,prefix] of [['argsRows','arg'],['envPassRows','pass']])d[key]=d[key].map((value,i)=>document.getElementById(`mcp-${prefix}-${i}`)?.value??value);
  }
  if(document.getElementById('cap-import-target'))d.targets=reviewTargetValues('cap-import-target');
}
function reviewMcpField(key,label,attrs=''){
  const d=state.capabilityDraft;return `<label class="field">${label}<input id="cap-import-${key}" value="${e(d[key]||'')}" ${attrs}></label>`;
}
function reviewMcpPairFields(key,prefix,label){
  return `<section class="mcp-pairs" aria-label="${label}"><div class="mcp-inline"><h3>${label}</h3>${btn('＋ 添加','mcp-entry-pair-add',prefix,'link')}</div>${state.capabilityDraft[key].map((pair,i)=>`<div class="mcp-pair-row"><input id="mcp-${prefix}-key-${i}" value="${e(pair[0])}" placeholder="名称" aria-label="${label}名称 ${i+1}" spellcheck="false"><input id="mcp-${prefix}-value-${i}" type="password" autocomplete="off" value="${e(pair[1])}" placeholder="值" aria-label="${label}值 ${i+1}"><button class="icon-button" aria-label="删除${label} ${i+1}" data-action="mcp-entry-pair-remove" data-value="${prefix}:${i}">×</button></div>`).join('')}</section>`;
}
function reviewMcpListFields(key,prefix,label,placeholder){
  return `<section class="mcp-pairs" aria-label="${label}"><div class="mcp-inline"><h3>${label}</h3>${btn('＋ 添加','mcp-entry-row-add',prefix,'link')}</div>${state.capabilityDraft[key].map((value,i)=>`<div class="mcp-value-row"><input id="mcp-${prefix}-${i}" value="${e(value)}" placeholder="${placeholder}" aria-label="${label} ${i+1}" spellcheck="false"><button class="icon-button" aria-label="删除${label} ${i+1}" data-action="mcp-entry-row-remove" data-value="${prefix}:${i}">×</button></div>`).join('')}</section>`;
}
function reviewMcpRender(){
  const d=state.capabilityDraft;if(!d?.entry)return;
  const json=d.view==='json',local=d.type==='1';let content='';
  if(d.entry==='choose'){
    content=`<div class="mcp-inline"><h3>选择要添加的服务</h3>${btn('返回 JSON','mcp-entry-back','','link')}</div><div class="mcp-service-list">${d.candidates.map((item,i)=>`<button class="mcp-existing" data-action="mcp-entry-candidate" data-value="${i}"><strong>${e(item.name)}</strong><span>${e(item.service?.command||item.service?.url||'配置待核对')}</span>${svg('chevron')}</button>`).join('')}</div>`;
  }else if(json){
    content=`<label class="field mcp-json-label" for="cap-import-json">MCP 配置 JSON</label><textarea id="cap-import-json" class="mcp-json-editor" rows="14" spellcheck="false" autocapitalize="off" aria-describedby="cap-import-error">${e(d.jsonText)}</textarea>`;
  }else{
    const connection=local?'stdio':d.transport;
    content=`<div class="mcp-basic-grid">${reviewMcpField('name','服务名称','placeholder="为这个服务取一个名称" maxlength="80"')}${reviewMcpField('description','简介（可选）','placeholder="这个服务有什么用途"')}</div>
      <section class="mcp-section"><div class="mcp-inline"><h3>连接类型</h3><div class="choice-strip" aria-label="连接类型">${[['http','Streamable HTTP'],['stdio','STDIO'],['sse','SSE']].map(([value,label])=>`<button class="choice-pill ${connection===value?'selected':''}" aria-pressed="${connection===value}" data-action="mcp-entry-connection" data-value="${value}">${label}</button>`).join('')}</div></div>
      ${local?`${reviewMcpField('command','启动命令','placeholder="例如 npx、uvx 或程序完整路径" spellcheck="false"')}
        ${reviewMcpListFields('argsRows','arg','参数','一个参数一行')}
        ${reviewMcpPairFields('envPairs','env','环境变量')}
        ${reviewMcpListFields('envPassRows','pass','传递的环境变量','例如 PATH，只填写变量名称')}
        ${reviewMcpField('cwd','工作目录（可选）','placeholder="例如 D:\\Projects\\demo" spellcheck="false"')}`:`
        ${reviewMcpField('source','服务地址','placeholder="https://mcp.example.com/mcp" spellcheck="false"')}
        <div class="mcp-inline"><h3>认证方式</h3><div class="choice-strip" aria-label="认证方式">${[['none','无需认证'],['bearer','Bearer Token'],['header','自定义请求头']].map(([value,label])=>`<button class="choice-pill ${d.auth===value?'selected':''}" aria-pressed="${d.auth===value}" data-action="mcp-entry-auth" data-value="${value}">${label}</button>`).join('')}</div></div>
        ${d.auth==='none'?'':`<div class="${d.auth==='header'?'mcp-basic-grid':'mcp-token-field'}">${d.auth==='header'?reviewMcpField('headerName','认证头名称','placeholder="例如 X-API-Key"'):''}${reviewMcpField('token',d.auth==='bearer'?'Token':'认证头值','type="password" autocomplete="off" placeholder="填写服务提供方的凭据"')}</div>`}
        ${reviewMcpPairFields('headerPairs','header','请求头（Headers）')}`}
      </section>`;
  }
  const tabs=`<div class="mcp-editor-tabs" role="tablist" aria-label="MCP 配置方式">${[['form','表单配置'],['json','JSON 配置']].map(([value,label])=>`<button id="mcp-tab-${value}" class="mcp-editor-tab ${d.view===value?'selected':''}" role="tab" aria-selected="${d.view===value}" aria-controls="mcp-editor-panel" tabindex="${d.view===value?'0':'-1'}" data-action="mcp-entry-view" data-value="${value}">${svg(value==='json'?'code':'sliders')}${label}</button>`).join('')}</div>`;
  const help=`<details class="mcp-editor-help"><summary aria-label="MCP 配置说明">?</summary><div>服务商给的是地址时选 HTTP 或 SSE；给的是程序命令时选 STDIO。参数每行填写一个，不需要加引号。JSON 可以粘贴单个服务或 mcpServers 配置，切换表单会读取其中的字段。环境变量传递只填写本机变量名称。添加的是服务，工具由服务提供。</div></details>`;
  const targets=d.entry==='choose'?'':`<section class="mcp-target-section"><h3>使用的客户端</h3>${reviewTargetField('cap-import-target',d.targets,{locked:!!d.preferred,single:false})}</section>`;
  const reuse=d.entry==='choose'?'':`<details class="mcp-reuse"><summary>从已配置客户端复用</summary>${Object.entries(capabilityCatalog.mcp).map(([id,item])=>`<button class="mcp-existing" data-action="mcp-entry-reuse" data-value="${e(id)}"><strong>${e(item.name)}</strong><span>${e(Object.keys(clients).filter(k=>capabilityInUse('mcp',id,k)).map(k=>clients[k]).join(' / ')||'未使用')}</span>${svg('chevron')}</button>`).join('')}</details>`;
  openDialog('添加 MCP',`<div class="mcp-entry"><div class="mcp-editor-heading">${tabs}${help}</div><div id="mcp-editor-panel" class="mcp-editor-panel" role="tabpanel" aria-labelledby="mcp-tab-${d.view}">${content}</div><p class="field-error" id="cap-import-error" role="alert">${e(d.issue||'')}</p>${targets}${reuse}</div>`,btn('取消','close-dialog')+(d.entry==='choose'?'':btn('添加并使用','capability-import-use','','primary')));
}
function reviewMcpFormSnapshot(d){return JSON.parse(JSON.stringify(Object.fromEntries(['source','type','imported','importedAuth','transport','token','auth','headerName','cwd','envPairs','headerPairs','command','argsRows','envPassRows'].map(k=>[k,d[k]]))));}
function reviewMcpPairs(value,label){
  if(!value||typeof value!=='object'||Array.isArray(value)||Object.values(value).some(v=>typeof v!=='string'))throw new Error(label+'需要名称和文本值，请核对配置。');
  return Object.entries(value);
}
function reviewMcpValidateService(service){
  if(!service||typeof service!=='object'||Array.isArray(service))throw new Error('这段配置不是 MCP 服务，请填写服务配置对象。');
  const has=key=>Object.prototype.hasOwnProperty.call(service,key);
  if(has('command')&&has('url'))throw new Error('同一服务包含两种连接方式，请保留地址或启动命令中的一种。');
  if(!has('command')&&!has('url'))throw new Error('没有找到服务地址或启动命令。');
  if(has('url')&&typeof service.url!=='string'||has('command')&&typeof service.command!=='string')throw new Error('服务地址和启动命令应为文本。');
  if(service.args!==undefined&&(!Array.isArray(service.args)||service.args.some(x=>typeof x!=='string')))throw new Error('args 应为文本参数组成的数组。');
  reviewMcpPairs(service.env===undefined?{}:service.env,'环境变量');reviewMcpPairs(service.headers===undefined?{}:service.headers,'请求头');
  if(service.env_vars!==undefined&&(!Array.isArray(service.env_vars)||service.env_vars.some(x=>typeof x!=='string'||!x.trim())))throw new Error('env_vars 应为环境变量名称组成的数组。');
  if(service.type!==undefined&&!['http','streamable-http','streamableHttp','sse','stdio'].includes(service.type))throw new Error('这份配置使用尚未适配的协议，原内容已保留。');
  if(has('command')&&service.type&&service.type!=='stdio'||has('url')&&service.type==='stdio')throw new Error('协议与服务地址或启动命令不一致，请核对配置。');
  for(const key of ['cwd','description','name'])if(service[key]!==undefined&&typeof service[key]!=='string')throw new Error(key+' 应为文本，原内容已保留。');
}
function reviewMcpReadJson(raw){
  let parsed;try{parsed=JSON.parse(raw.trim().replace(/^```(?:json)?\s*/i,'').replace(/\s*```$/,''));}catch{throw new Error('JSON 格式有误，请检查引号、逗号和括号。');}
  if(!parsed||typeof parsed!=='object'||Array.isArray(parsed))throw new Error('配置应为 MCP 服务对象或 mcpServers 对象。');
  if(Object.prototype.hasOwnProperty.call(parsed,'mcpServers')){
    if(!parsed.mcpServers||typeof parsed.mcpServers!=='object'||Array.isArray(parsed.mcpServers))throw new Error('mcpServers 应为以服务名称为键的对象。');
    const candidates=Object.entries(parsed.mcpServers).map(([name,service])=>({name,service}));
    if(!candidates.length)throw new Error('mcpServers 中还没有服务。');
    const envelope={...parsed};delete envelope.mcpServers;return {candidates,envelope};
  }
  return {candidates:[{name:typeof parsed.name==='string'?parsed.name:'',service:parsed}],envelope:{}};
}
function reviewMcpLoadCandidate(d,candidate){
  const s=candidate.service;reviewMcpValidateService(s);
  Object.assign(d,{entry:'configure',name:candidate.name||d.name,description:s.description||'',imported:JSON.parse(JSON.stringify(s)),importedAuth:null,type:Object.prototype.hasOwnProperty.call(s,'command')?'1':'0',source:s.url||'',command:s.command||'',argsRows:[...(s.args||[])],envPassRows:[...(s.env_vars||[])],transport:s.type==='sse'?'sse':'http',envPairs:reviewMcpPairs(s.env||{},'环境变量'),headerPairs:reviewMcpPairs(s.headers||{},'请求头'),cwd:s.cwd||'',auth:'none',token:'',headerName:'Authorization',issue:''});
  const authIndex=d.headerPairs.findIndex(([key,value])=>key.toLowerCase()==='authorization'&&/^Bearer /i.test(value));
  if(authIndex>=0){const [key,value]=d.headerPairs[authIndex];d.auth='bearer';d.token=value.slice(7);d.importedAuth={key,value};d.headerPairs.splice(authIndex,1);}
  if(!d.envPairs.length)d.envPairs=[['','']];if(!d.headerPairs.length)d.headerPairs=[['','']];
}
function reviewMcpDetect(){
  reviewMcpCapture();const d=state.capabilityDraft;d.issue='';
  try{
    const parsed=reviewMcpReadJson(d.jsonText);d.candidates=parsed.candidates;d.envelope=parsed.envelope;
    if(d.candidates.length===1){reviewMcpLoadCandidate(d,d.candidates[0]);d.view='form';}else{d.entry='choose';d.view='form';}
  }catch(err){d.issue=err.message;}
  reviewMcpRender();
}
function reviewMcpCandidate(index){
  const d=state.capabilityDraft,candidate=d.candidates[index];if(!candidate)return;
  try{reviewMcpLoadCandidate(d,candidate);d.view='form';}catch(err){d.issue=err.message;}
  reviewMcpRender();
}
function reviewMcpPairObject(pairs,label){
  const values={},seen=new Set();
  for(const [key,value] of pairs){if(!key&&!value)continue;if(!key.trim())throw new Error('请填写'+label+'名称。');const identity=label==='请求头'?key.trim().toLowerCase():key.trim();if(seen.has(identity))throw new Error(label+'名称重复，请合并后再添加。');seen.add(identity);Object.defineProperty(values,key.trim(),{value,enumerable:true,writable:true,configurable:true});}
  return values;
}
function reviewMcpService(d,complete=true){
  const service={...(d.imported||{})};
  if(d.description||d.imported?.description!==undefined)service.description=d.description;else delete service.description;
  if(Object.prototype.hasOwnProperty.call(service,'name'))service.name=d.name;
  if(d.type==='1'){
    service.command=d.command;if(d.argsRows.length||!d.imported||d.imported.args!==undefined)service.args=[...d.argsRows];else delete service.args;
    if(complete&&!d.command.trim())throw new Error('请填写启动命令或程序路径。');
    const env=reviewMcpPairObject(d.envPairs,'环境变量');if(Object.keys(env).length||d.imported?.env!==undefined)service.env=env;else delete service.env;
    const passed=d.envPassRows.map(x=>x.trim());
    if(passed.some(x=>!x))throw new Error('请填写要传递的环境变量名称，或删除空行。');
    if(new Set(passed).size!==passed.length)throw new Error('传递的环境变量名称重复，请合并。');
    if(passed.length||d.imported?.env_vars!==undefined)service.env_vars=passed;else delete service.env_vars;
    if(d.cwd||d.imported?.cwd!==undefined)service.cwd=d.cwd;else delete service.cwd;
  }else{
    if(complete){
      let url;try{url=new URL(d.source);}catch{throw new Error('服务地址不完整，请填写 HTTP(S) MCP 地址。');}
      if(!['http:','https:'].includes(url.protocol)||url.username||url.password)throw new Error('请使用 HTTP(S) 地址，访问凭据填写在认证设置中。');
    }
    service.url=d.source;const headers=reviewMcpPairObject(d.headerPairs,'请求头');
    if(d.auth!=='none'){
      if(complete&&!d.token.trim())throw new Error('请填写此服务的访问凭据。');
      const unchanged=d.auth==='bearer'&&d.importedAuth?.value.slice(7)===d.token;
      const key=unchanged?d.importedAuth.key:d.auth==='header'?d.headerName:'Authorization';
      if(!/^[A-Za-z0-9-]+$/.test(key))throw new Error('请核对认证头名称。');
      if(Object.keys(headers).some(k=>k.toLowerCase()===key.toLowerCase()))throw new Error('认证设置与请求头重复，请保留一种填写方式。');
      headers[key]=unchanged?d.importedAuth.value:d.auth==='bearer'?'Bearer '+d.token:d.token;
    }
    if(Object.keys(headers).length||d.imported?.headers!==undefined)service.headers=headers;else delete service.headers;
    if(d.transport==='sse')service.type='sse';else if(!d.imported||service.type==='sse')service.type='streamable-http';
  }
  reviewMcpValidateService(service);return service;
}
function reviewMcpParsed(d){
  if(d.entry!=='configure')throw new Error('请先选择要添加的服务。');
  if(d.view==='json'){
    const parsed=reviewMcpReadJson(d.jsonText);
    if(parsed.candidates.length!==1)throw new Error('配置包含多个服务，请点击“表单配置”选择要添加的一项。');
    reviewMcpLoadCandidate(d,parsed.candidates[0]);d.envelope=parsed.envelope;
  }
  if(!d.name.trim())throw new Error('请填写服务名称；JSON 可用 mcpServers 下的名称或 name 字段。');
  const service=reviewMcpService(d);
  return {format:d.type==='1'?'本地命令 · stdio':'远程服务 · '+(d.transport==='sse'?'SSE':'Streamable HTTP'),description:d.description||'自定义工具服务',serviceKind:d.type==='1'?'local':'remote',files:[JSON.stringify(service,null,2)]};
}
function reviewMcpAction(action,value){
  if(!action.startsWith('mcp-entry-'))return false;
  const d=state.capabilityDraft;if(d?.kind!=='mcp'||!d.entry)return true;
  switch(action){
    case 'mcp-entry-detect':reviewMcpDetect();return true;
    case 'mcp-entry-candidate':reviewMcpCandidate(Number(value));return true;
    case 'mcp-entry-reuse':if(capabilityCatalog.mcp[value]){if(d.preferred)capabilityUse({kind:'mcp',id:value,key:d.preferred,use:true});else capabilityTargets('mcp',value,true);}return true;
  }
  reviewMcpCapture();d.issue='';
  switch(action){
    case 'mcp-entry-view':
      if(!['form','json'].includes(value)||d.view===value&&d.entry!=='choose')return true;
      if(value==='form'){reviewMcpDetect();return true;}
      if(d.entry!=='choose')try{d.jsonText=JSON.stringify({...d.envelope,mcpServers:{[d.name||'new-mcp']:reviewMcpService(d,false)}},null,2);}catch(err){d.issue=err.message;reviewMcpRender();return true;}
      d.view='json';d.entry='configure';break;
    case 'mcp-entry-back':d.entry='configure';d.view='json';break;
    case 'mcp-entry-manual':if(d.view==='json'){reviewMcpDetect();return true;}break;
    case 'mcp-entry-auth':if(['none','bearer','header'].includes(value)&&d.auth!==value){d.auth=value;d.token='';}break;
    case 'mcp-entry-connection':if(['http','sse','stdio'].includes(value)){reviewMcpMode(d,value==='stdio'?'1':'0');if(value!=='stdio')d.transport=value;}break;
    case 'mcp-entry-mode':if(['0','1'].includes(value))reviewMcpMode(d,value);break;
    case 'mcp-entry-transport':if(['http','sse'].includes(value))d.transport=value;break;
    case 'mcp-entry-pair-add':if(['env','header'].includes(value))d[value==='env'?'envPairs':'headerPairs'].push(['','']);break;
    case 'mcp-entry-pair-remove':{const [prefix,index]=value.split(':');if(['env','header'].includes(prefix))d[prefix==='env'?'envPairs':'headerPairs'].splice(Number(index),1);break;}
    case 'mcp-entry-row-add':if(['arg','pass'].includes(value))d[value==='arg'?'argsRows':'envPassRows'].push('');break;
    case 'mcp-entry-row-remove':{const [prefix,index]=value.split(':');if(['arg','pass'].includes(prefix))d[prefix==='arg'?'argsRows':'envPassRows'].splice(Number(index),1);break;}
    default:return false;
  }
  reviewMcpRender();return true;
}
function reviewMcpMode(d,type){
  if(d.type===type)return;
  d.modes??={};d.modes[d.type]=reviewMcpFormSnapshot(d);
  Object.assign(d,{type,source:'',command:'',argsRows:[],envPassRows:[],cwd:'',auth:'none',token:'',headerName:'Authorization',transport:'http',envPairs:[['','']],headerPairs:[['','']],imported:null,importedAuth:null},d.modes[type]||{});
}
document.addEventListener('keydown',ev=>{
  if(!['mcp-tab-form','mcp-tab-json'].includes(ev.target.id)||!['ArrowLeft','ArrowRight','Home','End'].includes(ev.key))return;
  ev.preventDefault();const value=ev.key==='Home'?'form':ev.key==='End'?'json':state.capabilityDraft?.view==='form'?'json':'form';
  reviewMcpAction('mcp-entry-view',value);document.getElementById('mcp-tab-'+state.capabilityDraft.view)?.focus();
});
