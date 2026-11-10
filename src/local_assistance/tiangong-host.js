async function installTiangongAssistance(options) {
  'use strict';
  if (!document.getElementById('app')?._vnode?.component || typeof window.electronAPI?.getHarnessUrl !== 'function') return 'incompatible';
  const base = await window.electronAPI.getHarnessUrl();
  if (base !== 'http://127.0.0.1:7861' || typeof window[options.binding] !== 'function') return 'incompatible';
  window.__tiancaiAssistance?.dispose();
  const binding = window[options.binding];
  // The model-facing page never receives a callable approval transport.
  delete window[options.binding];
  const original = window.fetch;
  const requests = new Map();
  const seenPrompts = new Set();
  let serial = 0, disposed = false, latest = [], shown = null, expanded = false, notice = '';
  const send = (op, body = {}) => new Promise((resolve, reject) => {
    if (disposed) return reject(new Error('本机协助连接已停止'));
    const id = ++serial;
    const timer = setTimeout(() => { requests.delete(id); reject(new Error('本机协助暂未就绪')); }, 4500);
    requests.set(id, {resolve, reject, timer});
    binding(JSON.stringify({host: options.host, id, op, ...body}));
  });
  const element = (tag, text, cls) => {
    const node = document.createElement(tag);
    if (text != null) node.textContent = String(text);
    if (cls) node.className = cls;
    return node;
  };
  const host = element('div'); host.id = 'tiancai-local-assistance';
  const shadow = host.attachShadow({mode: 'closed'});
  const style = element('style');
  style.textContent = `
    :host{color-scheme:light dark;font:14px/1.55 -apple-system,BlinkMacSystemFont,"Segoe UI","Microsoft YaHei",sans-serif;--bg:#fff;--ink:#202633;--muted:#5e6879;--line:#e3e8ef;--accent:#4267d5;--soft:#f5f7fb;--danger:#b63846;position:fixed;right:20px;bottom:20px;z-index:2147483000;color:var(--ink)}
    :host([data-dark]){--bg:#20242c;--ink:#e8edf5;--muted:#b0bacb;--line:#3c4452;--accent:#9eafff;--soft:#2b303c;--danger:#ffa3ae}
    *{box-sizing:border-box}button,input,textarea,select{font:inherit}button{cursor:pointer;color:inherit;border:1px solid var(--line);border-radius:9px;background:var(--bg);padding:8px 14px;min-height:38px}button:hover{background:var(--soft)}button:disabled{opacity:.5;cursor:wait}:focus-visible{outline:3px solid var(--accent);outline-offset:3px}
    .pill{display:flex;align-items:center;gap:9px;border-radius:22px;box-shadow:0 4px 20px #0002;padding:9px 15px}.dot{width:8px;height:8px;border-radius:50%;background:var(--accent)}.busy{animation:pulse 1.6s ease-in-out infinite}.badge{background:var(--accent);color:var(--bg);border-radius:10px;min-width:20px;padding:0 6px;font-size:12px}
    .drawer{margin-bottom:12px;width:min(400px,calc(100vw - 40px));max-height:min(560px,70vh);overflow:auto;padding:18px;border:1px solid var(--line);border-radius:16px;background:var(--bg);box-shadow:0 12px 40px #0003}.row{padding:12px 0;border-top:1px solid var(--line)}.row:first-of-type{border:0}.title{font-size:16px;font-weight:600;margin:0 0 5px}.muted{color:var(--muted);font-size:13px}.actions{display:flex;gap:8px;justify-content:flex-end;flex-wrap:wrap;margin-top:16px}.primary{background:var(--accent);color:var(--bg);border-color:var(--accent)}.danger{color:var(--danger)}
    dialog{color:var(--ink);background:var(--bg);border:1px solid var(--line);border-radius:18px;padding:24px;width:min(580px,calc(100vw - 40px));max-height:85vh;box-shadow:0 18px 70px #0004}dialog::backdrop{background:#11182766;backdrop-filter:blur(3px)}h2{font-size:19px;line-height:1.3;margin:0 0 8px}p{margin:8px 0}pre{font:12px/1.6 Consolas,monospace;white-space:pre-wrap;overflow-wrap:anywhere;max-height:220px;overflow:auto;background:var(--soft);padding:12px;border-radius:10px;margin:12px 0}label{display:block;margin-top:15px;font-weight:600}textarea,input,select{display:block;width:100%;margin-top:7px;border:1px solid var(--line);border-radius:8px;background:var(--bg);color:var(--ink);padding:9px 11px}textarea{min-height:76px;resize:vertical}.choices{display:grid;gap:7px;margin-top:8px}.choices button{text-align:left}.choices button[aria-pressed=true]{border-color:var(--accent);background:var(--soft)}.error{color:var(--danger);min-height:20px}details{margin-top:8px}summary{cursor:pointer;color:var(--muted)}
    @keyframes pulse{50%{opacity:.25;box-shadow:0 0 0 4px color-mix(in srgb,var(--accent) 16%,transparent)}}@media(prefers-reduced-motion:reduce){*{animation:none!important;transition:none!important}}`;
  shadow.append(style);
  const drawer = element('section', null, 'drawer'); drawer.hidden = true;
  const pill = element('button', null, 'pill'); pill.type = 'button'; pill.hidden = true; pill.setAttribute('aria-label','查看本机协助进展');
  const dot = element('span', null, 'dot'); const label = element('span', '本机协助'); const badge = element('span', '', 'badge');
  pill.append(dot, label, badge); pill.onclick = () => { expanded = !expanded; render(); };
  shadow.append(drawer, pill); document.body.append(host);
  const dark = () => host.toggleAttribute('data-dark', document.documentElement.classList.contains('dark') || document.body.classList.contains('dark') || document.documentElement.getAttribute('data-theme') === 'dark');
  const observer = new MutationObserver(dark); observer.observe(document.documentElement,{attributes:true,attributeFilter:['class','data-theme']}); observer.observe(document.body,{attributes:true,attributeFilter:['class']});dark();
  const activeSession = () => {
    const queue = [document.getElementById('app')?._vnode]; const visited = new Set();
    for (let n=0;queue.length && n<12000;n++) {
      const v=queue.pop();if(!v || typeof v!=='object' || visited.has(v))continue;visited.add(v);
      if(v.component?.props?.isActive && typeof v.component.props.conversationSessionId==='string')return v.component.props.conversationSessionId;
      if(v.component?.subTree)queue.push(v.component.subTree);if(Array.isArray(v.children))queue.push(...v.children);
    }return null;
  };
  const button = (text, action, cls) => {
    const b=element('button',text,cls); b.type='button';
    b.onclick=async e=>{if(!e.isTrusted)return;b.disabled=true;try{await action();}catch(error){b.disabled=false;label.textContent=error.message;}};
    return b;
  };
  const closePrompt = () => { if(shown){shown.dialog.close();shown.dialog.remove();shown=null;} };
  function prompt(task, p) {
    if(shown?.nonce===p.nonce)return;closePrompt();
    const dialog=element('dialog'); const question=p.method==='item/tool/requestUserInput'; const form=p.method==='mcpServer/elicitation/request';
    dialog.setAttribute('aria-label', question||form?'协助需要补充信息':'协助需要你的确认');
    dialog.append(element('h2',question||form?'需要补充信息':'需要你的确认'));
    dialog.append(element('p',task.title || '当前天工任务','muted'));
    const params=p.params, values={}, readers=[];
    if(question){
      for(const q of params.questions || []){
        dialog.append(element('label',q.question));
        if(q.options?.length){
          const choices=element('div',null,'choices');
          for(const option of q.options){const b=element('button',option.label);b.type='button';b.setAttribute('aria-pressed','false');
            b.onclick=e=>{if(!e.isTrusted)return;values[q.id]=option.label;for(const sibling of choices.children)sibling.setAttribute('aria-pressed',String(sibling===b));};
            if(option.description)b.title=option.description;choices.append(b);
          }dialog.append(choices);
        }
        if(!q.options?.length || q.isOther){const input=element('textarea');input.setAttribute('aria-label',q.question);if(q.isSecret){const secret=element('input');secret.type='password';secret.autocomplete='off';secret.setAttribute('aria-label',q.question);dialog.append(secret);readers.push(()=>{if(secret.value)values[q.id]=secret.value;});}else{dialog.append(input);readers.push(()=>{if(input.value.trim())values[q.id]=input.value;});}}
      }
    }else if(form){
      dialog.append(element('p',params.message));
      for(const [key,schema] of Object.entries(params.requestedSchema?.properties || {})){
        dialog.append(element('label',schema.title || key));let input;
        if(schema.enum){input=element('select');const placeholder=element('option','请选择');placeholder.value='';input.append(placeholder);for(const [index,value] of schema.enum.entries()){const o=element('option',value);o.value=String(index);input.append(o);}}
        else{input=element('input');input.type=schema.type==='boolean'?'checkbox':schema.type==='number'||schema.type==='integer'?'number':'text';}
        input.setAttribute('aria-label',schema.title||key);dialog.append(input);
        readers.push(()=>{if(schema.enum){if(input.value!=='')values[key]=schema.enum[Number(input.value)];}
          else if(schema.type==='boolean')values[key]=input.checked;
          else if(input.value!=='')values[key]=schema.type==='number'||schema.type==='integer'?Number(input.value):input.value;});
      }
    }else{
      dialog.append(element('p',params.reason || '协助执行到这一步需要你确认。仅对本次操作生效。'));
      const command=params.operationPreview || params.command || params.networkApprovalContext || params.permissions || params.changes || params.grantRoot;
      if(command)dialog.append(element('pre',typeof command==='string'?command:JSON.stringify(command,null,2)));
      if(params.cwd)dialog.append(element('p','工作目录：'+params.cwd,'muted'));
      const detail=element('details');detail.append(element('summary','操作详情'));detail.append(element('pre',JSON.stringify(params,null,2)));dialog.append(detail);
    }
    const error=element('p','','error');error.setAttribute('role','alert');dialog.append(error);
    const actions=element('div',null,'actions');
    const submit=async allow=>{
      readers.forEach(read=>read());
      try{await send('decision',{task:task.id,nonce:p.nonce,response:{choice:allow?'allow':'deny',answers:values}});closePrompt();}
      catch(e){error.textContent=e.message;for(const b of actions.children)b.disabled=false;}
    };
    actions.append(button(question||form?'暂不提供':'拒绝',()=>submit(false),'danger'),button(question||form?'提交回答':'允许这一次',()=>submit(true),'primary'));
    dialog.append(actions);shadow.append(dialog);dialog.addEventListener('cancel',e=>{e.preventDefault();if(e.isTrusted)void submit(false);});
    shown={dialog,nonce:p.nonce};dialog.showModal();actions.children[0].focus();
  }
  function render(){
    const items=latest.filter(t=>t.phase!=='cleared');const pending=items.reduce((n,t)=>n+(t.pending?.length||0),0);
    pill.hidden=!items.length&&!notice;drawer.hidden=!expanded||(!items.length&&!notice);badge.hidden=!pending;badge.textContent=String(pending);
    dot.classList.toggle('busy',items.some(t=>['starting','running'].includes(t.phase)));
    label.textContent=pending?'协助需要你处理':items.some(t=>['starting','running'].includes(t.phase))?'正在协助处理':notice?'本轮协助未接入':'本机协助';
    const opened=new Set([...drawer.querySelectorAll('details[open]')].map(n=>n.dataset.task));
    drawer.replaceChildren(element('p','本机协助','title'));
    if(notice){drawer.append(element('p',notice,'error'));drawer.append(element('p','原任务继续。可在 Hub → 天工 Work → 接入设置 → 本机协助查看接入情况，处理后新建任务。','muted'));}
    for(const task of items){
      const row=element('article',null,'row');row.append(element('p',task.title||'当前任务'));row.append(element('p',task.detail,'muted'));
      if(task.progress?.length)row.append(element('p',task.progress.at(-1),'muted'));
      for(const p of task.pending||[])if(!p.params.decisionSubmitted)row.append(button('处理'+(p.method.includes('requestUserInput')?'问题':'请求'),()=>prompt(task,p),'primary'));
      const detail=element('details');detail.dataset.task=task.id;detail.open=opened.has(task.id);detail.append(element('summary','查看处理记录'));if(task.result)detail.append(element('pre',task.result));
      if(task.resultTruncated)detail.append(button('查看完整反馈',async()=>{const full=await send('detail',{task:task.id});detail.append(element('pre',full.result));}));
      if(task.files?.length)detail.append(element('pre','涉及文件\n'+task.files.join('\n')));
      if(task.commands?.length)detail.append(element('pre','检查\n'+task.commands.map(c=>c.command+'\n'+(c.exitCode==null?'未取得退出码':'退出码：'+c.exitCode)+(c.output?'\n'+c.output:'')).join('\n\n')));
      row.append(detail);const actions=element('div',null,'actions');
      if(['starting','running','waiting_user','cancelling'].includes(task.phase))actions.append(button('停止协助',()=>send('stop',{task:task.id}),'danger'));
      else actions.append(button('清理协助记录',()=>send('delete',{task:task.id})));
      row.append(actions);drawer.append(row);
    }
  }
  const contexts=new Map();
  const warn = (text, code) => {notice=text;render();if(code)void send('notice',{code}).catch(()=>{});};
  const wrapped=async function(input,init){
    let url;try{url=new URL(typeof input==='string'?input:input.url,location.href);}catch{return original.apply(this,arguments);}
    const isRun=url.origin===base && url.pathname.startsWith('/v2/chat/runs');
    if(!isRun || init?.method!=='POST' && init?.method!=='PATCH')return original.apply(this,arguments);
    const path=url.pathname.slice('/v2/chat/runs'.length);
    if(path==='' && init?.method==='POST' && typeof init.body==='string'){
      let body;try{body=JSON.parse(init.body);}catch{return original.apply(this,arguments);}
      if(!body || !body.session_id || !body.request_id || !body.project?.path || !body.query){warn('此任务缺少可核对的项目或任务身份，未接入本机协助。','unsupported');return original.apply(this,arguments);}
      if(Array.isArray(body.mcp_servers)&&body.mcp_servers.length&&!body.mcp_servers.some(s=>typeof s==='string'&&s.trim()==='tiancai-local-assistance')){warn('本轮只选择了其他连接，本机协助未加入。','excluded');return original.apply(this,arguments);}
      const currentExtra={...(body.extra_data||{}),...(body.extraData||{})};
      if(Array.isArray(currentExtra.tool_allowlist)&&!currentExtra.tool_allowlist.includes('mcp_call')){warn('本轮工具范围未包含 MCP 调用，未加入自动协助。','blocked');return original.apply(this,arguments);}
      let scope, prepared=init;
      try{
        scope=await send('register',{parent:{session_id:body.session_id,request_id:body.request_id,project:{path:body.project.path,id:String(body.project.id||body.project_id||'default')},query:body.query,tool_execution_mode:body.tool_execution_mode,
          agent_id:body.agent_id,user_id:body.user_id,mcp_servers:body.mcp_servers}});
        if(!scope?.contextRef || typeof scope.systemPrompt!=='string' || !scope.systemPrompt.includes(scope.contextRef))throw new Error('本机协助工具参数未就绪，未加入不完整规则。');
        const extra={...currentExtra};
        extra.orch_extra_system_prompt=(extra.orch_extra_system_prompt||'')+'\n\n'+scope.systemPrompt;
        body.extra_data=extra;
        // Native extraData wins over extra_data; update only its prompt as well.
        if(body.extraData)body.extraData={...body.extraData,orch_extra_system_prompt:extra.orch_extra_system_prompt};
        prepared={...init,body:JSON.stringify(body)};
        notice='';render();
      }catch(error){if(scope?.contextRef)void send('abandon',{context:scope.contextRef}).catch(()=>{});scope=null;warn(error.message);}
      // Submit exactly once. A response/JSON error must never replay the task.
      let response;
      try{response=await original.call(this,input,prepared);}
      catch(error){if(scope)void send('abandon',{context:scope.contextRef}).catch(()=>{});warn('天工任务提交未完成，协助上下文已撤销。','submit_failed');throw error;}
      if(scope){
        if(response.ok){try{const created=await response.clone().json();if(created.run_id){contexts.set(created.run_id,scope.contextRef);void send('run',{context:scope.contextRef,run:created.run_id}).catch(e=>warn(e.message));}else{void send('abandon',{context:scope.contextRef}).catch(()=>{});warn('天工未返回可核对的运行身份，未继续接入协助。','submit_failed');}}catch{void send('abandon',{context:scope.contextRef}).catch(()=>{});warn('天工任务响应无法解析，未继续接入协助。','submit_failed');}}
        else {void send('abandon',{context:scope.contextRef}).catch(()=>{});warn('天工任务提交失败（HTTP '+response.status+'），未继续接入协助。','submit_failed');}
      }
      return response;
    }
    const response=await original.apply(this,arguments);
    if(response.ok){
      const match=/^\/([A-Za-z0-9_-]+)\/(stop|settings|prompts)$/.exec(path);
      if(match){void send('invalidate',{run:match[1]}).catch(()=>{});contexts.delete(match[1]);}
    }return response;
  };
  window.fetch=wrapped;
  window.__tiancaiAssistance={
    reply(message){if(message.host!==options.host)return;const pending=requests.get(message.id);if(!pending)return;clearTimeout(pending.timer);requests.delete(message.id);message.error?pending.reject(new Error(message.error)):pending.resolve(message.data);},
    update(message){if(message.host!==options.host||disposed)return;const next=Array.isArray(message.tasks)?message.tasks:[];
      if(JSON.stringify(next)===JSON.stringify(latest))return;latest=next;
      if(shown&&!latest.some(t=>t.pending?.some(p=>p.nonce===shown.nonce)))closePrompt();render();
      const current=activeSession();for(const task of latest){const p=task.pending?.find(p=>!p.params.decisionSubmitted);if(p&&!seenPrompts.has(p.nonce)&&task.session===current&&!shown){seenPrompts.add(p.nonce);prompt(task,p);break;}}},
    dispose(){disposed=true;if(window.fetch===wrapped)window.fetch=original;closePrompt();observer.disconnect();host.remove();for(const p of requests.values()){clearTimeout(p.timer);p.reject(new Error('本机协助连接已停止'));}requests.clear();}
  };
  return 'installed';
}
