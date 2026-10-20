'use strict';
// Isolated fixtures only: never starts Hub/Codex or reads the user's config.
const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),vm=require('node:vm'),zlib=require('node:zlib');
const {Plan,run,hash,archiveRead,archiveWrite,axValid}=require('../assets/codex-repair/hub-repair.cjs');
const adapter=vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../assets/codex-repair/renderer-adapter.js'),'utf8'));
function fixture(fn){const dir=fs.mkdtempSync(path.join(os.tmpdir(),'hub-repair-test-'));const home=path.join(dir,'home'),localRoot=path.join(dir,'local'),backupRoot=path.join(home,'hub-desktop-repair-backups');fs.mkdirSync(home);fs.mkdirSync(localRoot);try{return fn({dir,home,localRoot,backupRoot,kind:'models'});}finally{fs.rmSync(dir,{recursive:true,force:true});}}
test('planning is read-only and restore survives a new planner',()=>fixture(o=>{
  const f=path.join(o.home,'config.toml');fs.writeFileSync(f,'original');const p=new Plan(o);p.file(f,Buffer.from('managed'));assert.equal(fs.readFileSync(f,'utf8'),'original');assert.equal(fs.existsSync(o.backupRoot),false);
  const backup=p.commit();assert.equal(fs.readFileSync(f,'utf8'),'managed');assert.equal(run({...o,action:'restore',backup,verifyOnly:true}).canRestore,true);assert.equal(fs.readFileSync(f,'utf8'),'managed');
  run({...o,action:'restore',backup});assert.equal(fs.readFileSync(f,'utf8'),'original');
}));
test('restore refuses later edits before restoring any other file',()=>fixture(o=>{
  const a=path.join(o.home,'a'),b=path.join(o.home,'b');fs.writeFileSync(a,'A');fs.writeFileSync(b,'B');const p=new Plan(o);p.file(a,Buffer.from('AA'));p.file(b,Buffer.from('BB'));const backup=p.commit();fs.writeFileSync(b,'user edit');assert.throws(()=>run({...o,action:'restore',backup}),/修复后文件已被修改/);assert.equal(fs.readFileSync(a,'utf8'),'AA');assert.equal(fs.readFileSync(b,'utf8'),'user edit');
}));
test('commit refuses changes made after planning',()=>fixture(o=>{const f=path.join(o.home,'a');fs.writeFileSync(f,'before');const p=new Plan(o);p.file(f,Buffer.from('candidate'));fs.writeFileSync(f,'new');assert.throws(()=>p.commit(),/文件已变化/);assert.equal(fs.readFileSync(f,'utf8'),'new');}));
test('path guard rejects an unrelated target and protected package directory',()=>fixture(o=>{const p=new Plan(o);assert.throws(()=>p.file(path.join(o.dir,'outside'),Buffer.from('x')),/超出/);const protectedRoot=path.join(o.home,'WindowsApps','OpenAI.Codex');fs.mkdirSync(protectedRoot,{recursive:true});assert.throws(()=>p.file(path.join(protectedRoot,'app.asar'),Buffer.from('x')),/商店安装目录/);}));
test('restore verifies backup bytes and does not trust an edited scope',()=>fixture(o=>{const f=path.join(o.home,'a');fs.writeFileSync(f,'before');const p=new Plan(o);p.file(f,Buffer.from('after'));const backup=p.commit();const m=JSON.parse(fs.readFileSync(path.join(backup,'manifest.json')));m.roots.push(o.dir);fs.writeFileSync(path.join(backup,'manifest.json'),JSON.stringify(m));assert.throws(()=>run({...o,action:'restore',backup}),/范围已变化/);assert.equal(fs.readFileSync(f,'utf8'),'after');}));
test('restore removes newly created files and never invents an original',()=>fixture(o=>{const f=path.join(o.home,'a');const p=new Plan(o);p.file(f,Buffer.from('created'));const backup=p.commit();run({...o,action:'restore',backup});assert.equal(fs.existsSync(f),false);}));
test('AX validation requires a complete Brotli encoded WebAssembly module',()=>{const wasm=Buffer.from('0061736d01000000','hex');assert.equal(axValid(zlib.brotliCompressSync(wasm)),true);assert.equal(axValid(zlib.brotliCompressSync(Buffer.from('not wasm'))),false);assert.equal(axValid(Buffer.from('corrupt')),false);});
test('ASAR round trip preserves executable, unpacked, links and unchanged bytes',()=>fixture(o=>{const a={header:{files:{'a.js':{offset:'0',size:2,executable:true,integrity:{algorithm:'SHA256',hash:'',blockSize:2,blocks:[]}},'native.node':{size:4,unpacked:true},alias:{link:'a.js'}}},entries:[]};a.entries.push({name:'a.js',entry:a.header.files['a.js'],bytes:Buffer.from('ok')});const raw=archiveWrite(a),file=path.join(o.home,'app.asar');fs.writeFileSync(file,raw);const b=archiveRead(file);assert.equal(b.entries.length,1);assert.equal(b.entries[0].bytes.toString(),'ok');assert.equal(b.header.files['a.js'].executable,true);assert.equal(b.header.files['native.node'].unpacked,true);assert.equal(b.header.files.alias.link,'a.js');assert.equal(b.header.files['a.js'].integrity.hash,hash(Buffer.from('ok')));assert.deepEqual(archiveWrite(b),raw);}));
test('Power adapter preserves normal behavior outside Hub and rejects ambiguous source',()=>{
  const source='function choose({harborEnabled:h,isElectron:e,isEverydayWorkMode:w}){return w||e&&h}';const a=adapter(source);assert.equal(a.power,true);
  const evaluate=(enabled,args)=>vm.runInNewContext(a.source+';choose('+JSON.stringify(args)+')',{globalThis:{__CODEXHUB_ENHANCED_MODE__:{modelControlsEnabled:enabled}}});
  assert.equal(evaluate(false,{harborEnabled:false,isElectron:true,isEverydayWorkMode:false}),false);assert.equal(evaluate(true,{harborEnabled:false,isElectron:true,isEverydayWorkMode:false}),true);assert.equal(evaluate(true,{harborEnabled:false,isElectron:false,isEverydayWorkMode:false}),false);
  assert.equal(adapter(source+source).power,false);assert.equal(adapter('function unknown(){return false}').source,'function unknown(){return false}');
});
test('Goal preserves cloud and side conversation restrictions',()=>{
  const source='let visible=gate(`3074100722`)&&read(cfg?.config,`goals`)===!0&&mode!==`cloud`&&!side,other=0;';const a=adapter(source);assert.equal(a.goal,true);
  const check=(mode,side,enabled)=>vm.runInNewContext(a.source+';visible',{globalThis:{__CODEXHUB_ENHANCED_MODE__:{modelControlsEnabled:enabled}},gate:()=>false,read:()=>false,cfg:{config:{}},mode,side});
  assert.equal(check('local',false,true),true);assert.equal(check('cloud',false,true),false);assert.equal(check('local',true,true),false);assert.equal(check('local',false,false),false);
});
test('plugin compatibility retains actual availability and loading conditions',()=>{
  const source='let {authMethod:a}=auth(),g=gate(`533078438`),b=blocked(a),d=e&&g&&b,v=availability({hostId:h}),u=e&&v&&!b,next=0;';const a=adapter(source);assert.equal(a.pluginUi,true);
  const check=(managed,available)=>vm.runInNewContext(a.source+';u',{globalThis:{__CODEXHUB_ENHANCED_MODE__:{modelControlsEnabled:managed}},auth:()=>({authMethod:'apiKey'}),gate:()=>true,blocked:()=>true,availability:()=>available,e:true,h:'local'});
  assert.equal(check(true,false),false);assert.equal(check(false,true),false);assert.equal(check(true,true),true);
});
test('Ultra fallback leaves the successful account path intact and persists local fallback',async()=>{
  const source='const k={showUltraInModelPickerSlider:"pref"};let readWitness=readLocal(k.showUltraInModelPickerSlider).catch(()=>!1);async function migration(s){await s.get(client).setUltraEffortEnabled(!0).catch(()=>{});await writeLocal(s,k.showUltraInModelPickerSlider,!1).catch(()=>{});return{queryKey:[`chatgpt-ultra-effort-migration`]}};async function mutate(s,v){let snap=s.query.snapshot(userQuery),prev=snap.getData();snap.setData(c=>c==null?c:{...c,ultraEffortEnabled:v});try{await s.get(client).setUltraEffortEnabled(v),await Promise.all([snap.invalidate(),s.query.snapshot(otherQuery).invalidate()])}catch(err){throw snap.setData(prev),err}};const query={queryFn:async()=>{let u=parse(await scope.get(client).userSettings());return{theme:"kept",lockdownModeEnabled:u.settings?.lockdown_mode_enabled===!0,ultraEffortEnabled:u.settings?.model_picker_persists_ultra_effort===!0}}};';
  const a=adapter(source);assert.equal(a.ultraPreference,true);let remoteWorks=true,local=false,writes=0;let cached={ultraEffortLocalFallback:true,ultraEffortEnabled:false};const snapshot={getData:()=>cached,setData:f=>{cached=typeof f==='function'?f(cached):f},invalidate:async()=>{}};const client={};const scope={get:()=>({userSettings:async()=>{if(!remoteWorks)throw Error('no account');return{settings:{lockdown_mode_enabled:true,model_picker_persists_ultra_effort:true}}},setUltraEffortEnabled:async()=>{writes++}}),query:{snapshot:()=>snapshot}};
  const context={globalThis:{__CODEXHUB_ENHANCED_MODE__:{modelControlsEnabled:true}},readLocal:async()=>local,writeLocal:async(s,key,value)=>{local=value},client,scope,userQuery:{},otherQuery:{},parse:x=>x};
  const api=vm.runInNewContext(a.source+';({query,mutate})',context);let result=await api.query.queryFn();assert.equal(result.lockdownModeEnabled,true);assert.equal(result.theme,'kept');remoteWorks=false;result=await api.query.queryFn();assert.equal(result.ultraEffortLocalFallback,true);await api.mutate(scope,true);assert.equal(local,true);assert.equal(writes,0);
});
test('unsupported service hashes stay unchanged',()=>{const h=require('../assets/codex-repair/patch-chrome-custom-provider-headers.cjs');const original=Buffer.from('unknown service');assert.equal(h.inspect(original,{allowUnsupported:true}).state,'unsupported');assert.throws(()=>h.inspect(original),/Unsupported/);});

test('retired plugin versions and newly created version directories restore together',()=>fixture(o=>{
  const old=path.join(o.home,'plugins/cache/openai-bundled/browser/local'),next=path.join(o.home,'plugins/cache/openai-bundled/browser/2.0.0');fs.mkdirSync(old,{recursive:true});fs.writeFileSync(path.join(old,'plugin.json'),'old');
  const p=new Plan(o);p.removeTree(old);p.file(path.join(next,'plugin.json'),Buffer.from('new'));const backup=p.commit();assert.equal(fs.existsSync(old),false);
  run({...o,action:'restore',backup});assert.equal(fs.readFileSync(path.join(old,'plugin.json'),'utf8'),'old');assert.equal(fs.existsSync(next),false);
}));
test('sequential edits keep the original backup and restore is resumable',()=>fixture(o=>{
  const file=path.join(o.home,'service.js');fs.writeFileSync(file,'original');const p=new Plan(o);p.file(file,Buffer.from('copied'));p.file(file,Buffer.from('patched'));const backup=p.commit();
  assert.equal(fs.readFileSync(file,'utf8'),'patched');run({...o,action:'restore',backup});run({...o,action:'restore',backup,verifyOnly:true});assert.equal(fs.readFileSync(file,'utf8'),'original');
}));
test('a corrupt backup fails before restoring other files',()=>fixture(o=>{
  const a=path.join(o.home,'a'),b=path.join(o.home,'b');fs.writeFileSync(a,'a');fs.writeFileSync(b,'b');const p=new Plan(o);p.file(a,Buffer.from('aa'));p.file(b,Buffer.from('bb'));const backup=p.commit();fs.writeFileSync(path.join(backup,'1.before'),'corrupt');assert.throws(()=>run({...o,action:'restore',backup}),/校验失败/);assert.equal(fs.readFileSync(a,'utf8'),'aa');
}));
test('official registration is isolated and only verified plugin output can be imported',()=>fixture(o=>{
  const cli=path.join(o.dir,'never-executed-cli');fs.writeFileSync(cli,'fixture');let isolated;let calls=0;
  const result=require('../assets/codex-repair/plugin-install.cjs').stage({cli,marketplace:o.dir},['browser'],map=>{
    const p=new Plan(o);p.copy(map.get('browser').source,path.join(o.home,'plugin'));return p.commit();
  },(command,args,options)=>{
    calls++;isolated=options.env.CODEX_HOME;assert.notEqual(isolated,o.home);if(args[1]==='add')assert.equal(fs.existsSync(path.join(isolated,'auth.json')),false);
    if(args[1]==='add') {const target=path.join(isolated,'plugins/cache/openai-bundled/browser/1.0.0');fs.mkdirSync(path.join(target,'.codex-plugin'),{recursive:true});fs.writeFileSync(path.join(target,'.codex-plugin/plugin.json'),'{"version":"1.0.0"}');fs.writeFileSync(path.join(isolated,'auth.json'),'never import');return{status:0,stdout:JSON.stringify({pluginId:'browser@openai-bundled',version:'1.0.0',installedPath:target})};}
    return{status:0,stdout:JSON.stringify({installed:[{pluginId:'browser@openai-bundled',installed:true}]})};
  });
  assert.equal(calls,2);assert.equal(fs.existsSync(isolated),false);assert.equal(fs.existsSync(path.join(o.home,'auth.json')),false);assert.equal(fs.existsSync(path.join(o.home,'plugin/.codex-plugin/plugin.json')),true);run({...o,action:'restore',backup:result});
}));
test('unrecognized registration output stops before the repair is committed',()=>fixture(o=>{
  const cli=path.join(o.dir,'never-executed-cli');fs.writeFileSync(cli,'fixture');let consumed=false;
  assert.throws(()=>require('../assets/codex-repair/plugin-install.cjs').stage({cli,marketplace:o.dir},['browser'],()=>{consumed=true},()=>({status:0,stdout:JSON.stringify({pluginId:'browser@openai-bundled',version:'1.0.0',installedPath:o.home})})),/未识别/);
  assert.equal(consumed,false);assert.equal(fs.existsSync(o.backupRoot),false);
}));
test('migrated plugin hooks and locale keep normal behavior outside Hub',()=>{
  const source='let {authMethod:a}=auth(),{data:d}=account(),workspace=derive(d),blocked=isBlocked(a),manage=route.kind===`manage`,flow=openPluginInstall;let i18n=layer?.get(`enable_i18n`,!1),locale_source="FIRST_AVAILABLE";';
  const changed=adapter(source);assert.equal(changed.pluginUi,true);assert.equal(changed.locale,true);
  for(const managed of [false,true]){let derived=0;const r=vm.runInNewContext(changed.source+';({blocked,i18n})',{globalThis:{__CODEXHUB_ENHANCED_MODE__:{modelControlsEnabled:managed}},auth:()=>({authMethod:'apiKey'}),account:()=>({data:{}}),derive:()=>{derived++},isBlocked:()=>true,route:{kind:'manage'},openPluginInstall(){},layer:{get:()=>false}});assert.equal(derived,1);assert.equal(r.blocked,!managed);assert.equal(r.i18n,managed);}
});
test('package plan preserves browser neighbors and links configuration backup',()=>fixture(o=>{
  const text='const enabled={inAppBrowserUse:f.inAppBrowserUse,mcpAppsBrowserUse:f.mcpAppsBrowserUse,inAppBrowserUseAllowed:f.inAppBrowserUseAllowed,browserPane:f.browserPane,externalBrowserUse:f.externalBrowserUse,externalBrowserUseAllowed:f.externalBrowserUseAllowed,findShortcuts:f.findShortcuts,computerUse:f.computerUse};';
  const entry={offset:'0',size:Buffer.byteLength(text)};const a={header:{files:{'.vite':{files:{build:{files:{'main.js':entry}}}}}},entries:[{name:'.vite/build/main.js',entry,bytes:Buffer.from(text)}]};const resources=path.join(o.dir,'resources');fs.mkdirSync(resources);const source=path.join(resources,'app.asar');fs.writeFileSync(source,archiveWrite(a));const original=fs.readFileSync(source);
  const checked=run({...o,resources,kind:'desktop',action:'check',configCandidate:'managed'});assert.equal(checked.needsRepair,true);assert.deepEqual(fs.readFileSync(source),original);assert.equal(fs.existsSync(o.backupRoot),false);
  const result=run({...o,resources,kind:'desktop',action:'repair',configCandidate:'managed'});assert.equal(typeof result.packagePlan.relatedBackup,'string');const next=archiveRead(result.packagePlan.prepared).entries[0].bytes.toString();assert.ok(next.includes('findShortcuts:f.findShortcuts,computerUse:f.computerUse'));assert.ok(next.includes('externalBrowserUseAllowed:!0'));assert.deepEqual(fs.readFileSync(source),original);
}));
