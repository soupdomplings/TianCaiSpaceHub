'use strict';
// Run the installed CLI's local-plugin installer in a disposable home. Only
// verified cache output is imported by Plan; auth, logs, databases and generated
// configuration from this home never replace the user's files.
const fs=require('node:fs'),os=require('node:os'),path=require('node:path');
const {spawnSync}=require('node:child_process');
function stage(options,names,consume,runner=spawnSync) {
  if(!names.length)return consume(new Map());
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'hub-plugin-registration-'));
  try {
    const cli=path.join(root,process.platform==='win32'?'codex.exe':'codex');
    fs.copyFileSync(options.cli,cli);if(process.platform!=='win32')fs.chmodSync(cli,0o700);
    const home=path.join(root,'home');fs.mkdirSync(home,{mode:0o700});
    // JSON basic strings are also valid TOML strings for these paths/keys.
    fs.writeFileSync(path.join(home,'config.toml'),'[marketplaces.openai-bundled]\nsource_type = "local"\nsource = '+JSON.stringify(options.marketplace)+'\n',{mode:0o600});
    const env={...process.env,CODEX_HOME:home};
    for(const key of ['OPENAI_API_KEY','CODEX_API_KEY','OPENAI_ACCESS_TOKEN'])delete env[key];
    const invoke=args=>{
      const out=runner(cli,args,{cwd:root,env,encoding:'utf8',timeout:60000,maxBuffer:4*1024*1024,windowsHide:true});
      if(out.error||out.status!==0)throw Error('当前客户端未能完成本地插件注册校验，尚未改动用户文件。请检查客户端插件管理是否可用，再重新检查本项。');
      try{return JSON.parse(out.stdout.replace(/^\uFEFF/,''));}catch{throw Error('客户端的插件注册结果格式未识别，尚未改动用户文件。');}
    };
    const result=new Map();
    for(const name of names) {
      if(!/^[\w-]+$/.test(name))throw Error('插件名称无效');
      const installed=invoke(['plugin','add',name+'@openai-bundled','--json']);
      const source=installed.installedPath,version=installed.version;
      if(installed.pluginId!==name+'@openai-bundled'||typeof version!=='string'||!/^[\w.+-]+$/.test(version)||['.','..'].includes(version)||
          typeof source!=='string'||path.resolve(source)!==path.join(home,'plugins','cache','openai-bundled',name,version)||!fs.existsSync(path.join(source,'.codex-plugin','plugin.json')))
        throw Error('客户端的插件安装位置或描述符未识别，尚未改动用户文件。');
      result.set(name,{source,version});
    }
    const listed=invoke(['plugin','list','--marketplace','openai-bundled','--json']);
    if(!Array.isArray(listed.installed)||names.some(name=>!listed.installed.some(p=>p.pluginId===name+'@openai-bundled'&&p.installed===true)))
      throw Error('客户端未确认插件安装成功，尚未改动用户文件。');
    return consume(result);
  } finally {fs.rmSync(root,{recursive:true,force:true});}
}
module.exports={stage};
