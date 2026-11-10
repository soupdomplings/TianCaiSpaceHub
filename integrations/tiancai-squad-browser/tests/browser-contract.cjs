// Execute the production Vue script with synthetic API/storage/router objects.
// No browser, account, website, payment or user profile is used.
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm'), assert = require('node:assert/strict');
const modules = process.argv[2];
if (!modules) throw new Error('Pass the isolated tool node_modules directory');
const ts = require(path.resolve(modules, 'typescript'));
const vue = require(path.resolve(modules, '@vue/compiler-sfc'));
const file = path.resolve(__dirname, '../overlay/frontend/src/views/auth/HubConnectView.vue');
const { descriptor, errors } = vue.parse(fs.readFileSync(file, 'utf8'), { filename: file });
assert.equal(errors.length, 0);
assert.equal(vue.compileTemplate({ source: descriptor.template.content, filename: file, id: 'hub-contract' }).errors.length, 0);
const input = descriptor.scriptSetup.content.replace(/^import .+$/gm, '') + '\nglobalThis.contract = { begin, approve, retry, code, message, busy, finished, failed };';
const compiled = ts.transpileModule(input, { compilerOptions: { target:ts.ScriptTarget.ES2022, module:ts.ModuleKind.None }, reportDiagnostics:true });
assert.equal(compiled.diagnostics.length,0);
const id = 'a'.repeat(64), proof = 'b'.repeat(64);
function fixture(flow, signed=true, fail=false, invalid=false) {
  const events=[], data=new Map([['auth_token','browser-old'],['refresh_token','browser-refresh']]);
  const hash=`#flow=${flow}&request=${invalid ? 'bad' : id}${flow==='return' ? '&verifier='+proof : ''}`;
  const route={hash,fullPath:'/hub/connect'+hash};
  const context={URLSearchParams,Date,Error,history:{state:{},replaceState:(_,__,url)=>events.push(['history',url])},
    location:{replace:url=>events.push(['location',url])}, localStorage:{setItem:(k,v)=>data.set(k,v)},
    useRoute:()=>route, useRouter:()=>({replace:async value=>events.push(['router',value])}),
    useAuthStore:()=>({user:{id:7,username:'Synthetic'},isAuthenticated:signed}),
    ref:value=>({value}),computed:f=>({get value(){return f()}}),onMounted:()=>{},
    apiClient:{
      get:async(url,options)=>{events.push(['get',url,options]);return {data:{code:'123456'}}},
      post:async(url,body)=>{events.push(['post',url,body]); if(fail) throw new Error('fixture expired');
        return {data:url.endsWith('complete') ? {state:'ready',access_token:'independent-access',refresh_token:'independent-refresh',expires_in:3600,user:{id:7}} : {approved:true}}}
    }};
  vm.createContext(context); vm.runInContext(compiled.outputText,context);
  return {ui:context.contract,events,data};
}
(async()=>{
  const signedOut=fixture('login',false); await signedOut.ui.begin();
  assert.equal(signedOut.events[0][0],'router');
  assert.equal(signedOut.events[0][1].query.redirect,'/hub/connect#flow=login&request='+id);
  assert(!signedOut.events.some(e=>e[0]==='post'));
  const signed=fixture('login'); await signed.ui.begin(); assert.equal(signed.ui.code.value,'123456');
  assert(!signed.events.some(e=>e[0]==='post'),'authorization requires the visible confirm button');
  await signed.ui.approve(); assert.equal(signed.ui.finished.value,true);
  assert.equal(signed.data.get('auth_token'),'browser-old','browser session is not rotated or exported');
  const back=fixture('return'); await back.ui.begin();
  assert.equal(back.events[0][0],'history','remove one-use proof before network');
  assert.equal(back.events.at(-1)[1],'/dashboard');
  assert.equal(back.data.get('refresh_token'),'independent-refresh');
  assert(!back.events.filter(e=>['history','location'].includes(e[0])).some(e=>String(e[1]).includes('independent-')));
  const expired=fixture('return',true,true); await expired.ui.begin();
  assert.equal(expired.ui.failed.value,true); assert.equal(expired.data.get('auth_token'),'browser-old');
  const invalid=fixture('return',true,false,true); await invalid.ui.begin();
  assert.equal(invalid.ui.failed.value,true); assert(!invalid.events.some(e=>e[0]==='post'));
  console.log(JSON.stringify({passed:6,kind:'production Vue script with synthetic API, storage and router; SFC template/TypeScript syntax',realWebsite:false}));
})().catch(e=>{console.error(e);process.exitCode=1});
