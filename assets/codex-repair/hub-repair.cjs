/* Hub repair planner. Bundled with Hub; never loads a skill from the user's home.
 * Check only builds a plan in memory/private temporary storage. Every write is
 * journalled with before/after hashes; restore refuses subsequent user changes.
 */
'use strict';
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const zlib = require('node:zlib');
const {spawnSync} = require('node:child_process');
const headerPatch = require('./patch-chrome-custom-provider-headers.cjs');
const hash = b => crypto.createHash('sha256').update(b).digest('hex');
const read = p => {try {return fs.readFileSync(p);} catch(e) {if(e.code==='ENOENT') return null; throw e;}};
const json = p => JSON.parse(fs.readFileSync(p, 'utf8').replace(/^\uFEFF/, ''));
const bytes = x => Buffer.from(JSON.stringify(x, null, 2)+'\n');
const exists = p => fs.existsSync(p);
function atomic(p, content, mode=0o600) {
  const t=p+'.hub-'+crypto.randomUUID()+'.tmp';
  try {const fd=fs.openSync(t,'wx',mode);try{fs.writeFileSync(fd,content);fs.fsyncSync(fd);}finally{fs.closeSync(fd);}fs.renameSync(t,p);}
  finally {if(exists(t))fs.unlinkSync(t);}
}
const inside = (p, root) => {const r=path.relative(root,p); return r===''||(!r.startsWith('..'+path.sep)&&r!=='..'&&!path.isAbsolute(r));};
function ensure(ok, why) {if(!ok) throw Error(why);}
function actual(p) {let tail=[],q=path.resolve(p); while(!exists(q)) {tail.unshift(path.basename(q));const up=path.dirname(q);ensure(up!==q,'找不到路径的上级目录');q=up;} return path.join(fs.realpathSync(q),...tail);}
function state(p) {
  let s; try{s=fs.lstatSync(p);}catch(e){if(e.code==='ENOENT')return {type:'missing'};throw e;}
  if(s.isSymbolicLink()) return {type:'link',target:fs.readlinkSync(p)};
  ensure(s.isFile(),`目标不是普通文件：${p}`);
  return {type:'file',sha256:hash(fs.readFileSync(p)),mode:s.mode&0o777};
}
const same=(a,b)=>a.type===b.type&&(a.type==='file'?a.sha256===b.sha256:a.type==='link'?a.target===b.target:true);
class Plan {
  constructor(o) {this.options=o;this.ops=new Map();this.lines=[];this.skipped=0;this.removeDirs=new Set();
    this.roots=[o.home,o.localRoot,path.join(o.home,'.tmp'),...(o.nativeRoots||[])].filter(Boolean).map(actual);
  }
  safe(p) {ensure(!/[\\/]WindowsApps[\\/]/i.test(actual(p)),'不原地修改商店安装目录');ensure(this.roots.some(r=>inside(actual(p),r)),`目标超出本次修复范围：${p}`);}
  file(p, content, mode) {this.safe(path.dirname(p)); const before=state(p);ensure(before.type!=='link',`文件是链接，未覆盖：${p}`);
    const after={type:'file',sha256:hash(content),mode:mode??before.mode??0o600};
    const key=path.resolve(p),previous=this.ops.get(key);
    if(same(before,after)){this.ops.delete(key);return;}
    ensure(!previous||previous.after.type==='file',`修复计划来源冲突：${p}`);
    this.ops.set(key,{path:key,before:previous?.before??before,after,content});
  }
  link(p,target) {this.safe(path.dirname(p));this.safe(target);let before;
    if(exists(p)&&fs.lstatSync(p).isDirectory()&&!fs.lstatSync(p).isSymbolicLink()) {this.lines.push(`保留现有实体目录：${p}`);return;}
    before=state(p);ensure(before.type!=='file',`连接位置被文件占用：${p}`);
    const after={type:'link',target:path.resolve(target)};if(!same(before,after))this.ops.set(path.resolve(p),{path:path.resolve(p),before,after});
  }
  copy(source,target) {
    const s=fs.lstatSync(source);ensure(!s.isSymbolicLink(),`来源含链接，未跨目录复制：${source}`);
    if(s.isDirectory()) for(const e of fs.readdirSync(source)) this.copy(path.join(source,e),path.join(target,e));
    else {ensure(s.isFile(),'插件来源含特殊文件');this.file(target,fs.readFileSync(source),s.mode&0o777);}
  }
  removeTree(target) {
    this.safe(path.dirname(target));let kind;try{kind=fs.lstatSync(target);}catch(e){if(e.code==='ENOENT')return;throw e;}
    if(kind.isDirectory()&&!kind.isSymbolicLink()) {this.safe(target);for(const e of fs.readdirSync(target))this.removeTree(path.join(target,e));this.removeDirs.add(path.resolve(target));}
    else {const key=path.resolve(target);ensure(!this.ops.has(key),'缓存清理与写入范围冲突');this.ops.set(key,{path:key,before:state(key),after:{type:'missing'}});}
  }
  get(p) {return this.ops.get(path.resolve(p))?.content??read(p);}
  object(p) {const b=this.get(p);return b?JSON.parse(b.toString('utf8').replace(/^\uFEFF/,'')):null;}
  skip(s) {this.skipped++;this.lines.push(s);}
  commit() {
    if(!this.ops.size&&!this.removeDirs.size)return null;
    const dir=path.join(this.options.backupRoot,`${Date.now()}-${crypto.randomUUID()}`);fs.mkdirSync(dir,{recursive:true,mode:0o700});
    const manifest={schemaVersion:1,kind:this.options.kind,home:actual(this.options.home),roots:this.roots,stage:'prepared',operations:[],createdDirs:[],removedDirs:[...this.removeDirs]};
    for(const op of this.ops.values()) {for(let d=path.dirname(op.path);!exists(d);d=path.dirname(d)){this.safe(d);if(!manifest.createdDirs.includes(d))manifest.createdDirs.push(d);}}
    const save=()=>atomic(path.join(dir,'manifest.json'),bytes(manifest));
    for(const [i,op] of [...this.ops.values()].entries()) {
      ensure(same(state(op.path),op.before),`检查后文件已变化，请重新检查：${op.path}`);
      if(op.before.type==='file') {fs.writeFileSync(path.join(dir,`${i}.before`),fs.readFileSync(op.path),{flag:'wx',mode:0o600});ensure(hash(fs.readFileSync(path.join(dir,`${i}.before`)))===op.before.sha256,'备份校验失败');}
      manifest.operations.push({path:op.path,before:op.before,after:op.after,backup:`${i}.before`,written:false});
    }
    save();
    try {
      for(const [i,op] of [...this.ops.values()].entries()) {
        this.safe(path.dirname(op.path));ensure(same(state(op.path),op.before),`文件在修复时发生变化：${op.path}`);
        fs.mkdirSync(path.dirname(op.path),{recursive:true});
        // Record the pending write first so a crash can still be restored.
        manifest.operations[i].written=true;save();
        if(op.after.type==='missing')fs.unlinkSync(op.path);
        else if(op.after.type==='link') {if(op.before.type==='link')fs.unlinkSync(op.path);fs.symlinkSync(op.after.target,op.path,process.platform==='win32'?'junction':'dir');}
        else atomic(op.path,op.content,op.after.mode);
        ensure(same(state(op.path),op.after),'写入后校验失败');
      }
      for(const d of [...this.removeDirs].sort((a,b)=>b.length-a.length)){this.safe(d);if(exists(d))fs.rmdirSync(d);}
      manifest.stage='applied';save();return dir;
    } catch(e) {manifest.stage='interrupted';save();throw Error(`${e.message}\n备份已保留：${dir}`);}
  }
}
function restore(o) {
  const dir=actual(o.backup);ensure(inside(dir,actual(o.backupRoot)),'备份不在修复记录目录');
  const m=json(path.join(dir,'manifest.json'));ensure(m.schemaVersion===1&&actual(m.home)===actual(o.home),'备份属于其他用户目录');
  ensure(['applied','prepared','interrupted','restoring','restored'].includes(m.stage),'这份记录不需要恢复');
  const p=new Plan(o);ensure(Array.isArray(m.roots)&&m.roots.every(r=>p.roots.includes(actual(r))),'备份的写入范围已变化，请进一步检查，未覆盖');
  for(const op of m.operations) {
    p.safe(path.dirname(op.path));const s=state(op.path);
    ensure(same(s,op.before)||(m.stage!=='restored'&&same(s,op.after)),`修复后文件已被修改，未覆盖：${op.path}`);
    if(op.before.type==='file') {ensure(/^\d+\.before$/.test(op.backup),'备份索引无效');ensure(hash(fs.readFileSync(path.join(dir,op.backup)))===op.before.sha256,'备份内容校验失败');}
  }
  for(const d of [...(m.createdDirs||[]),...(m.removedDirs||[])]){p.safe(d);if(exists(d))ensure(fs.lstatSync(d).isDirectory()&&!fs.lstatSync(d).isSymbolicLink(),'备份中的目录在修复后发生变化');}
  if(o.verifyOnly||m.stage==='restored')return {message:'所选文件备份已通过恢复前核对。',backup:dir,applied:false,canRestore:m.stage!=='restored'};
  m.stage='restoring';atomic(path.join(dir,'manifest.json'),bytes(m));
  for(const d of [...(m.removedDirs||[])].sort((a,b)=>a.length-b.length))fs.mkdirSync(d,{recursive:true});
  for(const op of [...m.operations].reverse()) {
    p.safe(path.dirname(op.path));
    const current=state(op.path);if(same(current,op.before))continue;
    ensure(same(current,op.after),'恢复期间文件发生变化');
    if(op.before.type==='file') {fs.mkdirSync(path.dirname(op.path),{recursive:true});atomic(op.path,fs.readFileSync(path.join(dir,op.backup)),op.before.mode);}
    else {if(current.type!=='missing')fs.unlinkSync(op.path);if(op.before.type==='link')fs.symlinkSync(op.before.target,op.path,process.platform==='win32'?'junction':'dir');}
    ensure(same(state(op.path),op.before),'恢复后文件校验失败');
  }
  for(const d of [...(m.createdDirs||[])].sort((a,b)=>b.length-a.length)) {if(exists(d)&&fs.readdirSync(d).length===0)fs.rmdirSync(d);}
  m.stage='restored';atomic(path.join(dir,'manifest.json'),bytes(m));
  return {message:'已恢复所选修复前的文件。原问题也可能恢复，请按需重新检查。',backup:dir,applied:false,canRestore:false};
}
function children(p) {try{return fs.readdirSync(p,{withFileTypes:true}).filter(e=>e.isDirectory()&&!e.isSymbolicLink()).map(e=>path.join(p,e.name));}catch(e){if(e.code==='ENOENT')return [];throw e;}}
function runtime(o,p,copy=false) {
  const win=process.platform==='win32',name=win?'node.exe':'node',cli=win?'codex.exe':'codex',repl=win?'node_repl.exe':'node_repl';
  const reference=path.join(o.resources,'cua_node','bin');ensure(exists(path.join(reference,name)),'当前客户端未附带受支持的电脑操控运行时；未替换其他版本');
  const nodeHash=hash(fs.readFileSync(path.join(reference,name)));
  const candidates=children(path.join(o.localRoot,'runtimes','cua_node')).map(d=>path.join(d,'bin')).filter(d=>{
    if(!exists(path.join(d,name))||!exists(path.join(d,repl)))return false;
    return hash(fs.readFileSync(path.join(d,name)))===nodeHash&&hash(fs.readFileSync(path.join(d,repl)))===hash(fs.readFileSync(path.join(reference,repl)));
  });
  const bin=candidates[0]??path.join(o.localRoot,'runtimes','cua_node','hub-'+nodeHash.slice(0,16),'bin');
  if(!candidates.length&&copy)p.copy(reference,bin);
  const sourceCli=o.cli;const cliHash=hash(fs.readFileSync(sourceCli));
  const userCli=children(path.join(o.localRoot,'bin')).map(d=>path.join(d,cli)).find(f=>exists(f)&&hash(fs.readFileSync(f))===cliHash)??path.join(o.localRoot,'bin','hub-'+cliHash.slice(0,16),cli);
  if(!exists(userCli)&&copy)p.file(userCli,fs.readFileSync(sourceCli),0o755);
  return {reference,bin,bins:[...new Set([bin,...candidates])],node:path.join(bin,name),repl:path.join(bin,repl),cli:userCli};
}
function marketplace(o) {const r=path.join(o.resources,'plugins','openai-bundled');ensure(exists(path.join(r,'.agents','plugins','marketplace.json')),'此客户端未附带受支持的插件市场；未下载或安装其他来源');return r;}
function plugins(o,p,names) {
  const source=marketplace(o),mirror=path.join(o.home,'marketplaces','openai-bundled-local');
  const document=json(path.join(source,'.agents','plugins','marketplace.json'));ensure(document.name==='openai-bundled'&&Array.isArray(document.plugins),'内置插件市场结构未识别');
  const roots=[mirror,path.join(o.home,'.tmp','bundled-marketplaces','openai-bundled')];
  for(const root of roots) {p.copy(source,root);}
  const chosen=document.plugins.filter(entry=>names.includes(entry.name)||exists(path.join(o.home,'plugins','cache','openai-bundled',entry.name))||(o.enabledPlugins||[]).includes(entry.name+'@openai-bundled'));
  const install=registered=>{for(const entry of chosen) {
    const name=entry.name;ensure(typeof name==='string'&&/^[\w-]+$/.test(name),'插件名称无效');
    const sourceDir=path.join(source,'plugins',name),descriptor=path.join(sourceDir,'.codex-plugin','plugin.json');if(!exists(descriptor))continue;
    const version=json(descriptor).version;ensure(typeof version==='string'&&/^[\w.-]+$/.test(version),'插件版本无效');
    const ready=registered.get(name),effective=ready?.version??version,cache=path.join(o.home,'plugins','cache','openai-bundled',name,effective),base=path.dirname(cache);
    // Match the official installer's single active version. Retired versions
    // are journalled too, so an old `local` directory cannot shadow the repair.
    if(ready)for(const old of children(base))if(![effective,'latest'].includes(path.basename(old)))p.removeTree(old);
    p.copy(ready?.source??sourceDir,cache);
    const latest=path.join(base,'latest');
    if(exists(latest)&&!fs.lstatSync(latest).isSymbolicLink()&&fs.lstatSync(latest).isDirectory())p.copy(ready?.source??sourceDir,latest);
    else p.link(latest,cache);
  }};
  if(o.action==='repair')require('./plugin-install.cjs').stage({cli:o.cli,marketplace:source},chosen.map(e=>e.name),install);
  else install(new Map());
  p.lines.push('已按当前安装包核对插件市场、稳定副本和已有插件缓存；其他可选插件的安装、启用状态保持原设置。');
  if(o.action==='repair'&&chosen.length)p.lines.push('已用当前客户端的正式插件安装命令在隔离目录注册并复核，再将安装结果纳入本次备份和修复；未导入隔离目录的登录信息、数据库或其他配置。');
  if(o.action==='check'&&chosen.length){p.registrationRequired=true;p.lines.push('现有插件可在确认修复时用正式安装命令重新校验注册，检查阶段不启动客户端程序。');}
  return {source,mirror};
}
function chrome(o,p) {
  const {source}=plugins(o,p,['browser','chrome']);const rt=runtime(o,p,true);
  const descriptor=json(path.join(source,'plugins','chrome','.codex-plugin','plugin.json'));
  const version=descriptor.version,root=path.join(o.home,'plugins','cache','openai-bundled','chrome',version);
  const ids=json(path.join(source,'plugins','chrome','scripts','extension-ids.json'));
  ensure(/^[a-z][a-z0-9_.]+$/.test(ids.extensionHostName)&&Array.isArray(ids.extensionIds)&&ids.extensionIds.length>0&&ids.extensionIds.every(x=>/^[a-p]{32}$/.test(x)),'浏览器扩展身份未识别');
  const win=process.platform==='win32',platform=win?'windows':'macos';
  const host=path.join(root,'extension-host',platform,process.arch,win?'extension-host.exe':'ChatGPT for Chrome');
  ensure(p.get(host),'当前安装包没有匹配本机架构的浏览器连接程序');
  const client=path.join(root,'scripts','browser-client.mjs'),service=path.join(root,'scripts','browser-service.mjs');
  ensure(p.get(client)&&p.get(service),'当前浏览器插件不完整');
  const config={schemaVersion:1,channel:'prod',browserClientPath:client,codexCliPath:rt.cli,nodePath:rt.node,nodeReplPath:rt.repl,proxyHost:'127.0.0.1',proxyPort:0};
  p.file(path.join(path.dirname(host),'extension-host-config.json'),bytes(config));
  const native={allowed_origins:[...new Set(ids.extensionIds.map(id=>`chrome-extension://${id}/`))],description:'ChatGPT browser native messaging host',name:ids.extensionHostName,path:host,type:'stdio'};
  const manifests=win?[path.join(o.userHome,'AppData','Local','OpenAI','extension',ids.extensionHostName+'.json')]:[
    'Google/Chrome','Chromium','Google/ChromeForTesting','Google/Chrome for Testing','Microsoft Edge','BraveSoftware/Brave-Browser','Vivaldi','com.operasoftware.Opera'
  ].map(x=>path.join(o.userHome,'Library','Application Support',x,'NativeMessagingHosts',ids.extensionHostName+'.json'));
  for(const target of manifests)p.file(target,bytes(native));
  if(win) {
    ensure(ids.windowsNativeMessaging?.registryRoot==='HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts','浏览器注册表契约未识别');
    // A .reg operation is journalled separately by the native host.
    p.registry={key:'Software\\Google\\Chrome\\NativeMessagingHosts\\'+ids.extensionHostName,value:manifests[0]};
  }
  const paths={browserClientPath:client,browserServicePath:service,codexCliPath:rt.cli,codexHome:o.home,extensionHostPath:host,nodePath:rt.node,nodeModuleDirs:[path.join(rt.bin,'node_modules')],nodeReplPath:rt.repl,resourcesPath:o.resources};
  const identity=(prefix,list)=>prefix+hash(Buffer.from(list.join('\0')+'\0')).slice(0,32);
  const now=new Date().toISOString();
  const entry={schemaVersion:2,appServerProtocolVersion:2,appVersion:version,channel:'prod',cliVersion:version,
    entryId:identity('codex-runtime-',[ids.extensionHostName,...ids.extensionIds,'prod',version,host,rt.cli,o.home,o.resources]),
    extensionBuildChannels:['prod'],extensionIds:ids.extensionIds,
    installId:identity('codex-install-',[ids.extensionHostName,o.resources,o.home]),nativeHostNames:[ids.extensionHostName],nativeHostProtocolVersion:2,nativeHostVersion:version,paths,proxyHost:'127.0.0.1',proxyPort:0,updatedAt:now};
  for(const target of [...new Set([path.join(o.localRoot,'chrome-native-hosts-v2.json'),path.join(o.home,'chrome-native-hosts-v2.json')])]) {
    const doc=p.object(target)??{schemaVersion:2,entries:[]};ensure(doc.schemaVersion===2&&Array.isArray(doc.entries),'浏览器连接记录结构未知，未覆盖');
    const matching=doc.entries.filter(e=>e.entryId===entry.entryId);
    if(matching.length===1&&JSON.stringify(matching[0].paths)===JSON.stringify(paths)&&JSON.stringify(matching[0].extensionIds)===JSON.stringify(ids.extensionIds))continue;
    // Preserve unrelated registrations, including a different Codex home.
    doc.entries=doc.entries.filter(e=>!(e.entryId===entry.entryId||(e.installId===entry.installId&&e.paths?.codexHome===o.home)));
    doc.entries.push(entry);p.file(target,bytes(doc));
  }
  p.lines.push('Chrome 本机连接已按当前扩展 ID、运行时和用户目录核对，包含两份 v2 记录及服务路径。修复后需要新建工具会话；浏览器扩展仍需在浏览器中正常安装。');
  return rt;
}
function serviceFiles(o,rt) {
  const source=marketplace(o),files=[];
  for(const name of ['browser','chrome']) {
    const descriptor=path.join(source,'plugins',name,'.codex-plugin','plugin.json');if(!exists(descriptor))continue;
    const version=json(descriptor).version;
    for(const root of [path.join(o.home,'marketplaces','openai-bundled-local','plugins',name),path.join(o.home,'.tmp','bundled-marketplaces','openai-bundled','plugins',name),path.join(o.home,'plugins','cache','openai-bundled',name,version)]){
      const f=path.join(root,'scripts','browser-service.mjs');if(exists(f))files.push({target:f,reference:path.join(source,'plugins',name,'scripts','browser-service.mjs'),version});
    }
  }
  for(const bin of rt.bins){const rel=path.join('node_modules','@oai','browser-desktop','scripts','browser-service.mjs');if(exists(path.join(bin,rel)))files.push({target:path.join(bin,rel),reference:path.join(rt.reference,rel),version:null});}
  return files;
}
function browserHeaders(o,p) {
  const rt=runtime(o,p);let n=0;
  for(const {target,reference} of serviceFiles(o,rt)) {
    const b=read(target),ref=read(reference);if(!ref) {p.skip('当前安装包不包含相应浏览器服务，已跳过');continue;}
    const source=headerPatch.inspect(ref,{allowUnsupported:true});const current=headerPatch.inspect(b,{allowUnsupported:true});
    if(source.state==='unsupported'||current.state==='unsupported'){p.skip('浏览器服务完整哈希未匹配到受支持的兼容方案，保留原文件');continue;}
    ensure(source.profile===current.profile,'浏览器服务与当前安装包不是同一来源');p.file(target,Buffer.from(current.patched));n++;
  }
  p.lines.push(`已核对 ${n} 份支持的浏览器服务。只兼容第三方厂商的精确缺 token 错误；真实认证、请求头、网络策略及其他错误仍按原逻辑处理。`);
}
function axValid(b){try{const d=zlib.brotliDecompressSync(b);return WebAssembly.validate(d);}catch{return false;}}
function browserAx(o,p){
  const rt=runtime(o,p),groups=serviceFiles(o,rt).map(e=>({...e,service:read(e.reference),reference:e.reference.replace('browser-service.mjs','browser-accessibility.wasm.br'),target:e.target.replace('browser-service.mjs','browser-accessibility.wasm.br')}));
  let count=0;
  for(const g of groups){if(!g.service?.includes('browser-accessibility.wasm.br'))continue;
    const b=read(g.reference),current=read(g.target);let healthy=axValid(b)?b:null;
    if(!healthy&&g.version){const peers=groups.filter(x=>x.version===g.version&&hash(x.service)===hash(g.service)).map(x=>read(x.reference)).filter(axValid);if(peers.length&&new Set(peers.map(hash)).size===1)healthy=peers[0];}
    if(!healthy){p.skip('未找到相同版本、相同服务的健康 AX 资源，保留原文件');continue;}
    if(current&&axValid(current)&&hash(current)!==hash(healthy)){p.skip('现有 AX 文件健康但与来源不同，未混用版本');continue;}
    p.file(g.target,healthy);count++;
  }
  p.lines.push(`已核对 ${count} 份浏览器辅助功能资源；使用 Brotli 解压及 WASM 校验，修复后请新建工具会话。`);
}
function patchTemp(script,original,args=[]) {
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'hub-script-'));const f=path.join(dir,'asset.mjs');
  try{fs.writeFileSync(f,original);const r=spawnSync(process.execPath,[path.join(__dirname,script),f,...args],{encoding:'utf8',timeout:20000,windowsHide:true});
    if(r.status!==0)return null;const next=fs.readFileSync(f);if(next.equals(original))return original;
    const syntax=spawnSync(process.execPath,['--check',f],{encoding:'utf8',timeout:20000,windowsHide:true});ensure(syntax.status===0,'候选脚本语法校验失败，未替换原文件');return next;
  }finally{fs.rmSync(dir,{recursive:true,force:true});}
}
function context(o,p){
  if(process.platform!=='win32'){p.lines.push('Windows 跨调用 helper 补丁不适用于 macOS，未修改文件。');return;}
  const rt=runtime(o,p),rel=path.join('node_modules','@oai','sky','dist','project','cua','sky_js','src','targets','windows','internal','helper_transport.js');let count=0;
  for(const bin of rt.bins){const f=path.join(bin,rel),b=read(f);if(!b)continue;const h=hash(b);
    if(h==='3600ac240cd6cb7029f1e489df990cae22d72177350b8084412df1f3fa5bb60a'){count++;continue;}
    if(h!=='6423ba834f18139d55cdac2290c91cd9b24b568332b07cddd2a7eda043702b7c'){p.skip('当前 helper 不匹配已知跨调用错误，未应用旧补丁');continue;}
    let s=b.toString('utf8');const replace=(a,b)=>{ensure(s.split(a).length===2,'helper 锚点不唯一');s=s.replace(a,b);};
    replace('import{spawnSync as n,spawn as s}from"node:child_process";','import{AsyncLocalStorage as ComputerUseAsyncLocalStorage}from"node:async_hooks";/* computer_use_node_repl_request_context_patch */import{spawnSync as n,spawn as s}from"node:child_process";');
    replace('e(this,E,"f").set(a,{child:i,codexTurnMetadata:r,createElicitation:l,method:n,params:s,resolve:t,reject:o,timeout:u,turnScope:d}),i.stdin.write','e(this,E,"f").set(a,{child:i,codexTurnMetadata:r,createElicitation:l,method:n,params:s,resolve:t,reject:o,runInRequestContext:ComputerUseAsyncLocalStorage.snapshot(),timeout:u,turnScope:d}),i.stdin.write');
    replace('if(null!=s)return void e(this,f,"m",T).call(this,n,s);','if(null!=s)return void n.runInRequestContext((()=>e(this,f,"m",T).call(this,n,s)));');
    ensure(hash(Buffer.from(s))==='3600ac240cd6cb7029f1e489df990cae22d72177350b8084412df1f3fa5bb60a','helper 输出完整哈希不匹配');p.file(f,Buffer.from(s));count++;
  }p.lines.push(`已核对 ${count} 份已知跨调用 helper。修复保留审批，完成后需要新建工具会话。`);
}
// Minimal ASAR reader/writer preserves links, executable bits, unpacked files,
// every untouched byte and per-entry integrity. No npm/network dependency.
function archiveRead(file) {
  const raw=fs.readFileSync(file);ensure(raw.length>16&&raw.readUInt32LE(0)===4,'ASAR 头未识别');
  const size=raw.readUInt32LE(4),len=raw.readUInt32LE(12);ensure(len<64*1024*1024&&size+8<=raw.length&&len+16<=size+8,'ASAR 长度异常');
  const header=JSON.parse(raw.subarray(16,16+len).toString('utf8')),entries=[];
  function walk(files,prefix){for(const [name,e] of Object.entries(files)){ensure(!['.','..'].includes(name)&&!/[\\/]/.test(name),'ASAR 路径异常');const key=prefix+name;if(e.files)walk(e.files,key+'/');else if(!e.unpacked&&!e.link){const offset=Number(e.offset),length=e.size;ensure(Number.isSafeInteger(offset)&&offset>=0&&Number.isSafeInteger(length)&&length>=0&&size+8+offset+length<=raw.length,'ASAR 条目越界');entries.push({name:key,entry:e,bytes:raw.subarray(size+8+offset,size+8+offset+length)});}}}walk(header.files,'');
  return {header,entries,raw};
}
function archiveWrite(a){let offset=0;const data=[];for(const x of a.entries){x.entry.offset=String(offset);x.entry.size=x.bytes.length;offset+=x.bytes.length;data.push(x.bytes);if(x.entry.integrity){ensure(x.entry.integrity.algorithm==='SHA256','ASAR 完整性算法未识别');const blockSize=x.entry.integrity.blockSize;ensure(Number.isInteger(blockSize)&&blockSize>0,'ASAR 块大小异常');const blocks=[];for(let p=0;p<x.bytes.length;p+=blockSize)blocks.push(hash(x.bytes.subarray(p,p+blockSize)));x.entry.integrity={algorithm:'SHA256',hash:hash(x.bytes),blockSize,blocks};}}
  const h=Buffer.from(JSON.stringify(a.header)),padded=(4+h.length+3)&~3,head=Buffer.alloc(8+4+padded);head.writeUInt32LE(4,0);head.writeUInt32LE(4+padded,4);head.writeUInt32LE(padded,8);head.writeUInt32LE(h.length,12);h.copy(head,16);return Buffer.concat([head,...data]);}
function archivePlan(o,p) {
  const file=path.join(o.resources,'app.asar'),a=archiveRead(file);let count=0;
  const candidates=a.entries.filter(x=>x.name.endsWith('.js')&&(x.name.startsWith('.vite/build/')||x.name.startsWith('webview/assets/')));
  const jobs=o.kind==='proxy'?['proxy']:o.kind==='surface'?['surface']:o.kind==='discovery'?['discovery']:['power','ultra','locale','goal','plugin','browser','trusted'];
  for(const job of jobs){let matched=0;
    for(const e of candidates){let text=e.bytes.toString('utf8'),next=null;
      if(job==='proxy'&&text.includes('NODE_REPL_NODE_MODULE_DIRS')&&text.includes('CODEX_WINDOWS_REGISTERED_CORE'))next=Buffer.from(require('./patch-node-repl-proxy-env.cjs').patchSource(text).source);
      if(job==='surface'&&e.name.startsWith('.vite/build/')&&text.includes('CUA_REPL_ENABLED_SURFACES'))next=patchTemp('patches/computerUseSurface.cjs',e.bytes);
      if(job==='trusted'&&text.includes('NODE_REPL_TRUSTED_CODE_PATHS')&&e.name.startsWith('.vite/build/'))next=patchTemp('patches/nodeReplTrustedPaths.cjs',e.bytes);
      if(job==='power'&&text.includes('harborEnabled:')&&text.includes('isEverydayWorkMode:'))next=patchTemp('patches/powerSlider.cjs',e.bytes);
      if(job==='ultra'&&text.includes('chatgpt-ultra-effort-migration')&&text.includes('showUltraInModelPickerSlider'))next=patchTemp('patches/ultraSlider.cjs',e.bytes);
      if(job==='locale'&&text.includes('enable_i18n')&&text.includes('locale_source'))next=patchTemp('patches/locale.cjs',e.bytes);
      if(job==='goal'&&text.includes('3074100722')){
        const re=/([\w$]+)=([\w$]+)\(`3074100722`\)&&([\w$]+)\(([\w$]+)\?\.config,`goals`\)===!0&&([\w$]+)!==`cloud`(&&![\w$]+)?(?=,)/g;
        if([...text.matchAll(re)].length===1)next=Buffer.from(text.replace(re,(_,result,gate,config,c,mode,side='')=>`${result}=${mode}!==\`cloud\`${side}/*CODEXHUB_GOAL_V1*/`));
      }
      if(job==='plugin'&&text.includes('openPluginInstall')&&text.includes('authMethod:')){
        const re=/\{authMethod:([\w$]+)\}=([\w$]+)\(\),((?:\{data:[\w$]+\}=[\w$]+\(\),)?)([\w$]+)=([\w$]+)\(\1\),/g;
        if([...text.matchAll(re)].length===1)next=Buffer.from(text.replace(re,(_,auth,hook,middle,blocked)=>`{authMethod:${auth}}=${hook}(),${middle}${blocked}=!1/*CODEXHUB_PLUGIN_UI_V1*/,`));
        if(!next){const migrated=/\{authMethod:([\w$]+)\}=([\w$]+)\(\),([^;]{0,2500}?)([\w$]+)=([\w$]+)\(\1\),([\w$]+=[\w$]+\.kind===`manage`,)/g;
          if([...text.matchAll(migrated)].length===1)next=Buffer.from(text.replace(migrated,(_,auth,hook,middle,blocked,unused,tail)=>`{authMethod:${auth}}=${hook}(),${middle}${blocked}=!1/*CODEXHUB_PLUGIN_UI_V1*/,${tail}`));}
      }
      if(job==='plugin'&&text.includes('533078438')){
        const re=/\{authMethod:([\w$]+)\}=([\w$]+)\(\),([\w$]+)=([\w$]+)\(`533078438`\),([\w$]+)=([\w$]+)\(\1\),([\w$]+)=e&&\3&&\5,([\w$]+)=([\w$]+)\(\{hostId:([\w$]+)\}\),([\w$]+)=e&&\8&&!\5,/g;
        if([...text.matchAll(re)].length===1)next=Buffer.from(text.replace(re,(_,auth,hook,flag,gate,blocked,isBlocked,disabled,available,availability,host,enabled)=>`{authMethod:${auth}}=${hook}(),${flag}=${gate}(\`533078438\`),${blocked}=!1,${disabled}=e&&${flag}&&${blocked},${available}=${availability}({hostId:${host}}),${enabled}=e&&${available}&&!${blocked},/*CODEXHUB_PLUGIN_SIDEBAR_V1*/`));
      }
      if(job==='browser'){
        // Only complete independent sender/receiver objects; leave unknown hooks intact.
        const re=/inAppBrowserUse:([\w$]+)\.inAppBrowserUse,(mcpAppsBrowserUse:[^,}]+,)?inAppBrowserUseAllowed:\1\.inAppBrowserUseAllowed,((?:[\w$]+:[^,}]+,){0,10}?)browserPane:\1\.browserPane,((?:[\w$]+:[^,}]+,){0,8}?)externalBrowserUse:\1\.externalBrowserUse,externalBrowserUseAllowed:\1\.externalBrowserUseAllowed,((?:[\w$]+:[^,}]+,){0,6}?)computerUse:/g;
        let browserNext=text;
        if([...text.matchAll(re)].length===1)browserNext=text.replace(re,(_,v,m='',between,other,tail)=>`inAppBrowserUse:!0,${m}inAppBrowserUseAllowed:!0,${between}browserPane:!0,${other}externalBrowserUse:!0,externalBrowserUseAllowed:!0,${tail}computerUse:`);
        const receiver=/([\w$]+)=([\w$]+)===`win32`&&([\w$]+)\.CODEX_ELECTRON_ENABLE_WINDOWS_COMPUTER_USE===`1`\?\{\.\.\.([\w$]+),computerUse:!0,computerUseNodeRepl:!0\}:\4/g;
        if([...browserNext.matchAll(receiver)].length===1)browserNext=browserNext.replace(receiver,(_,result,platform,env,base)=>`${result}=${platform}===\`win32\`?{...${base},browserPane:!0,inAppBrowserUse:!0,inAppBrowserUseAllowed:!0,externalBrowserUse:!0,externalBrowserUseAllowed:!0,...${env}.CODEX_ELECTRON_ENABLE_WINDOWS_COMPUTER_USE===\`1\`?{computerUse:!0,computerUseNodeRepl:!0}:{}}:${base}`);
        if(browserNext!==text)next=Buffer.from(browserNext);
      }
      if(job==='discovery'&&e.name.startsWith('.vite/build/')&&text.includes('.imageGuidedAssets,')){
        const id='[A-Za-z_$][\\w$]*';const re=new RegExp(`(\\.\\.\\.${id}\\.${id}\\.imageGuidedAssets,autoInstallOptOutKey:[^,]+,isAvailable:)\\(\\{features:(${id})\\}\\)=>\\2\\.imageGuidedAssets(?=})`,'g');
        if([...text.matchAll(re)].length===1)next=Buffer.from(text.replace(re,'$1()=>!0/*CODEXHUB_IMAGE_GUIDED_DISCOVERY_V1*/,installWhenMissing:!1'));
      }
      if(next){matched++;if(!next.equals(e.bytes)){e.original??=e.bytes;e.bytes=next;count++;}}
    }
    if(matched)p.lines.push(`${({power:'Power 拖动条',ultra:'Ultra 偏好',locale:'语言设置',goal:'Goal 入口',plugin:'插件安装入口',browser:'浏览器主程序',trusted:'运行时可信路径',proxy:'子进程代理继承',surface:'Windows Any App',discovery:'Image-guided Assets 可发现性'})[job]}：匹配到 ${matched} 个支持的组件`);
    else p.skip(`${({power:'Power',ultra:'Ultra',locale:'语言',goal:'Goal',plugin:'插件入口',browser:'浏览器',trusted:'可信路径',proxy:'代理继承',surface:'Any App',discovery:'Image-guided Assets'})[job]}：当前布局未匹配、已开放或不适用，保留原代码`);
  }
    if(count){
    const syntaxDir=fs.mkdtempSync(path.join(os.tmpdir(),'hub-package-syntax-'));try{for(const e of candidates.filter(e=>e.original)){const f=path.join(syntaxDir,'candidate.'+(e.name.startsWith('.vite/build/')?'cjs':'mjs'));fs.writeFileSync(f,e.bytes);const checked=spawnSync(process.execPath,['--check',f],{encoding:'utf8',timeout:20000,windowsHide:true});ensure(checked.status===0,'候选客户端脚本语法校验失败，保留原程序');}}finally{fs.rmSync(syntaxDir,{recursive:true,force:true});}
    const next=archiveWrite(a);const verified=archiveReadBytes(next);ensure(verified.entries.length===a.entries.length,'候选 ASAR 校验失败');
    p.package={source:file,before:hash(a.raw),after:hash(next),bytes:next};p.lines.push('需要修复客户端安装文件；会准备独立备份后更新，Hub 保持运行。');
  }
}
function archiveReadBytes(b){const d=fs.mkdtempSync(path.join(os.tmpdir(),'hub-asar-check-'));try{const f=path.join(d,'app.asar');fs.writeFileSync(f,b);return archiveRead(f);}finally{fs.rmSync(d,{recursive:true,force:true});}}
function surface(o,p){if(process.platform!=='win32'){p.lines.push('Windows Any App 兼容补丁不适用于 macOS；请在系统设置中检查辅助功能与屏幕录制权限。');return;}
  plugins(o,p,['computer-use','unified-computer-use']);runtime(o,p,true);
  const source=marketplace(o);const desc=path.join(source,'plugins','unified-computer-use','.codex-plugin','plugin.json');
  if(!exists(desc)||!exists(path.join(source,'plugins','unified-computer-use','scripts','launch.mjs'))){archivePlan(o,p);return;}
  const version=json(desc).version,roots=[path.join(o.home,'marketplaces','openai-bundled-local','plugins','unified-computer-use'),path.join(o.home,'.tmp','bundled-marketplaces','openai-bundled','plugins','unified-computer-use'),path.join(o.home,'plugins','cache','openai-bundled','unified-computer-use',version)];
  const profiles=json(path.join(__dirname,'surface-profiles.json'));
  for(const root of roots)for(const profile of profiles){const f=path.join(root,...profile.RelativePath.split('\\')),b=p.get(f);if(!b)continue;
    const h=hash(b);if(h===profile.PatchedSha256.toLowerCase())continue;
    if(h!==profile.OriginalSha256.toLowerCase()){p.skip('旧版 Any App 文件完整哈希不匹配，已跳过');continue;}
    const original=b.toString();const newline=original.includes('\r\n')&&!/(?<!\r)\n/.test(original)?'\r\n':'\n';
    const replacement=profile.Replacement.replace(/\r\n/g,'\n').replace(/[\r\n]+$/,'').replace(/\n/g,newline);
    let next;if(profile.UseRegex){const re=/const surfaces = new Set\(\s*\(process\.env\.CUA_REPL_ENABLED_SURFACES \?\? "browser,computer"\)\.split\(","\)\.map\(\(surface\) => surface\.trim\(\)\)\.filter\(Boolean\)\s*\);/g;ensure([...original.matchAll(re)].length===1,'旧版 Any App 锚点不唯一');next=original.replace(re,replacement);}
    else{const anchor=profile.Anchor.replace(/\r\n/g,'\n').replace(/[\r\n]+$/,'').replace(/\n/g,newline);ensure(original.split(anchor).length===2,'旧版 Any App 说明锚点不唯一');next=original.replace(anchor,replacement);}
    ensure(hash(Buffer.from(next))===profile.PatchedSha256.toLowerCase(),'旧版 Any App 候选文件完整哈希不匹配，未替换');p.file(f,Buffer.from(next));
  }p.lines.push('已检查旧版 Any App 启动入口和运行时使用说明；保留审批及其他插件状态。');
}
function screenshot(o,p){
  if(process.platform!=='win32'||!(o.windowsBuild>=10240&&o.windowsBuild<22000)) {p.lines.push('本专项仅用于 Windows 10 的已知截图错误，当前系统不适用。');return;}
  if(!/SetIsBorderRequired|0x80004002|FrameArrived timed out/i.test(o.evidence??'')){p.lines.push('尚无已知截图错误的证据。可将截图失败提示粘贴到“错误提示”，再检查；未修改 helper。');return;}
  const profiles=json(path.join(__dirname,'win10-profiles.json')),rt=runtime(o,p);let count=0;
  for(const bin of rt.bins){const f=path.join(bin,'node_modules','@oai','sky','bin','windows','codex-computer-use.exe'),b=read(f);if(!b)continue;
    const h=hash(b),profile=profiles.find(x=>[x.OriginalSha256,x.PatchedSha256].some(v=>v.toLowerCase()===h));
    if(!profile){p.skip('截图 helper 完整哈希未匹配，未替换未知二进制');continue;}
    ensure(json(path.join(bin,'node_modules','@oai','sky','package.json')).version===profile.SkyVersion,'截图 helper 和运行时版本不一致');
    if(h===profile.PatchedSha256.toLowerCase()){count++;continue;}
    const next=Buffer.from(b);
    for(const r of profile.Regions){const original=Buffer.from(r.OriginalHex,'hex'),after=Buffer.from(r.PatchedHex,'hex');ensure(original.length===after.length&&r.Offset>=0&&r.Offset+original.length<=b.length&&b.subarray(r.Offset,r.Offset+original.length).equals(original),'截图 helper 字节区域不匹配');after.copy(next,r.Offset);}
    ensure(hash(next)===profile.PatchedSha256.toLowerCase(),'截图 helper 候选完整哈希未通过');p.file(f,next);count++;
  }p.lines.push(`已核对 ${count} 份受支持的 Windows 10 截图 helper。这里只修复已知截图兼容问题，保留审批和系统权限。`);
}
function run(o) {
  ensure(['check','repair','restore'].includes(o.action),'未知修复操作');
  if(o.action==='restore')return restore(o);
  const p=new Plan(o);
  if(typeof o.configCandidate==='string')p.file(path.join(o.home,'config.toml'),Buffer.from(o.configCandidate));
  switch(o.kind){
    case 'models':p.lines.push('已核对模型强度和速度基础配置。Power、Ultra 偏好、Goal、插件入口和语言兼容会在下一次“增强启动”时按组件独立适配；不支持的组件保留原样。Fast 是否可用仍取决于模型与上游支持。');break;
    case 'mcp':p.lines.push(`已核对已配置的 MCP 运行时命令；${o.configChanges??0} 条失效的受支持 Node 路径可更新。保留每条原有参数与独立环境，不新增 MCP。`);break;
    case 'plugins':plugins(o,p,[]);break;
    case 'browser':chrome(o,p);break;
    case 'headers':browserHeaders(o,p);break;
    case 'ax':browserAx(o,p);break;
    case 'context':context(o,p);break;
    case 'surface':surface(o,p);break;
    case 'screenshot':screenshot(o,p);break;
    case 'discovery':case 'desktop':archivePlan(o,p);break;
    case 'proxy':if(o.proxyEvidence===true)archivePlan(o,p);else p.lines.push('尚无“客户端已有代理、子进程丢失代理”的证据，本项没有修改客户端。');break;
    default:throw Error('此修复类别未识别');
  }
  const changed=p.ops.size,packageChanged=Boolean(p.package);let backup=null,packagePlan=null;
  if(o.action==='repair'){
    backup=p.commit();
    if(p.package){const dir=path.join(o.backupRoot,`${Date.now()}-${crypto.randomUUID()}-package`);fs.mkdirSync(dir,{recursive:true,mode:0o700});const prepared=path.join(dir,'prepared.asar');fs.writeFileSync(prepared,p.package.bytes,{flag:'wx',mode:0o600});packagePlan={source:p.package.source,sourceSha256:p.package.before,prepared,preparedSha256:p.package.after,backup:dir,relatedBackup:backup,home:o.home,resources:o.resources,kind:o.kind};fs.writeFileSync(path.join(dir,'package-plan.json'),bytes(packagePlan),{flag:'wx',mode:0o600});backup=dir;}
  }
  const summary=o.action==='check'?`检查完成：发现 ${changed} 个文件或连接需要处理${packageChanged?'，另有客户端安装文件需要修复':''}。检查没有更改用户文件。`:
    changed||packageChanged?'所选修复已准备或应用，请查看下方结果。':'本项没有需要应用的已识别修复，未修改文件。';
  return {message:[summary,...p.lines].join('\n\n'),backup,applied:o.action==='repair'&&(changed>0||packageChanged),canRestore:Boolean(backup),registry:p.registry,packagePlan,needsRepair:changed>0||packageChanged||p.registrationRequired===true,skipped:p.skipped};
}
module.exports={run,Plan,state,same,hash,archiveRead,archiveWrite,axValid};
if(require.main===module){try{console.log(JSON.stringify(run(JSON.parse(fs.readFileSync(0,'utf8').replace(/^\uFEFF/,'')))));}catch(e){console.log(JSON.stringify({error:e.message}));process.exitCode=1;}}
