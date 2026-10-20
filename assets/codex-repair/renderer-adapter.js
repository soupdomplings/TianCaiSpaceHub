// Pure source adapter used through CDP. No filesystem, network or page writes.
// Each feature validates its own structure and leaves unknown layouts intact.
(function adaptHubDesktopSource(source) {
  const guard='globalThis.__CODEXHUB_ENHANCED_MODE__?.modelControlsEnabled===true';
  const result={source,power:false,ultraPreference:false,goal:false,pluginUi:false,browserUi:false,computerUi:false,locale:false};
  const unique=(re,replace)=>{
    const matches=[...result.source.matchAll(re)];if(matches.length!==1)return false;
    const next=replace(...matches[0]);if(typeof next!=='string')return false;
    result.source=result.source.slice(0,matches[0].index)+next+result.source.slice(matches[0].index+matches[0][0].length);return true;
  };
  if(!source.includes('CODEXHUB_POWER_V1'))result.power=unique(/function ([\w$]+)\(\{harborEnabled:([\w$]+),isElectron:([\w$]+),isEverydayWorkMode:([\w$]+)\}\)\{return \4\|\|\3&&\2\}/g,
    (m,fn,harbor,electron,work)=>`function ${fn}({harborEnabled:${harbor},isElectron:${electron},isEverydayWorkMode:${work}}){return ${work}||${electron}&&(${harbor}||(${guard}))/*CODEXHUB_POWER_V1*/}`);
  if(!source.includes('CODEXHUB_GOAL_V1'))result.goal=unique(/([\w$]+)=([\w$]+)\(`3074100722`\)&&([\w$]+)\(([\w$]+)\?\.config,`goals`\)===!0&&([\w$]+)!==`cloud`(&&![\w$]+)?(?=,)/g,
    (m,value,gate,read,config,mode,side='')=>`${value}=((${gate}(\`3074100722\`)&&${read}(${config}?.config,\`goals\`)===!0)||(${guard}))&&${mode}!==\`cloud\`${side}/*CODEXHUB_GOAL_V1*/`);
  if(source.includes('openPluginInstall')&&!source.includes('CODEXHUB_PLUGIN_UI_V1'))result.pluginUi=unique(/\{authMethod:([\w$]+)\}=([\w$]+)\(\),((?:\{data:[\w$]+\}=[\w$]+\(\),)?)([\w$]+)=([\w$]+)\(\1\),/g,
    (m,auth,hook,middle,blocked,isBlocked)=>`{authMethod:${auth}}=${hook}(),${middle}${blocked}=${isBlocked}(${auth})&&!(${guard})/*CODEXHUB_PLUGIN_UI_V1*/,`);
  if(source.includes('openPluginInstall')&&!result.pluginUi&&!source.includes('CODEXHUB_PLUGIN_UI_V1'))result.pluginUi=unique(/\{authMethod:([\w$]+)\}=([\w$]+)\(\),([^;]{0,2500}?)([\w$]+)=([\w$]+)\(\1\),([\w$]+=[\w$]+\.kind===`manage`,)/g,
    (m,auth,hook,middle,blocked,isBlocked,tail)=>`{authMethod:${auth}}=${hook}(),${middle}${blocked}=${isBlocked}(${auth})&&!(${guard})/*CODEXHUB_PLUGIN_UI_V1*/,${tail}`);
  if(source.includes('locale_source')&&!source.includes('CODEXHUB_LOCALE_V1'))result.locale=unique(/([\w$]+=)([\w$]+)\?\.get\(`enable_i18n`,!1\)/g,
    (m,assign,layer)=>`${assign}(${layer}?.get(\`enable_i18n\`,!1)||(${guard}))/*CODEXHUB_LOCALE_V1*/`);
  if(source.includes('533078438')&&!source.includes('CODEXHUB_PLUGIN_SIDEBAR_V1')){
    result.pluginUi=unique(/\{authMethod:([\w$]+)\}=([\w$]+)\(\),([\w$]+)=([\w$]+)\(`533078438`\),([\w$]+)=([\w$]+)\(\1\),([\w$]+)=e&&\3&&\5,([\w$]+)=([\w$]+)\(\{hostId:([\w$]+)\}\),([\w$]+)=e&&\8&&!\5,/g,
      (m,auth,authHook,flag,gate,blocked,isBlocked,disabled,available,availability,host,enabled)=>`{authMethod:${auth}}=${authHook}(),${flag}=${gate}(\`533078438\`),${blocked}=${isBlocked}(${auth})&&!(${guard}),${disabled}=e&&${flag}&&${blocked},${available}=${availability}({hostId:${host}}),${enabled}=e&&${available}&&!${blocked},/*CODEXHUB_PLUGIN_SIDEBAR_V1*/`)||result.pluginUi;
  }
  if(source.includes('featureName:`computer_use`')||source.includes('showComputerUseSetup')){
    result.computerUi=unique(/([\w$]+)=([\w$]+)\(`1506311413`\)(?=,|;)/g,
      (m,v,fn)=>`${v}=(${fn}(\`1506311413\`)||(${guard}))/*CODEXHUB_COMPUTER_UI_V1*/`);
  }
  // Preserve configuration, loading and host checks. Only the rollout gates
  // are compatible here; main-process/runtime repair is a separate operation.
  if(source.includes('featureName:`browser_use`')&&source.includes('featureName:`browser_use_external`')&&!source.includes('CODEXHUB_BROWSER_UI_V1')) {
    for(const id of ['410262010','410065390']) {
      if(unique(new RegExp(`([\\w$]+)=([\\w$]+)\\(\x60${id}\x60\\)(?=,)`,'g'),
        (m,v,fn)=>`${v}=(${fn}(\`${id}\`)||(${guard}))/*CODEXHUB_BROWSER_UI_V1*/`))result.browserUi=true;
    }
  }
  // Keep the normal ChatGPT account path. Only a failed account-settings query
  // in a Hub-managed local session uses the existing local preference API.
  function ultraFallback(text) {
    if(text.includes('CODEXHUB_ULTRA_PREF_V1'))return null;
    const read=[...text.matchAll(/([\w$]+)\(([\w$]+)\.showUltraInModelPickerSlider\)\.catch\(\(\)=>!1\)/g)];
    const write=[...text.matchAll(/await ([\w$]+)\(([\w$]+),([\w$]+)\.showUltraInModelPickerSlider,!1\)\.catch\(\(\)=>\{\}\)/g)];
    if(read.length!==1||write.length!==1||read[0][2]!==write[0][3])return null;
    const readFn=read[0][1],settings=read[0][2],writeFn=write[0][1];
    const mutation=/async function ([\w$]+)\(([\w$]+),([\w$]+)\)\{let ([\w$]+)=\2\.query\.snapshot\(([\w$]+)\),([\w$]+)=\4\.getData\(\);\4\.setData\(([\w$]+)=>\7==null\?\7:\{\.\.\.\7,ultraEffortEnabled:\3\}\);try\{await \2\.get\(([\w$]+)\)\.setUltraEffortEnabled\(\3\),await Promise\.all\(\[\4\.invalidate\(\),\2\.query\.snapshot\(([\w$]+)\)\.invalidate\(\)\]\)\}catch\(([\w$]+)\)\{throw \4\.setData\(\6\),\10\}\}/g;
    const mutations=[...text.matchAll(mutation)];if(mutations.length!==1)return null;
    const [,fn,scope,enabled,snapshot,query,previous,cached,client,tpp]=mutations[0];
    let next=text.replace(mutation,()=>`async function ${fn}(${scope},${enabled}){let ${snapshot}=${scope}.query.snapshot(${query}),${previous}=${snapshot}.getData();${snapshot}.setData(${cached}=>${cached}==null?${cached}:{...${cached},ultraEffortEnabled:${enabled}});if(${previous}?.ultraEffortLocalFallback===!0&&(${guard})){try{await ${writeFn}(${scope},${settings}.showUltraInModelPickerSlider,${enabled});await ${snapshot}.invalidate();return}catch(error){throw ${snapshot}.setData(${previous}),error}}try{await ${scope}.get(${client}).setUltraEffortEnabled(${enabled}),await Promise.all([${snapshot}.invalidate(),${scope}.query.snapshot(${tpp}).invalidate()])}catch(error){throw ${snapshot}.setData(${previous}),error}}`);
    // Match a complete query body with a lexical brace scanner. Quotes,
    // comments and templates cannot accidentally terminate a source fragment.
    function bodyEnd(start){let depth=1,quote=null,escaped=false,line=false,block=false;
      for(let i=start;i<next.length;i++){const c=next[i],n=next[i+1];
        if(line){if(c==='\n')line=false;continue;}if(block){if(c==='*'&&n==='/'){block=false;i++;}continue;}
        if(quote){if(escaped){escaped=false;continue;}if(c==='\\'){escaped=true;continue;}if(c===quote)quote=null;continue;}
        if(c==='"'||c==="'"||c==='`'){quote=c;continue;}if(c==='/'&&n==='/'){line=true;i++;continue;}if(c==='/'&&n==='*'){block=true;i++;continue;}
        if(c==='{')depth++;else if(c==='}'&&--depth===0)return i;
      }return -1;
    }
    const queries=[];let probe=0;
    for(;;){const anchor=next.indexOf('.userSettings());',probe);if(anchor<0)break;probe=anchor+17;
      const start=next.lastIndexOf('queryFn:async()=>{',anchor);if(start<0||anchor-start>4000)continue;
      const body=start+'queryFn:async()=>{'.length,end=bodyEnd(body);if(end<0)continue;
      const success=next.slice(body,end);if(!success.includes('model_picker_persists_ultra_effort'))continue;
      queries.push({start,end,success});
    }
    if(queries.length!==1)return null;
    const q=queries[0];
    next=next.slice(0,q.start)+`queryFn:async()=>{try{${q.success}}catch(error){if(!(${guard}))throw error;return{lockdownModeEnabled:!1,ultraEffortEnabled:await ${readFn}(${settings}.showUltraInModelPickerSlider).catch(()=>!1)===!0,ultraEffortLocalFallback:!0/*CODEXHUB_ULTRA_PREF_V1*/}}}`+next.slice(q.end+1);
    const key='queryKey:[`chatgpt-ultra-effort-migration`]';const at=next.indexOf(key);
    if(at<0||next.indexOf(key,at+key.length)>=0)return null;
    const from=Math.max(0,at-1200),segment=next.slice(from,at),old='.setUltraEffortEnabled(!0).catch(()=>{})';
    if(segment.split(old).length!==2)return null;
    // Preserve a local preference when the migration's remote write fails.
    next=next.slice(0,from)+segment.replace(old,`.setUltraEffortEnabled(!0).catch(error=>{if(${guard})throw error})/*CODEXHUB_ULTRA_MIGRATION_V1*/`)+next.slice(at);
    return next;
  }
  if(source.includes('showUltraInModelPickerSlider')&&source.includes('model_picker_persists_ultra_effort')){
    const next=ultraFallback(result.source);if(next){result.source=next;result.ultraPreference=true;}
  }
  if(Object.entries(result).some(([k,v])=>k!=='source'&&v===true)){
    const flags=Object.fromEntries(Object.entries(result).filter(([k,v])=>k!=='source'&&v===true));
    result.source+='\n;globalThis.__CODEXHUB_MODEL_CONTROLS_SOURCE__=Object.assign(globalThis.__CODEXHUB_MODEL_CONTROLS_SOURCE__??{},{version:2},'+JSON.stringify(flags)+');\n';
  }
  return result;
})
