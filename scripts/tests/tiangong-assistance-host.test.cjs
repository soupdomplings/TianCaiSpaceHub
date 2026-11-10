'use strict';
// Synthetic renderer contract. No desktop app, CDP endpoint or model is opened.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const workspace = path.resolve(__dirname, '../..');
const source = fs.readFileSync(path.join(workspace, 'src/local_assistance/tiangong-host.js'), 'utf8');

class Node {
  constructor(tag) { this.tagName=tag;this.children=[];this.attributes=new Map();this.dataset={};this.hidden=false;this.textContent='';this.open=false;this.parent=null;this.style={};this.className='';this.value='';this.classList={contains:()=>false,toggle:()=>{}}; }
  append(...nodes) { for(const node of nodes){node.parent=this;this.children.push(node);} }
  replaceChildren(...nodes) { this.children=[];this.append(...nodes); }
  setAttribute(k,v) { this.attributes.set(k,v); }
  getAttribute(k) { return this.attributes.get(k) ?? null; }
  toggleAttribute(k,v) { if(v)this.attributes.set(k,'');else this.attributes.delete(k); }
  attachShadow() { this.shadow=new Node('shadow');return this.shadow; }
  remove() { if(this.parent)this.parent.children=this.parent.children.filter(n=>n!==this); }
  addEventListener(k,v) { this['on'+k]=v; }
  showModal() { this.open=true; }
  close() { this.open=false; }
  focus() { this.focused=true; }
  querySelectorAll(selector) { return walk(this).filter(n=>selector==='details[open]'&&n.tagName==='details'&&n.open); }
}
function walk(root) { return [root,...root.children.flatMap(walk)]; }
async function fixture(options={}) {
  const body=new Node('body'),html=new Node('html'),app=new Node('div');
  app._vnode={component:{props:{isActive:true,conversationSessionId:'fixture-session'}}};
  const requests=[],fetches=[],messages=[];
  let ctx;
  const window={electronAPI:{getHarnessUrl:async()=>options.base ?? 'http://127.0.0.1:7861'},fetch:async(...args)=>{
    fetches.push(args);return options.fetch ? options.fetch(...args) : new Response('{"run_id":"fixture-run"}',{status:200});
  }};
  window.__fixturebinding=raw=>{
    const message=JSON.parse(raw);messages.push(message);
    queueMicrotask(()=>window.__tiancaiAssistance?.reply({host:'fixture-host',id:message.id,
      ...(options.failRegister&&message.op==='register'?{error:'synthetic host not ready'}:{data:message.op==='register'?
        (options.registerReply??{contextRef:'fixture-context',systemPrompt:'合成自动协助规则：连续失败两次主动 mcp_call；context_ref=fixture-context；完整工具参数'})
        :message.op==='detail'?{result:'完整合成反馈'}:{}})}));
  };
  const original=window.fetch;
  ctx=vm.createContext({window,document:{getElementById:id=>id==='app'?app:undefined,body,documentElement:html,createElement:tag=>new Node(tag)},
    MutationObserver:class{observe(){}disconnect(){}},URL,location:{href:'file:///synthetic/index.html'},setTimeout,clearTimeout,console});
  vm.runInContext(source,ctx);
  const installed=await ctx.installTiangongAssistance({host:'fixture-host',binding:'__fixturebinding',rules:'合成自动协助规则：连续失败两次主动 assist_start'});
  const root=body.children.find(n=>n.id==='tiancai-local-assistance');
  return {window,body,root,installed,original,fetches,messages,ctx,dispose:()=>window.__tiancaiAssistance?.dispose()};
}
// Fields match requestBody in the installed 2026-11-04 renderer; values are synthetic.
const nativeTask=JSON.parse(fs.readFileSync(path.join(__dirname,'fixtures/tiangong-assistance-task.json'),'utf8'));
const create=overrides=>({method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({...nativeTask,extra_data:{orch_extra_system_prompt:'原生限制'},...overrides})});

test('only the verified supported renderer installs; no approval binding is left on window',async()=>{
  const f=await fixture();assert.equal(f.installed,'installed');assert.equal(f.window.__fixturebinding,undefined);assert.equal(typeof f.window.__tiancaiAssistance.reply,'function');f.dispose();assert.equal(f.window.fetch,f.original);
  const wrong=await fixture({base:'http://untrusted.invalid'});assert.equal(wrong.installed,'incompatible');assert.equal(wrong.window.fetch,wrong.original);
});
test('a native task gets context and automatic rules without changing its project or permission',async()=>{
  const f=await fixture();try{
    await f.window.fetch('http://127.0.0.1:7861/v2/chat/runs',create());await new Promise(setImmediate);
    assert.equal(f.fetches.length,1);const body=JSON.parse(f.fetches[0][1].body);
    assert.match(body.extra_data.orch_extra_system_prompt,/原生限制.*\n\n合成自动协助规则/s);assert.match(body.extra_data.orch_extra_system_prompt,/context_ref=fixture-context/);
    assert.equal(body.project.path,'C:/synthetic-project');assert.equal(body.tool_execution_mode,'confirm');assert.deepEqual(f.messages.map(m=>m.op),['register','run']);
  }finally{f.dispose();}
});
test('explicit selection of other MCPs and unrelated requests are left alone',async()=>{
  const f=await fixture();try{
    await f.window.fetch('http://127.0.0.1:7861/v2/chat/runs',create({mcp_servers:['another-service']}));
    await f.window.fetch('http://127.0.0.1:7861/v2/data/projects',{method:'GET'});
    assert.deepEqual(f.messages.map(m=>m.op),['notice']);assert.equal(f.fetches.length,2);assert.equal(JSON.parse(f.fetches[0][1].body).extra_data.orch_extra_system_prompt,'原生限制');
  }finally{f.dispose();}
});
test('a registration failure submits the unchanged native task exactly once',async()=>{
  const f=await fixture({failRegister:true});try{await f.window.fetch('http://127.0.0.1:7861/v2/chat/runs',create());assert.equal(f.fetches.length,1);assert.equal(JSON.parse(f.fetches[0][1].body).extra_data.orch_extra_system_prompt,'原生限制');
    const pill=f.root.shadow.children.find(n=>n.className==='pill');assert.equal(pill.hidden,false);assert.ok(walk(f.root.shadow).some(n=>n.textContent==='synthetic host not ready'));
  }finally{f.dispose();}
});

test('the real native task shape without extra_data gets complete host instructions',async()=>{
  const f=await fixture();try{
    await f.window.fetch('http://127.0.0.1:7861/v2/chat/runs',create({extra_data:undefined}));await new Promise(setImmediate);
    const submitted=JSON.parse(f.fetches[0][1].body);assert.match(submitted.extra_data.orch_extra_system_prompt,/context_ref=fixture-context/);
    const reg=f.messages.find(m=>m.op==='register');assert.equal(reg.parent.project.path,nativeTask.project.path);assert.equal(reg.parent.project.id,nativeTask.project.id);assert.deepEqual(reg.parent.mcp_servers,[]);assert.equal(reg.parent.agent_id,'desktop-orchestration');
    for(const field of Object.keys(nativeTask))assert.deepEqual(submitted[field],nativeTask[field]);
  }finally{f.dispose();}
});

test('a missing host context or incomplete prompt never injects rules or replays the task',async()=>{
  for(const registerReply of [{contextRef:'fixture-context'},{contextRef:'fixture-context',systemPrompt:'no task identity'},{systemPrompt:'orphan'}]){
    const f=await fixture({registerReply});try{
      await f.window.fetch('http://127.0.0.1:7861/v2/chat/runs',create());await new Promise(setImmediate);
      assert.equal(f.fetches.length,1);assert.equal(JSON.parse(f.fetches[0][1].body).extra_data.orch_extra_system_prompt,'原生限制');assert.equal(f.messages.filter(m=>m.op==='run').length,0);
      assert.equal(f.root.shadow.children.find(n=>n.className==='pill').hidden,false);
    }finally{f.dispose();}
  }
});

test('camel-case extraData cannot replace the new context and an explicit tool restriction is respected',async()=>{
  const f=await fixture();try{
    await f.window.fetch('http://127.0.0.1:7861/v2/chat/runs',create({extraData:{orch_extra_system_prompt:'原生覆盖规则',other:7}}));
    const b=JSON.parse(f.fetches[0][1].body);assert.match(b.extraData.orch_extra_system_prompt,/原生覆盖规则.*context_ref=fixture-context/s);assert.equal(b.extraData.other,7);
    const count=f.messages.filter(m=>m.op==='register').length;
    await f.window.fetch('http://127.0.0.1:7861/v2/chat/runs',create({extra_data:{tool_allowlist:['read']}}));
    assert.equal(f.messages.filter(m=>m.op==='register').length,count);assert.ok(f.messages.some(m=>m.op==='notice'&&m.code==='blocked'));
  }finally{f.dispose();}
});

test('unsupported native task shape is visible and preserves the original task body',async()=>{
  const f=await fixture();try{const init=create({request_id:undefined});await f.window.fetch('http://127.0.0.1:7861/v2/chat/runs',init);
    assert.equal(f.fetches[0][1].body,init.body);assert.equal(f.messages.filter(m=>m.op==='register').length,0);
    assert.equal(f.root.shadow.children.find(n=>n.className==='pill').hidden,false);
  }finally{f.dispose();}
});
test('successful HTTP with malformed JSON never replays a submitted task',async()=>{
  const f=await fixture({fetch:async()=>new Response('not-json',{status:200})});try{
    const response=await f.window.fetch('http://127.0.0.1:7861/v2/chat/runs',create());assert.equal(response.status,200);assert.equal(f.fetches.length,1);assert.equal(await response.text(),'not-json');
  }finally{f.dispose();}
});
test('a network failure after submission never replays and abandons the context',async()=>{
  const f=await fixture({fetch:async()=>{throw new Error('synthetic transport failure');}});try{
    await assert.rejects(()=>f.window.fetch('http://127.0.0.1:7861/v2/chat/runs',create()),/synthetic transport/);await new Promise(setImmediate);assert.equal(f.fetches.length,1);assert.ok(f.messages.some(m=>m.op==='abandon'));
  }finally{f.dispose();}
});
test('native stop and settings changes invalidate the parent assistance scope',async()=>{
  const f=await fixture();try{
    for(const op of ['stop','settings','prompts'])await f.window.fetch('http://127.0.0.1:7861/v2/chat/runs/fixture-run/'+op,{method:op==='settings'?'PATCH':'POST',body:'{}'});
    await new Promise(setImmediate);assert.equal(f.messages.filter(m=>m.op==='invalidate').length,3);
  }finally{f.dispose();}
});
test('progress details are stable across identical polling and never render HTML',async()=>{
  const f=await fixture();try{
    const value={host:'fixture-host',tasks:[{id:'fixture-task',session:'fixture-session',phase:'running',title:'<img src=x onerror=bad()>',detail:'合成处理中',result:'合成结果',progress:['检查中'],pending:[]}]};
    f.window.__tiancaiAssistance.update(value);const drawer=f.root.shadow.children.find(n=>n.tagName==='section');const previous=drawer.children[1];
    const detail=walk(drawer).find(n=>n.tagName==='details');detail.open=true;
    f.window.__tiancaiAssistance.update(value);assert.equal(drawer.children[1],previous);assert.equal(detail.open,true);
    assert.equal(walk(drawer).filter(n=>n.tagName==='img').length,0);assert.ok(walk(drawer).some(n=>n.textContent==='<img src=x onerror=bad()>'));
  }finally{f.dispose();}
});
test('approval appears in TianGong, ignores scripted clicks and sends only a human single decision',async()=>{
  const f=await fixture();try{
    f.window.__tiancaiAssistance.update({host:'fixture-host',tasks:[{id:'fixture-task',session:'fixture-session',phase:'waiting_user',title:'合成只读分析',pending:[{nonce:'fixture-nonce',method:'item/commandExecution/requestApproval',params:{command:'synthetic-check',cwd:'C:/synthetic-project'}}]}]});
    const dialog=walk(f.root.shadow).find(n=>n.tagName==='dialog');assert.ok(dialog.open);
    const allow=walk(dialog).find(n=>n.tagName==='button'&&n.textContent==='允许这一次');const deny=walk(dialog).find(n=>n.tagName==='button'&&n.textContent==='拒绝');assert.ok(deny.focused);
    await allow.onclick({isTrusted:false});assert.equal(f.messages.filter(m=>m.op==='decision').length,0);
    await allow.onclick({isTrusted:true});await new Promise(setImmediate);const event=f.messages.find(m=>m.op==='decision');assert.equal(event.task,'fixture-task');assert.equal(event.nonce,'fixture-nonce');assert.equal(event.response.choice,'allow');
  }finally{f.dispose();}
});
test('switching away from a task does not auto-open its human prompt',async()=>{
  const f=await fixture();try{
    f.window.__tiancaiAssistance.update({host:'fixture-host',tasks:[{id:'other-task',session:'other-session',phase:'waiting_user',pending:[{nonce:'other-nonce',method:'item/tool/requestUserInput',params:{questions:[{id:'q',question:'合成问题'}]}}]}]});
    assert.equal(walk(f.root.shadow).filter(n=>n.tagName==='dialog').length,0);
    assert.ok(walk(f.root.shadow).some(n=>n.tagName==='button'&&n.textContent==='处理问题'));
  }finally{f.dispose();}
});
