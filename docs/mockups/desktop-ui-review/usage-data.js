'use strict';

// Deterministic synthetic records only. This file never reads files or services.
(function (root) {
  const HOUR = 3600000, DAY = 24 * HOUR, OFFSET = 8 * HOUR;
  const now = Date.parse('2026-11-07T17:45:00+08:00');
  const start = Math.floor((now + OFFSET) / DAY) * DAY - OFFSET - 59 * DAY;
  const sources = {team:'团队 API',backup:'备用 API',account:'ChatGPT 示例账号'};
  const scopes = {ordinary:'普通接入',workbuddy:'WorkBuddy',tiangong:'天工 Work'};
  const results = {completed:'完成',failed:'失败',cancelled:'已取消',client_disconnected:'客户端断开',incomplete:'未完整结束',running:'进行中'};
  const models = ['gpt-6','claude-sonnet · 示例','gpt-5.6'];
  let seed = 20261107;
  function random() { seed = (seed * 1664525 + 1013904223) >>> 0; return seed / 4294967296; }
  const records = [];
  for (let hour = start; hour < now; hour += HOUR) {
    const localHour = new Date(hour + OFFSET).getUTCHours();
    const dayFactor = [.7,1.2,.85,1.45,.75,1.05,1.3][Math.floor((hour-start)/DAY)%7];
    const demand = Math.round((localHour >= 8 && localHour <= 22 ? 4 + Math.floor(random() * 11) : Math.floor(random() * 3))*dayFactor);
    for (let i = 0; i < demand; i++) {
      const time = hour + Math.floor(random() * HOUR);
      if (time > now) continue;
      const scope = ['ordinary','workbuddy','workbuddy','tiangong'][Math.floor(random() * 4)];
      const source = scope === 'tiangong' ? (random() < .82 ? 'team' : 'backup') : ['team','team','team','backup','account'][Math.floor(random() * 5)];
      const model = source === 'account' ? 'gpt-6' : models[Math.floor(random() * models.length)];
      const roll = random();
      const status = roll < .023 ? 'failed' : roll < .037 ? 'cancelled' : roll < .048 ? 'client_disconnected' : roll < .056 ? 'incomplete' : 'completed';
      const known = status === 'completed' && random() > .045;
      const input = known ? 1200 + Math.floor(random() * 11000) : null;
      const output = known ? 100 + Math.floor(random() * 2200) : null;
      const stream = random() < .8;
      const ttft = status === 'completed' && stream ? Math.floor(380 + random() * 2000 + (localHour === 14 ? random() * 1800 : 0)) : null;
      records.push({id:'DEMO-' + (10000 + records.length),time,scope,source,model,status,input,output,
        tokens:known ? input + output : null,cache:known ? Math.floor(input * random() * .7) : null,
        ttft,latency:Math.max(ttft || 0,600)+Math.floor(random() * (status === 'completed' ? 24000 : 3400)),stream,
        upstreamStream:status === 'failed' ? null : stream,entry:scope === 'ordinary' ? null : (model === 'gpt-6' ? 'model-a' : 'model-b')});
    }
  }
  records.sort((a,b) => a.time - b.time);
  records.push({id:'DEMO-LIVE',time:now-1000,scope:'workbuddy',source:'team',model:'gpt-6',status:'running',input:null,output:null,tokens:null,cache:null,ttft:null,latency:null,stream:true,upstreamStream:null,entry:'model-a'});

  function range(preset) {
    if (preset === 'today') return {from:Math.floor((now + OFFSET) / DAY) * DAY - OFFSET,to:now,step:HOUR};
    return {from:now-(preset === '30d' ? 30 : 7)*DAY,to:now,step:DAY};
  }
  function select(filter = {}) {
    const bounds = range(filter.range || '7d');
    const from = filter.from ?? bounds.from, to = filter.to ?? bounds.to;
    return records.filter(r => r.time >= from && r.time < to &&
      (!filter.scope || filter.scope === 'all' || filter.scope === r.scope) &&
      (!filter.source || filter.source === 'all' || filter.source === r.source) &&
      (!filter.model || filter.model === 'all' || filter.model === r.model) &&
      (!filter.status || filter.status === 'all' || filter.status === r.status));
  }
  function summary(rows) {
    const terminal = rows.filter(r=>r.status !== 'running');
    const usage = rows.filter(r=>Number.isFinite(r.tokens));
    const completed = terminal.filter(r=>r.status === 'completed').length;
    return {requests:rows.length,terminal:terminal.length,completed,
      failed:rows.filter(r=>r.status === 'failed').length,
      cancelled:rows.filter(r=>r.status === 'cancelled').length,
      disconnected:rows.filter(r=>r.status === 'client_disconnected').length,
      incomplete:rows.filter(r=>r.status === 'incomplete').length,
      running:rows.length-terminal.length,
      tokens:usage.length ? usage.reduce((sum,r)=>sum+r.tokens,0) : null,
      input:usage.length ? usage.reduce((sum,r)=>sum+r.input,0) : null,
      output:usage.length ? usage.reduce((sum,r)=>sum+r.output,0) : null,
      coverage:rows.length ? usage.length / rows.length : null,usageSamples:usage.length,
      completion:terminal.length ? completed / terminal.length : null};
  }
  function series(filter = {}) {
    const bounds = range(filter.range || '7d');
    const groups = new Map();
    const first = Math.floor((bounds.from+OFFSET)/bounds.step)*bounds.step-OFFSET;
    for (let time=first;time<bounds.to;time+=bounds.step) groups.set(time,[]);
    for (const r of select(filter)) {
      const time = Math.floor((r.time+OFFSET)/bounds.step)*bounds.step-OFFSET;
      if (groups.has(time)) groups.get(time).push(r);
    }
    return [...groups].map(([time,rows])=>({time,from:Math.max(time,bounds.from),to:Math.min(time+bounds.step,bounds.to),...summary(rows)}));
  }
  function breakdown(rows,key) {
    const groups = new Map();
    rows.forEach(r=>{if(!groups.has(r[key]))groups.set(r[key],[]);groups.get(r[key]).push(r);});
    return [...groups].map(([key,value])=>({key,...summary(value)})).sort((a,b)=>(b.tokens||0)-(a.tokens||0));
  }
  function date(time, mode='day') {
    const d = new Date(time + OFFSET), pad = n=>String(n).padStart(2,'0');
    if(mode === 'full')return `${pad(d.getUTCMonth()+1)}-${pad(d.getUTCDate())} ${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}`;
    if(mode === 'hour')return pad(d.getUTCHours())+':00';
    return `${pad(d.getUTCMonth()+1)}-${pad(d.getUTCDate())}`;
  }
  const api = {now,start,records,sources,scopes,results,models,range,select,summary,series,breakdown,date};
  root.ReviewUsage = api;
  if(typeof module !== 'undefined' && module.exports)module.exports=api;
})(typeof globalThis !== 'undefined' ? globalThis : window);
