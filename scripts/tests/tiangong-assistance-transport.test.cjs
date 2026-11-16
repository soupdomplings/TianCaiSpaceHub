const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const inspector = require('node:inspector');
const { test } = require('node:test');

const adapt = vm.runInNewContext(fs.readFileSync('src/local_assistance/run-fetch-adapter.js', 'utf8'));
const source = `(() => {
  const captured = [];
  async function connection() { return {base:'http://fixture.invalid',token:'synthetic-only'}; }
async function runFetch(path, options = {}) {
  const api = window.electronAPI;
  if (!api.chatRunOpen || !api.chatRunRead || !api.chatRunCancel) {
    const { base, token } = await connection();
    return fetch(base+path,{...options,headers:{Authorization:token}});
  }
  const signal = options.signal;
  const opened = await api.chatRunOpen({path,method:options.method||'GET',body:options.body,signal});
  const body = (await api.chatRunRead(opened.id)).text;
  await api.chatRunCancel(opened.id);
  captured.push(path);
  return new Response(body,{status:200});
}
async function request$3(method, path, body, signal) {
  return runFetch(path,{method,body,signal});
}
  return {send:request$3,captured};
})()`;

function fixture() {
  const sent = [], reads = [], cancels = [], fetched = [];
  const window = {electronAPI:Object.freeze({
    async chatRunOpen(input){sent.push(input);return {id:'run'};},
    async chatRunRead(id){reads.push(id);return {text:'{"run_id":"fixture-run"}'};},
    async chatRunCancel(id){cancels.push(id);},
  })};
  const context = vm.createContext({window,Response,fetch:async (...args)=>{fetched.push(args);return new Response('legacy');}});
  return {window,context,sent,reads,cancels,fetched};
}

test('the exact runFetch binding is preserved and missing assistance is transparent', async()=>{
  const result = adapt(source);
  assert.equal(result.adapted,true);
  assert.equal((result.source.match(/async function runFetch\(/g)||[]).length,1);
  assert.equal(result.source.includes('__tchNativeRunFetch'),false);
  const f = fixture();
  const handle = vm.runInContext(result.source,f.context);
  assert.deepEqual(JSON.parse(await (await handle.send('POST','/v2/chat/runs','original')).text()),{run_id:'fixture-run'});
  assert.equal(f.sent.length,1);assert.equal(f.sent[0].body,'original');
  assert.equal(f.fetched.length,0);assert.equal(f.reads.length,1);assert.equal(f.cancels.length,1);
  assert.ok(Object.isFrozen(f.window.electronAPI));
  assert.equal(f.window.__tiancaiAssistanceTransportV1,undefined);
});

test('native submissions delegate once and keep the original response and abort signal',async()=>{
  const f = fixture(), result=adapt(source);
  const handle=vm.runInContext(result.source,f.context);
  const controller=new AbortController();let calls=0,response;
  f.window.__tiancaiAssistance={async runRequest(path,options,proceed){
    calls++;assert.equal(path,'/v2/chat/runs');
    response=await proceed({...options,body:'with-context'});return response;
  }};
  const actual=await handle.send('POST','/v2/chat/runs','original',controller.signal);
  assert.equal(actual,response);assert.equal(calls,1);assert.equal(f.sent.length,1);
  assert.equal(f.sent[0].body,'with-context');assert.equal(f.sent[0].signal,controller.signal);
  delete f.window.__tiancaiAssistance;
  await handle.send('GET','/v2/chat/runs/run',undefined);
  assert.equal(calls,1);assert.equal(f.sent.length,2);assert.equal(f.fetched.length,0);
});

test('legacy fetch is not wrapped twice; unsupported or repeated sources are left alone',async()=>{
  const f=fixture();f.window.electronAPI=Object.freeze({});
  f.window.__tiancaiAssistance={runRequest(){throw Error('IPC-only hook must not be called');}};
  const result=adapt(source),handle=vm.runInContext(result.source,f.context);
  assert.equal(await (await handle.send('POST','/v2/chat/runs','legacy')).text(),'legacy');
  assert.equal(f.fetched.length,1);assert.equal(f.sent.length,0);
  const again=adapt(result.source);assert.equal(again.present,true);assert.equal(again.source,undefined);
  assert.equal(adapt(source.replace('api.chatRunRead(', 'api.otherRead(')).adapted,false);
  assert.equal(adapt(source+source).adapted,false);
});

test('LiveEdit updates an already captured closure without reloading or changing module bindings',async()=>{
  const session=new inspector.Session();session.connect();
  const post=(method,params={})=>new Promise((resolve,reject)=>session.post(method,params,(error,result)=>error?reject(error):resolve(result)));
  let scriptId;
  session.on('Debugger.scriptParsed',({params})=>{if(params.url==='tiangong-assistance-live-fixture.js')scriptId=params.scriptId;});
  try{
    await post('Debugger.enable');
    const f=fixture();
    const handle=new vm.Script(source,{filename:'tiangong-assistance-live-fixture.js'}).runInContext(f.context);
    await handle.send('POST','/v2/chat/runs','before');
    await new Promise(setImmediate);
    assert.ok(scriptId);
    const original=await post('Debugger.getScriptSource',{scriptId});
    const result=adapt(original.scriptSource);
    const edited=await post('Debugger.setScriptSource',{scriptId,scriptSource:result.source});
    assert.equal(edited.status,'Ok');
    const applied=await post('Debugger.getScriptSource',{scriptId});assert.equal(applied.scriptSource,result.source);
    let calls=0;
    f.window.__tiancaiAssistance={runRequest(path,options,proceed){calls++;return proceed({...options,body:'after-context'});}};
    await handle.send('POST','/v2/chat/runs','after');
    assert.equal(calls,1);assert.equal(f.sent[1].body,'after-context');
    assert.deepEqual(Array.from(handle.captured),['/v2/chat/runs','/v2/chat/runs']);
    assert.ok(Object.isFrozen(f.window.electronAPI));
  }finally{await post('Debugger.disable').catch(()=>{});session.disconnect();}
});
