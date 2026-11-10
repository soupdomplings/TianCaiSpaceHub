// Independent SVG diagrams, never HTML captures. Only synthetic usage-data.js is read.
// node render-workspace-layouts.cjs --sharp <path-to-sharp-package>
'use strict';
const fs=require('node:fs'),path=require('node:path');
const U=require('./usage-data.js');
let sharp;
try{sharp=require('sharp');}catch(error){
  const i=process.argv.indexOf('--sharp');
  if(i<0||!process.argv[i+1])throw new Error('Provide an installed sharp package with --sharp. No packages are downloaded.');
  sharp=require(path.resolve(process.argv[i+1]));
}
const palettes={
  light:{bg:'#f4f6f8',surface:'#ffffff',soft:'#f8fafb',ink:'#172b36',muted:'#586b78',subtle:'#71828e',line:'#e2e8ed',accent:'#087f83',accentSoft:'#e8f5f3',on:'#ffffff',secondary:'#8780ce',ok:'#207457',warn:'#94600d',error:'#b34147'},
  dark:{bg:'#111a22',surface:'#19252f',soft:'#1d2c37',ink:'#e5edf2',muted:'#a3b3bf',subtle:'#91a5b3',line:'#30424f',accent:'#50c7c7',accentSoft:'#173d40',on:'#102a2e',secondary:'#b4a4f3',ok:'#8dd4af',warn:'#f0c678',error:'#ff9fa3'}
};
const esc=s=>String(s).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&apos;'}[c]));
const num=n=>n===null?'—':n.toLocaleString('en-US',{maximumFractionDigits:0});
const compact=n=>n===null?'—':n>=1e6?(n/1e6).toFixed(2)+' M':n>=1e3?(n/1e3).toFixed(1)+' K':num(n);
class Diagram{
  constructor(w,h,theme='light'){this.w=w;this.h=h;this.p=palettes[theme];this.items=[];}
  rect(x,y,w,h,fill=this.p.surface,r=14,stroke=null){this.items.push(`<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="${r}" fill="${fill}"${stroke?` stroke="${stroke}"`:''}/>`);}
  line(x,y,x2,y2,color=this.p.line){this.items.push(`<path d="M${x} ${y}L${x2} ${y2}" fill="none" stroke="${color}"/>`);}
  text(x,y,text,size=13,fill=this.p.ink,weight=400,anchor='start'){this.items.push(`<text x="${x}" y="${y}" font-size="${size}" font-weight="${weight}" fill="${fill}" text-anchor="${anchor}">${esc(text)}</text>`);}
  circle(x,y,r,fill){this.items.push(`<circle cx="${x}" cy="${y}" r="${r}" fill="${fill}"/>`);}
  button(x,y,w,label,primary=false){this.rect(x,y,w,33,primary?this.p.accent:this.p.surface,8,primary?null:this.p.line);this.text(x+w/2,y+21,label,12,primary?this.p.on:this.p.ink,500,'middle');}
  icon(x,y,symbol='◇'){this.rect(x,y,38,38,this.p.soft,8);this.text(x+19,y+26,symbol,22,this.p.accent,500,'middle');}
  chip(x,y,w,label){this.rect(x,y,w,26,this.p.soft,4);this.text(x+10,y+17,label,11,this.p.muted);}
  raw(svg){this.items.push(svg);}
  svg(){return `<svg xmlns="http://www.w3.org/2000/svg" width="${this.w}" height="${this.h}" viewBox="0 0 ${this.w} ${this.h}"><title>TianCaiSpaceHub 第三稿 · 独立合成布局图，非浏览器截图</title><style>text{font-family:'Microsoft YaHei UI','Segoe UI',sans-serif}path{stroke-linejoin:round;stroke-linecap:round}</style>${this.items.join('\n')}</svg>`;}
}
function shell(d,active='概览'){
  const p=d.p,sidebar=d.w<1000?168:204,x=sidebar+26,cw=d.w-x-26;
  d.rect(0,0,d.w,d.h,p.bg,0,null);d.rect(0,0,d.w,30,p.ink,0,null);
  d.text(16,20,'界面方案 03  ·  合成数据',11,p.surface,600);d.text(d.w-20,20,'日常使用   ⌄       深色预览       重置演示',11,p.surface,400,'end');
  d.rect(0,30,sidebar,d.h-30,p.surface,0,null);d.line(sidebar,30,sidebar,d.h);
  d.rect(22,55,34,34,p.accent,8,null);d.text(39,80,'T',23,p.on,700,'middle');d.text(67,70,'天才空间',16,p.ink,600);d.text(67,87,'TianCaiSpaceHub',10,p.muted);
  const nav=(label,y,symbol)=>{if(label===active)d.rect(12,y-24,sidebar-24,39,p.accentSoft,6,null);d.text(27,y,symbol,17,label===active?p.accent:p.muted);d.text(54,y,label,d.w<1000?12:13,label===active?p.accent:p.muted,label===active?600:400);};
  nav('概览',135,'⌁');d.text(25,184,'客户端',11,p.subtle);
  nav('ChatGPT(Codex)',222,'◉');nav('WorkBuddy',265,'W');nav('天工 Work',308,'T');
  d.text(sidebar/2,347,'⋯',20,p.muted,500,'middle');d.text(25,395,'共用能力',11,p.subtle);
  nav('模型与渠道',433,'◇');nav('Skill 管理',476,'▤');nav('MCP 管理',519,'♧');nav('聊天接入',562,'◌');nav('请求记录',605,'⌁');
  nav('设置',d.h-128,'⚙');nav('帮助',d.h-88,'?');d.text(24,d.h-42,'界面评审稿',10,p.muted);d.text(24,d.h-24,'基线 v0.4.30-13',10,p.subtle);
  d.rect(sidebar,30,d.w-sidebar,48,p.surface,0,null);d.line(sidebar,78,d.w,78);d.text(x,59,'工作台 / '+active,12,p.muted);
  d.circle(d.w-169,54,3,p.ok);d.text(d.w-157,59,'本机服务运行中   ›',12,p.muted);
  d.rect(sidebar,d.h-27,d.w-sidebar,27,p.surface,0,null);d.line(sidebar,d.h-27,d.w,d.h-27);
  d.text(x,d.h-10,'独立布局图 · 非 HTML / 产品运行截图 · 仅供界面评审',10,p.muted);return{x,cw,sidebar};
}
function tabs(d,x,y,w,labels,index=0){const p=d.p;d.line(x,y+13,x+w,y+13);let cursor=x;
  labels.forEach((label,i)=>{d.text(cursor,y,label,13,i===index?p.accent:p.muted,i===index?600:400);const tw=label.length*13;if(i===index)d.rect(cursor,y+12,tw,2,p.accent,0,null);cursor+=tw+30;});}
function switcher(d,x,y,labels,index=0,width=72){const p=d.p;d.rect(x,y,labels.length*width+6,34,p.soft,6);labels.forEach((label,i)=>{if(i===index)d.rect(x+3+i*width,y+3,width,28,p.surface,4);d.text(x+3+i*width+width/2,y+22,label,12,i===index?p.ink:p.muted,500,'middle');});}
function filters(d,x,y,w,compactMode){switcher(d,x,y,['今天','近 7 天','近 30 天'],1,70);const vals=['全部接入点  ⌄','全部来源  ⌄','全部模型  ⌄'];let sx=compactMode?x:x+w-435;
  const sy=compactMode?y+48:y;vals.forEach((v,i)=>{d.rect(sx+i*147,sy,137,33,d.p.surface,6);d.text(sx+10+i*147,sy+21,v,12,d.p.muted);});}
function metric(d,x,y,w,h,label,value){const p=d.p;d.text(x+20,y+24,label,12,p.muted);d.text(x+20,y+60,value,30,p.ink,600);}
function graph(d,x,y,w,h){
  const p=d.p,series=U.series({range:'7d'}),left=54,right=16,top=16,bottom=28,pw=w-left-right,ph=h-top-bottom,max=Math.max(...series.flatMap(s=>[s.input,s.output]))*1.15;
  const xx=i=>x+left+pw*i/(series.length-1),yy=v=>y+top+ph*(1-v/max);
  d.text(x+left,y+4,'Token',10,p.muted);
  for(let i=0;i<=3;i++){const v=max*i/3;d.line(x+left,yy(v),x+w-right,yy(v));d.text(x+left-10,yy(v)+4,compact(v),10,p.muted,400,'end');}
  ['input','output'].forEach((key,j)=>{const color=j?p.secondary:p.accent;d.raw(`<path d="${series.map((s,i)=>(i?'L':'M')+xx(i).toFixed(1)+' '+yy(s[key]).toFixed(1)).join(' ')}" fill="none" stroke="${color}" stroke-width="2.4"/>`);series.forEach((s,i)=>d.circle(xx(i),yy(s[key]),3,color));});
  series.forEach((s,i)=>{if(w<650&&i%2&&i!==series.length-1)return;d.text(xx(i),y+h-7,U.date(s.time),10,p.muted,400,i===0?'start':i===series.length-1?'end':'middle');});
}
function ranks(d,x,y,w,title,rows,total,scope=false){const p=d.p;d.rect(x,y,w,211);d.text(x+20,y+27,title,14,p.ink,600);
  rows.slice(0,3).forEach((r,i)=>{const ry=y+61+i*48;d.text(x+20,ry,String(i+1).padStart(2,'0'),10,p.subtle);d.text(x+49,ry,scope?U.scopes[r.key]:r.key,12);d.text(x+w-20,ry,compact(r.tokens),12,p.ink,500,'end');d.rect(x+49,ry+8,w-69,4,p.soft,2,null);d.rect(x+49,ry+8,(w-69)*(r.tokens||0)/total,4,p.accent,2,null);d.text(x+49,ry+27,num(r.requests)+' 次请求',10,p.muted);});
}
function usage(theme='light',compactMode=false){
  const d=new Diagram(compactMode?900:1366,compactMode?1460:1068,theme),{x,cw}=shell(d),p=d.p;
  const rows=U.select({range:'7d'}),s=U.summary(rows);
  d.text(x,126,'概览',24,p.ink,600);d.button(x+cw-175,109,133,'查看请求记录  →');d.circle(x+cw-16,124,9,p.muted);d.text(x+cw-16,129,'?',13,p.surface,600,'middle');
  tabs(d,x,171,cw,['运行状态','使用统计'],1);filters(d,x,199,cw,compactMode);
  const my=compactMode?294:249,mh=compactMode?168:84,cols=compactMode?2:4,mw=cw/cols;
  const metrics=[['请求数',num(s.requests)],['输入用量',compact(s.input)],['输出用量',compact(s.output)],['请求完成率',(s.completion*100).toFixed(1)+'%']];
  metrics.forEach((m,i)=>{const col=i%cols,row=Math.floor(i/cols);if(col)d.line(x+col*mw,my+row*84,x+col*mw,my+(row+1)*84);metric(d,x+col*mw,my+row*84,mw,84,...m);});if(compactMode)d.line(x,my+84,x+cw,my+84);
  const gy=my+mh+14;d.rect(x,gy,cw,276);d.text(x+20,gy+28,'使用趋势',14,p.ink,600);switcher(d,x+cw-182,gy+16,['Token','请求'],0,78);
  graph(d,x+17,gy+53,cw-34,181);d.rect(x+20,gy+253,13,3,p.accent,0,null);d.text(x+40,gy+258,'输入',11,p.muted);d.rect(x+100,gy+253,13,3,p.secondary,0,null);d.text(x+120,gy+258,'输出',11,p.muted);
  const by=gy+292,models=U.breakdown(rows,'model'),scopes=U.breakdown(rows,'scope');
  if(compactMode){ranks(d,x,by,cw,'模型用量',models,s.tokens);ranks(d,x,by+227,cw,'接入点分布',scopes,s.tokens,true);}else{ranks(d,x,by,(cw-16)/2,'模型用量',models,s.tokens);ranks(d,x+(cw+16)/2,by,(cw-16)/2,'接入点分布',scopes,s.tokens,true);}
  const ry=by+(compactMode?454:227);d.rect(x,ry,cw,79);d.text(x+20,ry+26,'请求结果',14,p.ink,600);const results=[['完成',s.completed,p.ok],['失败',s.failed,p.error],['未完整结束',s.incomplete,p.warn],['已取消',s.cancelled,p.subtle],['客户端断开',s.disconnected,p.subtle]];
  results.forEach(([label,count,color],i)=>{const xx=x+20+i*(cw-30)/5;d.circle(xx+3,ry+53,3,color);d.text(xx+15,ry+57,label+'  '+count+'  ›',compactMode?10:12,p.muted);});
  return d;
}
function modelLibrary(update=false){
  const d=new Diagram(1366,940),{x,cw}=shell(d,'模型与渠道'),p=d.p;
  d.text(x,126,'模型与渠道',24,p.ink,600);d.button(x+cw-182,109,140,'+ 添加模型来源',true);d.circle(x+cw-16,124,9,p.muted);d.text(x+cw-16,129,'?',13,p.surface,600,'middle');
  tabs(d,x,187,cw,['我的模型','账号与渠道']);let py=220;
  if(update){d.rect(x,py,cw,58,p.accentSoft,7);d.text(x+18,py+34,'团队 API 有更新，客户端副本保持原样',13,p.ink,600);d.button(x+cw-112,py+13,94,'查看差异');py+=74;}
  d.rect(x,py,cw,346);d.rect(x+20,py+16,345,34,p.soft,6);d.text(x+34,py+38,'⌕  搜索模型或来源',12,p.muted);d.text(x+cw-20,py+38,'3 个模型',11,p.muted,400,'end');d.line(x+20,py+64,x+cw-20,py+64);
  [['GPT-6','团队 API · 日常编程'],['Claude Sonnet · 示例','备用 API · 文档与复杂任务'],['GPT-6 · 账号来源','ChatGPT 示例账号']].forEach(([name,desc],i)=>{
    const y=py+83+i*84;d.icon(x+20,y);d.text(x+74,y+14,name,14,p.ink,600);d.text(x+74,y+36,desc,12,p.muted);d.button(x+cw-203,y+3,96,'模型设置');d.button(x+cw-98,y+3,80,'查看来源');if(i<2)d.line(x+20,y+64,x+cw-20,y+64);
  });return d;
}
function clientFrame(key='workbuddy',theme='light',compactMode=false,tab=0){
  const d=new Diagram(compactMode?900:1366,1068,theme),name={codex:'ChatGPT(Codex)',workbuddy:'WorkBuddy',tiangong:'天工 Work'}[key],frame=shell(d,name),{x,cw}=frame,p=d.p;
  d.text(x,126,name,24,p.ink,600);
  const sx=x+(key==='codex'?243:key==='workbuddy'?183:163);d.circle(sx,119,3,key==='workbuddy'?p.accent:p.ok);d.text(sx+10,124,key==='workbuddy'?'1 个请求处理中':key==='codex'?'已打开':'已打开 · 空闲',compactMode?11:12,key==='workbuddy'?p.accent:p.ok);
  d.button(x+cw-151,107,112,'打开客户端',true);d.circle(x+cw-16,124,9,p.muted);d.text(x+cw-16,129,'?',13,p.surface,600,'middle');
  tabs(d,x,187,cw,key==='codex'?['状态概览','模型','接入设置','Skill 与 MCP','历史与维护']:['状态概览','模型','Skill 与 MCP','维护'],tab);
  return {d,...frame};
}
function clientModelsLayout(){
  const {d,x,cw}=clientFrame('workbuddy','light',false,1),p=d.p;
  d.rect(x,220,cw,315);d.text(x+22,253,'使用中的模型 / 2',16,p.ink,600);d.button(x+cw-180,235,158,'+ 从我的模型加入',true);
  [['GPT-6','团队 API · 思考 high · 输出 8,192'],['Claude Sonnet · 示例','备用 API · 思考 high · 输出 8,192']].forEach(([name,desc],i)=>{
    const y=294+i*83;d.icon(x+22,y);d.text(x+77,y+15,name,14,p.ink,600);d.text(x+77,y+38,desc,12,p.muted);d.button(x+cw-240,y+3,92,'调整参数');d.button(x+cw-138,y+3,116,'用于其他客户端');d.line(x+22,y+63,x+cw-22,y+63);
  });d.text(x+22,510,'手动配置模型',12,p.accent);return d;
}
function clientOverviewLayout(key='workbuddy',theme='light',compactMode=false){
  const {d,x,cw}=clientFrame(key,theme,compactMode),p=d.p;
  d.text(x+8,249,'状态概览',16,p.ink,600);if(key==='workbuddy')d.text(x+cw-8,249,'进行中请求（1）  ›',12,p.accent,500,'end');
  const y=270,h=440,xx=q=>x+cw*q,yy=q=>y+h*q;
  const line=(path,active=false,config=false)=>d.raw(`<path d="${path}" fill="none" stroke="${active?p.accent:p.line}" stroke-width="${active?2:1.5}"${active||config?' stroke-dasharray="4 9"':''}/>`);
  const a=xx(.16),b=xx(.5),c=xx(.84),top=yy(.26),bottom=yy(.75);
  line(`M${a+33} ${top} H${b-33}`,key==='workbuddy');line(`M${b+33} ${top} H${c-33}`,key==='workbuddy');
  line(`M${a-33} ${top} C${a-85} ${top} ${a-85} ${bottom} ${a-33} ${bottom}`,false,true);
  line(`M${a+33} ${top} C${a+190} ${top} ${b-150} ${bottom} ${b-33} ${bottom}`,false,true);
  line(`M${b+33} ${top} C${b+190} ${top} ${c-150} ${bottom} ${c-33} ${bottom}`,false,true);
  const node=(cx,cy,symbol,label,value,tone='muted',hub=false)=>{
    d.rect(cx-30,cy-30,60,60,hub?p.accent:p.surface,hub?22:21,hub?null:p.line);d.text(cx,cy+9,symbol,hub?26:24,hub?p.on:p.accent,600,'middle');
    d.circle(cx+27,cy+25,5,p.surface);d.circle(cx+27,cy+25,3.5,tone==='ok'?p.ok:tone==='active'?p.accent:p.subtle);
    d.text(cx,cy+57,label,13,p.ink,600,'middle');d.text(cx,cy+81,value,compactMode?10:11,p.muted,400,'middle');
  };
  const short={workbuddy:'WorkBuddy',codex:'Codex',tiangong:'天工 Work'}[key];
  node(a,top,key==='workbuddy'?'W':key==='codex'?'C':'T',short,'已打开','ok');node(b,top,'T','天才 Hub','运行中','ok',true);
  node(c,top,'◇','模型',key==='workbuddy'?'1 个请求进行中':key==='codex'?'2 个使用中':'1 个使用中',key==='workbuddy'?'active':'ok');
  node(a,bottom,'▤','Skill',key==='tiangong'?'待适配':'1 项使用中',key==='tiangong'?'muted':'ok');node(b,bottom,'♧','MCP',key==='codex'?'2 项使用中':'1 项使用中','ok');
  node(c,bottom,'◌','聊天与执行',key==='codex'?'App 已连接':key==='workbuddy'?'暂未接通':'待授权',key==='codex'?'ok':'muted');
  d.line(x,y+h,x+cw,y+h);return d;
}
function capabilityUseLayout(kind='skills',theme='light'){
  const d=new Diagram(1366,1068,theme),p=d.p,{x,cw}=shell(d,kind==='skills'?'Skill 管理':'MCP 管理');
  const title=kind==='skills'?'Skill 管理':'MCP 管理';d.text(x,126,title,24,p.ink,600);d.button(x+cw-169,108,128,kind==='skills'?'+ 添加 Skill':'+ 添加 MCP',true);d.circle(x+cw-16,124,9,p.muted);d.text(x+cw-16,129,'?',13,p.surface,600,'middle');
  if(kind==='mcp')tabs(d,x,187,cw,['工具服务','Dumpling-MCP']);
  const py=kind==='skills'?178:220;d.rect(x,py,cw,480,p.surface,13);d.rect(x+22,py+18,330,34,p.soft,6);d.text(x+35,py+40,'⌕  搜索名称或来源',12,p.muted);d.text(x+cw-22,py+40,'3 项',11,p.muted,400,'end');
  const values=kind==='skills'?[['周报整理','把本周进展整理为清晰的周报。',[true,false,null]],['资料研究','检索知识库，形成带来源的研究摘要。',[false,false,null]],['代码审查','检查改动，整理可执行的审查意见。',[false,true,null]]]:[['知识库查询','知识库搜索与文档读取。',[true,false,null]],['项目文件工具','通过本地进程提供项目文件工具。',[false,false,null]],['本地文档','读取项目内的文档与目录。',[false,true,null]]];
  values.forEach(([name,desc,states],i)=>{const ry=py+80+i*126;d.line(x+22,ry-8,x+cw-22,ry-8);d.icon(x+22,ry+27,kind==='skills'?'▤':'♧');d.text(x+78,ry+42,name,14,p.ink,600);d.text(x+78,ry+66,desc,11,p.muted);
    const used=states.findIndex(v=>v===true),sx=x+cw-205;
    if(used>=0){d.rect(sx,ry+32,183,37,p.soft,8);d.text(sx+13,ry+56,used===0?'C':'W',14,p.accent,600);d.text(sx+39,ry+56,'1 个客户端使用中  ›',12,p.ink);}else d.button(sx+51,ry+34,132,'选择客户端  ›');
  });return d;
}
function drawClientTileGrid(d,x,y,w,entries,cols=3,draft=false){
  const p=d.p,gap=8,tw=Math.min(124,(w-gap*(cols-1))/cols),th=84;
  entries.forEach((entry,i)=>{
    const xx=x+i%cols*(tw+gap),yy=y+Math.floor(i/cols)*(th+gap),on=entry.on||entry.selected;
    const names={codex:'ChatGPT(Codex)',workbuddy:'WorkBuddy',tiangong:'天工 Work',cursor:'Cursor'},symbols={codex:'C',workbuddy:'W',tiangong:'T',cursor:'↗'};
    d.rect(xx,yy,tw,th,on?p.accentSoft:p.surface,9,on?p.accent:p.line);
    d.rect(xx+tw/2-13,yy+10,26,26,on?p.accent:p.soft,8);
    d.text(xx+tw/2,yy+29,symbols[entry.key],17,on?p.on:p.subtle,600,'middle');
    d.text(xx+tw/2,yy+56,names[entry.key],12,on?p.ink:p.muted,600,'middle');
    if(on)d.text(xx+tw-8,yy+15,'✓',10,p.accent,600,'end');
    if(entry.unsupported)d.text(xx+tw/2,yy+73,'待适配',10,p.muted,400,'middle');
    if(draft&&entry.selected)d.text(xx+tw/2,yy+73,'已选',10,p.accent,400,'middle');
  });
}
function clientTargetsLayout(compactMode=false){
  const d=new Diagram(compactMode?900:1366,1068),p=d.p;
  shell(d,'Skill 管理');d.text(compactMode?194:230,126,'Skill 管理',24,p.ink,600);
  const w=compactMode?330:490,h=compactMode?390:290,x=(d.w-w)/2,y=240;
  d.raw(`<rect width="${d.w}" height="${d.h}" fill="#101c29" fill-opacity=".38"/>`);
  d.rect(x,y,w,h,p.surface,14);d.text(x+24,y+30,'配置成功后的选择 · 合成示意',11,p.muted);d.text(x+24,y+62,'代码审查 · 使用的客户端',20,p.ink,600);
  drawClientTileGrid(d,x+24,y+94,w-48,[{key:'workbuddy',on:true},{key:'codex'},{key:'tiangong',unsupported:true}],compactMode?2:3);
  d.line(x,y+h-61,x+w,y+h-61);d.button(x+w-108,y+h-47,84,'完成',true);return d;
}
function capabilityAddLayout(){
  const d=capabilityUseLayout('skills'),p=d.p;d.raw(`<rect width="${d.w}" height="${d.h}" fill="#101c29" fill-opacity=".38"/>`);
  const x=358,y=208,w=650,h=500;d.rect(x,y,w,h,p.surface,12);d.text(x+24,y+30,'交互示例 · 仅合成数据',11,p.muted);d.text(x+24,y+62,'添加 Skill',21,p.ink,600);d.text(x+w-26,y+37,'×',20,p.muted,400,'end');d.line(x,y+83,x+w,y+83);
  const field=(xx,yy,ww,label,value)=>{d.text(xx,yy,label,12,p.ink);d.rect(xx,yy+12,ww,36,p.surface,6,p.line);d.text(xx+12,yy+36,value,12,p.ink);};
  field(x+24,y+116,284,'内容来源','本地技能目录    ⌄');field(x+328,y+116,298,'名称','新技能 · 示例');field(x+24,y+192,w-48,'目录或仓库地址','DEMO-ONLY-SKILL-FOLDER');
  d.text(x+24,y+286,'使用的客户端',13,p.ink,600);
  drawClientTileGrid(d,x+24,y+307,w-48,[{key:'workbuddy',selected:true},{key:'codex'},{key:'tiangong'}],3,true);
  d.line(x,y+h-63,x+w,y+h-63);d.button(x+w-234,y+h-48,80,'取消');d.button(x+w-142,y+h-48,116,'添加并使用',true);return d;
}

function requestSetLayout(){
  const d=runtimeFlow(),p=d.p;d.raw(`<rect width="${d.w}" height="${d.h}" fill="#10232f" opacity=".36"/>`);
  const w=690,h=409,x=(d.w-w)/2,y=258;d.rect(x,y,w,h,p.surface,14,p.line);
  d.text(x+26,y+31,'仅合成数据',11,p.muted);d.text(x+26,y+62,'进行中请求 · WorkBuddy',21,p.ink,600);d.text(x+w-28,y+36,'×',20,p.muted,400,'end');
  d.line(x,y+84,x+w,y+84);d.chip(x+26,y+103,94,'WorkBuddy');d.text(x+137,y+121,'gpt-6   ·   团队 API   ·   model-a',12,p.ink);
  d.rect(x+26,y+151,w-52,35,p.soft,4);d.text(x+38,y+173,'请求 / 时间',11,p.muted);d.text(x+258,y+173,'模型 / 渠道',11,p.muted);d.text(x+w-165,y+173,'状态',11,p.muted);
  d.text(x+38,y+214,'DEMO-LIVE',12,p.ink,600);d.text(x+38,y+236,'2026-11-07 17:44:59',10,p.muted);d.text(x+258,y+214,'gpt-6',12,p.ink);d.text(x+258,y+236,'团队 API · model-a',11,p.muted);d.text(x+w-165,y+214,'进行中',12,p.accent);d.text(x+w-61,y+214,'详情',12,p.accent);d.line(x+26,y+254,x+w-26,y+254);
  d.text(x+26,y+285,'1 条',11,p.muted);d.text(x+w-30,y+285,'上一页    1 / 1    下一页',11,p.muted,400,'end');d.line(x,y+h-61,x+w,y+h-61);d.button(x+w-238,y+h-48,84,'关闭');d.button(x+w-142,y+h-48,116,'查看渠道配置',true);return d;
}
function channelLayout(){
  const d=new Diagram(1366,1068),{x,cw}=shell(d,'模型与渠道'),p=d.p;
  d.text(x,126,'模型与渠道',24,p.ink,600);d.button(x+cw-182,109,140,'+ 添加来源',true);d.circle(x+cw-16,124,9,p.muted);d.text(x+cw-16,129,'?',13,p.surface,600,'middle');tabs(d,x,187,cw,['我的模型','账号与渠道'],1);
  d.rect(x,220,cw,583);d.text(x+22,254,'团队 API',19,p.ink,600);d.chip(x+133,234,140,'WorkBuddy 专用');d.button(x+cw-116,237,94,'全部渠道');
  [['所属客户端','WorkBuddy'],['对应模型','gpt-6'],['渠道配置','已保存 · 合成配置'],['访问地址','https://api.example.com'],['接口类型','openai-responses'],['当前参数','思考 high · 输出 8,192']].forEach(([label,value],i)=>{const y=300+i*53;d.text(x+22,y,label,12,p.muted);d.text(x+200,y,value,13,p.ink);d.line(x+22,y+20,x+cw-22,y+20);});
  d.text(x+22,642,'›  路由身份与高级维护',13,p.muted,600);d.text(x+45,674,'workbuddy  /  model-a  /  gpt-6  /  team',12,p.muted);d.line(x+22,724,x+cw-22,724);d.button(x+22,749,138,'查看此路由记录');d.button(x+cw-145,749,123,'调整此端参数');return d;
}
function sourceAddLayout(){
  const d=modelLibrary(),p=d.p;d.raw(`<rect width="${d.w}" height="${d.h}" fill="#10232f" opacity=".36"/>`);
  const w=650,h=790,x=(d.w-w)/2,y=72;d.rect(x,y,w,h,p.surface,14,p.line);d.text(x+26,y+30,'交互示例 · 仅合成数据',11,p.muted);d.text(x+26,y+60,'添加模型来源',21,p.ink,600);d.text(x+w-28,y+36,'×',20,p.muted,400,'end');d.line(x,y+83,x+w,y+83);
  const field=(yy,label,value)=>{d.text(x+26,y+yy,label,12,p.ink,500);d.rect(x+26,y+yy+10,w-52,35,p.surface,6,p.line);d.text(x+37,y+yy+33,value,12,p.ink);};
  field(115,'厂商与接入方式','智谱 Anthropic                                                   ⌄');field(183,'来源名称','智谱 · 示例');field(251,'智谱接入模式','Coding Plan                                                          ⌄');field(319,'服务地址','https://open.bigmodel.cn/api/anthropic');field(387,'API Key','DEMO-ONLY-NOT-A-KEY');d.text(x+26,y+451,'合成原型，请勿输入真实密钥。',11,p.muted);
  field(485,'模型列表','GLM · 示例');d.button(x+26,y+550,164,'获取模型目录（演示）');d.text(x+26,y+617,'›  模型映射与高级设置',13,p.muted,600);d.text(x+26,y+647,'厂商协议、目录、优先级和兼容规则保留',12,p.muted);d.line(x,y+h-61,x+w,y+h-61);d.button(x+w-287,y+h-47,84,'取消');d.button(x+w-191,y+h-47,165,'保存并继续（演示）',true);return d;
}
function reuse(){
  const d=clientModelsLayout(),p=d.p;d.raw(`<rect width="${d.w}" height="${d.h}" fill="#101c29" fill-opacity=".38"/>`);const x=358,y=209,w=650;
  d.rect(x,y,w,400,p.surface,10);d.text(x+24,y+28,'跨端复用 · 选择目标',11,p.muted);d.text(x+24,y+59,'将 GPT-6 用于客户端',20,p.ink,600);d.text(x+w-26,y+42,'×',20,p.muted,400,'end');d.line(x,y+80,x+w,y+80);
  d.rect(x+24,y+101,w-48,73,p.soft,7);d.icon(x+38,y+118);d.text(x+90,y+128,'GPT-6',14,p.ink,600);d.text(x+90,y+151,'团队 API · 来自 WorkBuddy 当前配置',12,p.muted);d.chip(x+w-121,y+124,79,'独立副本');
  drawClientTileGrid(d,x+24,y+206,w-48,[{key:'codex',on:true},{key:'tiangong'}],2);
  d.line(x,y+345,x+w,y+345);d.button(x+w-108,y+359,84,'完成');return d;
}
function sourceUpdate(){
  const d=modelLibrary(true),p=d.p;d.raw(`<rect width="${d.w}" height="${d.h}" fill="#101c29" fill-opacity=".38"/>`);const x=348,y=139,w=670;
  d.rect(x,y,w,649,p.surface,10);d.text(x+24,y+28,'继续上一场景：已复用到天工并独立调整',11,p.muted);d.text(x+24,y+60,'来源更新 · 选择同步范围',20,p.ink,600);d.line(x,y+81,x+w,y+81);
  d.rect(x+24,y+102,w-48,67,p.soft,7);d.text(x+40,y+127,'GPT-6 · 团队 API',14,p.ink,600);d.text(x+40,y+150,'来源版本 2 · 客户端副本不会自动更新',12,p.muted);
  d.text(x+24,y+205,'连接地址',12,p.muted);d.text(x+211,y+200,'api.example.com',12,p.muted);d.text(x+211,y+223,'→ api.example.com/gateway',13,p.accent);d.line(x+24,y+242,x+w-24,y+242);
  d.text(x+24,y+272,'同步来源连接信息，保留各端已经调整的模型参数。',13);d.text(x+24,y+297,'Codex 继续使用普通路由，不生成专用渠道。',12,p.muted);
  const rows=[['WorkBuddy','high','8,192',false],['天工 Work','max','16,384',true]];
  rows.forEach(([name,effort,output,selected],i)=>{const ty=y+324+i*78;d.rect(x+27,ty+12,16,16,selected?p.accent:p.surface,3,selected?null:p.line);if(selected)d.text(x+35,ty+25,'✓',12,p.on,600,'middle');d.text(x+62,ty+24,name,14,p.ink,600);d.text(x+62,ty+49,'思考 '+effort+' · 输出 '+output+' · 保留',12,p.muted);d.text(x+w-27,ty+27,'待同步',11,p.muted,400,'end');d.line(x+24,ty+66,x+w-24,ty+66);});
  d.rect(x+24,y+500,w-48,62,p.soft,7);d.text(x+39,y+525,'只同步选中的天工副本；WorkBuddy 保持旧版本。',12,p.muted);d.text(x+39,y+547,'写入前仍核对目标 revision、文件锁与外部后改。',12,p.muted);d.line(x,y+585,x+w,y+585);d.button(x+w-260,y+601,77,'取消');d.button(x+w-170,y+601,146,'同步所选客户端',true);return d;
}
function runtimeFlow(theme='light',compactMode=false,mode='active'){
  const d=new Diagram(compactMode?900:1366,1068,theme),{x,cw}=shell(d),p=d.p;
  d.text(x,126,'概览',24,p.ink,600);d.button(x+cw-158,108,116,'+ 配置客户端');d.circle(x+cw-16,124,9,p.muted);d.text(x+cw-16,129,'?',13,p.surface,600,'middle');tabs(d,x,178,cw,['运行状态','使用统计']);
  const fy=215,ox=x+20,inner=cw-40,lw=compactMode?160:222,hw=compactMode?84:128,rw=compactMode?148:188,channelWidth=rw,gap=(inner-lw-hw-rw-channelWidth)/3,hx=mode==='closed'?ox+(inner-hw)/2:ox+lw+gap,rx=hx+hw+gap,cx=rx+rw+gap;
  const color={workbuddy:theme==='dark'?'#b3a0f0':'#7866c5',tiangong:theme==='dark'?'#e5bb63':'#c18c28',ordinary:theme==='dark'?'#94b7e1':'#6685b0',codex:p.subtle};
  const panelH=mode==='closed'?340:mode==='capacity'?632:460;
  d.rect(x,fy,cw,panelH,p.surface,18);d.text(ox,fy+32,'路由运行',16,p.ink,600);d.text(ox+86,fy+31,mode==='closed'?'当前待用':'1 个请求处理中',11,mode==='closed'?p.muted:p.accent);
  const top=fy+56,hy=top+(mode==='closed'?45:mode==='capacity'?207:105),hh=128;
  const clients=mode==='closed'?[]:[{key:'codex',name:'ChatGPT / Codex',symbol:'◉',label:'',height:58},
    {key:'workbuddy',name:'WorkBuddy',symbol:'W',label:'请求处理中',height:50},
    {key:'tiangong',name:'天工 Work',symbol:'T',label:'已打开 · 空闲',height:50},
    ...(mode==='capacity'?['A','B','C','D','E'].map(letter=>({key:'planned-'+letter,name:'客户端 '+letter,symbol:letter,planned:true,height:40})):[])];
  let yy=top+(mode==='capacity'?0:64);
  clients.forEach(c=>{if(c.key==='ordinary')yy+=5;c.y=yy;c.cy=yy+c.height/2;yy+=c.height+7;});
  const routes=mode==='closed'?[]:U.records.filter(r=>r.status==='running');
  const rtop=hy+hh/2-32;
  const wire=(a,b,scope,active=false,unknown=false)=>{
    const dx=(b.x-a.x)*.55,shape=`M${a.x} ${a.y} C${a.x+dx} ${a.y} ${b.x-dx} ${b.y} ${b.x} ${b.y}`;
    d.raw(`<path d="${shape}" stroke="${unknown?p.line:color[scope]}" stroke-width="1.4" opacity="${unknown?'.9':'.32'}" fill="none"${unknown?' stroke-dasharray="3 5"':''}/>`);
    if(active)d.raw(`<path d="${shape}" stroke="${color[scope]}" stroke-width="2.5" stroke-dasharray="2 22" fill="none"/>`);
  };
  clients.filter(c=>!c.planned).forEach(c=>wire({x:ox+lw,y:c.cy},{x:hx,y:hy+hh/2},c.key,c.key==='workbuddy',c.key==='codex'));
  routes.forEach((r,i)=>{const y=rtop+i*74+32;wire({x:hx+hw,y:hy+hh/2},{x:rx,y},r.scope,true);wire({x:rx+rw,y},{x:cx,y},r.scope,true);});
  clients.forEach(c=>{
    d.rect(ox,c.y,lw,c.height,c.planned?p.soft:p.surface,10,c.planned?null:p.line);
    d.text(ox+20,c.cy+5,c.symbol,c.planned?13:18,c.planned?p.subtle:color[c.key],600,'middle');
    d.text(ox+39,c.y+(c.planned?25:23),c.name,compactMode?11:12,p.ink,600);
    if(c.planned)d.text(ox+lw-10,c.y+25,'规划',9,p.subtle,400,'end');
    else if(c.key==='codex'){
      d.text(ox+39,c.y+44,'执行',8,p.muted);let dx=ox+65;
      [['App',true],['VS Code',false],['CLI',false]].forEach(([label,ok])=>{d.circle(dx,c.y+41,2,ok?p.ok:p.subtle);d.text(dx+5,c.y+44,label,8,ok?p.ok:p.muted);dx+=label==='VS Code'?42:28;});
      d.circle(ox+lw-11,c.y+21,2.5,p.ok);
    }else{d.text(ox+39,c.y+39,c.label,10,p.muted);d.circle(ox+lw-11,c.y+20,2.5,c.key==='ordinary'?p.subtle:p.ok);}
  });
  if(mode!=='closed')d.raw(`<rect x="${hx-7}" y="${hy-7}" width="${hw+14}" height="${hh+14}" rx="28" fill="none" stroke="${p.accent}" opacity=".12"/>`);
  d.rect(hx,hy,hw,hh,p.surface,21,p.line);d.rect(hx+hw/2-19,hy+16,38,38,p.accent,13);d.text(hx+hw/2,hy+44,'T',24,p.on,600,'middle');d.text(hx+hw/2,hy+80,'Hub',16,p.ink,600,'middle');d.circle(hx+hw/2-41,hy+100,2.5,p.ok);d.text(hx+hw/2+4,hy+103,'本机服务运行中',compactMode?8:10,p.ok,400,'middle');
  routes.forEach((r,i)=>{
    const y=rtop+i*74,name=r.model.length>18?r.model.slice(0,16)+'…':r.model;
    d.rect(rx,y,rw,64,p.surface,10,color[r.scope]);d.text(rx+12,y+26,name,compactMode?11:12,p.ink,600);d.circle(rx+rw-12,y+24,2.5,p.accent);
    d.text(rx+12,y+46,(r.scope==='ordinary'?'普通接入':r.scope==='tiangong'?'天工 Work':'WorkBuddy')+' · 1 个请求',10,p.muted);
    d.rect(cx,y,channelWidth,64,p.surface,10,color[r.scope]);d.text(cx+18,y+37,'◇',18,color[r.scope],400,'middle');d.text(cx+35,y+26,U.sources[r.source],compactMode?11:12,p.ink,600);d.text(cx+35,y+46,'处理中',10,p.accent);
  });
  const footer=fy+panelH-39;d.line(ox,footer-19,x+cw-20,footer-19);d.text(x+cw-20,footer+3,'演示状态  ⌄',10,p.muted,400,'end');
  const sy=fy+panelH+24,cols=compactMode?2:4,sw=cw/cols;
  [['◌','聊天接入','2 个示例账号'],['▤','Skill','2 项使用中'],['♧','MCP','3 项使用中'],['◇','Dumpling','']].forEach(([symbol,name,label],i)=>{const sx=x+i%cols*sw,y=sy+Math.floor(i/cols)*65;if(i%cols)d.line(sx,y,sx,y+47);d.text(sx+15,y+24,symbol,18,p.muted);d.text(sx+44,y+18,name,12,p.ink,500);d.text(sx+44,y+38,label,10,p.muted);d.text(sx+sw-12,y+25,'›',15,p.muted);});
  return d;
}

function pageHelp(){
  const d=runtimeFlow(),p=d.p,mw=626,mh=760,x=(d.w-mw)/2,y=132;
  d.raw(`<rect width="${d.w}" height="${d.h}" fill="#10232f" opacity=".36"/>`);d.rect(x,y,mw,mh,p.surface,14,p.line);
  d.text(x+26,y+28,'本页帮助',11,p.muted);d.text(x+26,y+60,'运行状态 · 使用说明',20,p.ink,600);d.text(x+mw-28,y+36,'×',20,p.muted,400,'end');
  d.text(x+26,y+99,'推荐顺序',11,p.muted,600);d.text(x+26,y+126,'1. 已有配置时打开客户端；首次使用先点“配置客户端”。',12,p.ink);d.text(x+26,y+150,'2. 观察当前请求链路，点选节点查看详情。',12,p.ink);
  const rows=[
    ['配置客户端',['选择客户端和模型，沿引导保存接入；可从已配置模型复用。']],
    ['客户端 / Hub 节点',['查看运行、受管配置和本机服务各自的证据。','ChatGPT 执行通道与模型请求分别记录。']],
    ['模型 / 渠道节点',['短节点逐行对应；相关请求按路由列出，渠道直达对应配置。']],
    ['使用统计',['切换到用量和趋势；已结束的请求仍在请求记录中。']],
    ['演示状态 / 暂停动效',['切换本页合成场景；顶部按钮暂停全局动效。','下方聊天、Skill、MCP、Dumpling 摘要进入各自管理页。']]
  ];
  let yy=y+178;for(const [label,lines] of rows){d.line(x+26,yy-8,x+mw-26,yy-8);d.text(x+26,yy+13,label,12,p.ink,600);lines.forEach((line,i)=>d.text(x+26,yy+35+i*20,line,12,p.muted));yy+=lines.length===2?84:64;}
  d.rect(x+26,yy+3,mw-52,89,p.soft,9);['只显示已打开 / 实际在用的客户端和进行中模型、渠道；无人活动时只留 Hub。','绿色运行点不代表调用成功；流动仅表示请求进行中。','普通接入不推定为 ChatGPT，同名模型与渠道仍按端和条目独立。'].forEach((line,i)=>d.text(x+39,yy+27+i*21,line,11,p.muted));
  d.text(x+26,y+mh-69,'合成原型 · 关闭说明后继续当前操作',10,p.subtle);d.line(x,y+mh-53,x+mw,y+mh-53);d.button(x+mw-109,y+mh-42,83,'知道了',true);return d;
}

function drawReviewInput(d,x,y,w,label,value){
  d.text(x,y,label,12,d.p.ink,500);d.rect(x,y+12,w,37,d.p.surface,9,d.p.line);d.text(x+12,y+36,value,12,d.p.ink);
}
function dumplingEnvironmentLayout(mode='password',theme='light',compactMode=false){
  const d=new Diagram(compactMode?900:1366,mode==='application'?1230:mode==='browser'?1200:1120,theme),p=d.p,{x,cw}=shell(d,'MCP 管理');
  d.text(x,126,'MCP 管理',24,p.ink,600);d.button(x+cw-140,108,140,'+ 添加 MCP',true);tabs(d,x,182,cw,['工具服务','Dumpling-MCP'],1);
  drawReviewInput(d,x,222,Math.min(320,cw-250),'环境','新环境 · 示例      ⌄');d.button(x+Math.min(340,cw-230),235,116,'+ 添加环境');
  [['1','配置环境'],['2','登录 / 授权'],['3','连接客户端']].forEach(([n,label],i)=>{const xx=x+i*(compactMode?160:180);d.circle(xx+11,313,11,i===0?p.accent:p.line);d.text(xx+11,317,n,11,i===0?p.on:p.muted,600,'middle');d.text(xx+33,317,label,12,i===0?p.accent:p.muted,i===0?600:400);});
  const cy=348,cardH=mode==='application'?826:mode==='browser'?746:666;d.rect(x,cy,cw,cardH,p.surface,14);d.text(x+24,cy+31,'配置环境',17,p.ink,600);
  const ix=x+24,gap=20,fw=(cw-48-gap)/2;
  drawReviewInput(d,ix,cy+68,fw,'环境名称','新环境 · 示例');drawReviewInput(d,ix+fw+gap,cy+68,fw,'NVWA 服务地址','https://nvwa.example.com/product');
  d.text(ix,cy+148,'认证方式',12,p.ink,500);switcher(d,ix,cy+160,['账号密码','认证服务连接'],mode==='password'?0:1,compactMode?136:152);
  let yy=cy+230;
  if(mode==='password'){
    drawReviewInput(d,ix,yy,fw,'账号','demo-user');drawReviewInput(d,ix+fw+gap,yy,fw,'密码','DEMO-ONLY');d.rect(ix,yy+75,15,15,p.surface,4,p.line);d.text(ix+25,yy+87,'记住密码',12,p.ink);yy+=130;
  }else{
    d.rect(ix,yy-3,15,15,mode==='application'?p.accent:p.surface,4,p.line);if(mode==='application')d.text(ix+7.5,yy+9,'✓',12,p.on,600,'middle');d.text(ix+25,yy+9,'使用共享应用代表指定用户',12,p.ink);yy+=44;
    drawReviewInput(d,ix,yy,fw,'ClientID','DEMO-CLIENT');drawReviewInput(d,ix+fw+gap,yy,fw,'ClientSecret','DEMO-ONLY-SECRET');d.rect(ix,yy+75,15,15,p.surface,4,p.line);d.text(ix+25,yy+87,'保存应用密钥',12,p.ink);yy+=122;
    if(mode==='application'){drawReviewInput(d,ix,yy,fw,'代表的账号','demo-user');yy+=84;}
    d.rect(ix,yy,cw-48,38,p.soft,7);d.text(ix+12,yy+24,mode==='application'?'在认证服务管理添加应用服务，获取 ClientID 和 ClientSecret。':'在认证服务申请应用；个人登录将在浏览器中完成。',compactMode?10:12,p.ink);yy+=66;
  }
  d.line(ix,yy-7,x+cw-24,yy-7);d.text(ix,yy+19,'⌄  高级设置',13,p.ink,600);yy+=49;
  drawReviewInput(d,ix,yy,fw,'独立认证服务地址','留空使用 NVWA 服务地址');drawReviewInput(d,ix+fw+gap,yy,fw,'MCP 地址','/mcp');yy+=77;
  drawReviewInput(d,ix,yy,fw,mode==='password'?'登录机构（可选）':mode==='application'?'应用签名算法':'限定授权账号（可选）',mode==='application'?'SHA-256      ⌄':'');drawReviewInput(d,ix+fw+gap,yy,fw,'指定租户（可选）','');
  const bottom=cy+cardH-54;d.line(ix,bottom-12,x+cw-24,bottom-12);d.button(x+cw-188,bottom,164,'保存环境并继续',true);return d;
}
function mcpEditorLayout({theme='light',compactMode=false,local=false,json=false,filled=false}={}){
  const d=new Diagram(compactMode?700:1366,compactMode?1220:1068,theme),p=d.p;
  const {x:mainX,cw}=shell(d,'MCP 管理');d.text(mainX,126,'MCP 管理',24,p.ink,600);d.button(mainX+cw-128,108,128,'+ 添加 MCP',true);
  const w=compactMode?636:760,h=local?1008:compactMode?1040:json?806:filled?895:810,x=(d.w-w)/2,y=(d.h-h)/2,ix=x+26,iw=w-52;
  d.raw(`<rect width="${d.w}" height="${d.h}" fill="#101c29" fill-opacity=".38"/>`);d.rect(x,y,w,h,p.surface,14);d.text(ix,y+28,'交互示例 · 仅合成数据',10,p.accent);d.text(ix,y+59,'添加 MCP',22,p.ink,600);d.text(x+w-25,y+41,'×',20,p.muted,400,'end');d.line(x,y+82,x+w,y+82);
  d.rect(ix,y+102,iw-40,42,p.soft,9);const tw=(iw-48)/2;d.rect(ix+4+(json?tw:0),y+106,tw,34,p.surface,6);d.text(ix+4+tw/2,y+128,'☷  表单配置',13,json?p.muted:p.accent,600,'middle');d.text(ix+4+tw*1.5,y+128,'{ }  JSON 配置',13,json?p.accent:p.muted,600,'middle');d.circle(x+w-37,y+123,11,p.soft);d.text(x+w-37,y+128,'?',13,p.muted,600,'middle');
  let yy=y+176;
  if(json){
    d.text(ix,yy,'MCP 配置 JSON',12,p.ink,600);yy+=17;d.rect(ix,yy,iw,352,p.soft,7,p.line);
    const lines=['{','  "mcpServers": {','    "project-tools": {','      "type": "stdio",','      "command": "npx",','      "args": ["-y", "demo-mcp-server"],','      "env": {"DEMO_KEY": "DEMO-ONLY"},','      "env_vars": ["PATH"],','      "cwd": '+JSON.stringify('D:\\Projects\\demo'),'    }','  }','}'];
    lines.forEach((line,i)=>d.raw(`<text x="${ix+16}" y="${yy+26+i*25}" xml:space="preserve" style="font-family:Consolas,monospace" font-size="12" fill="${p.ink}">${esc(line)}</text>`));yy+=378;
  }else{
    if(compactMode){drawReviewInput(d,ix,yy,iw,'服务名称','');yy+=77;drawReviewInput(d,ix,yy,iw,'简介（可选）','');yy+=88;}
    else{drawReviewInput(d,ix,yy,(iw-18)/2,'服务名称',filled?'项目工具 · 示例':'');drawReviewInput(d,ix+(iw+18)/2,yy,(iw-18)/2,'简介（可选）',filled?'项目协作工具':'');yy+=89;}
    d.line(ix,yy-12,ix+iw,yy-12);d.text(ix,yy+17,'连接类型',12,p.ink,600);
    const types=[['Streamable HTTP',local?false:true,138],['STDIO',local,74],['SSE',false,60]];let sx=compactMode?ix:ix+iw-290,sy=compactMode?yy+31:yy-3;
    types.forEach(([label,selected,ww])=>{d.rect(sx,sy,ww,32,selected?p.accentSoft:p.surface,7,selected?p.accent:p.line);d.text(sx+ww/2,sy+21,label,12,selected?p.accent:p.ink,500,'middle');sx+=ww+6;});yy+=compactMode?100:66;
    if(local){
      drawReviewInput(d,ix,yy,iw,'启动命令','npx');yy+=78;
      d.text(ix,yy,'参数',12,p.ink,600);d.text(ix+iw,yy,'＋ 添加',11,p.accent,500,'end');yy+=13;
      for(const value of ['-y','demo-mcp-server']){d.rect(ix,yy,iw-34,36,p.surface,6,p.line);d.text(ix+12,yy+23,value,12);d.text(ix+iw-14,yy+24,'×',17,p.muted,400,'middle');yy+=45;}yy+=13;
      d.text(ix,yy,'环境变量',12,p.ink,600);d.text(ix+iw,yy,'＋ 添加',11,p.accent,500,'end');yy+=13;
      d.rect(ix,yy,(iw-44)/2,36,p.surface,6,p.line);d.text(ix+12,yy+23,'DEMO_KEY',12);d.rect(ix+(iw-44)/2+10,yy,(iw-44)/2,36,p.surface,6,p.line);d.text(ix+(iw-44)/2+22,yy+23,'••••••••••',12,p.muted);d.text(ix+iw-14,yy+24,'×',17,p.muted,400,'middle');yy+=63;
      d.text(ix,yy,'传递的环境变量',12,p.ink,600);d.text(ix+iw,yy,'＋ 添加',11,p.accent,500,'end');yy+=13;d.rect(ix,yy,iw-34,36,p.surface,6,p.line);d.text(ix+12,yy+23,'PATH',12);d.text(ix+iw-14,yy+24,'×',17,p.muted,400,'middle');yy+=68;
      drawReviewInput(d,ix,yy,iw,'工作目录（可选）','D:\\Projects\\demo');yy+=81;
    }else{
      drawReviewInput(d,ix,yy,iw,'服务地址',filled?'https://mcp.example.com/mcp':'');yy+=85;d.text(ix,yy+18,'认证方式',12,p.ink,600);
      const auth=[['无需认证',!filled,92],['Bearer Token',filled,112],['自定义请求头',false,114]];sx=compactMode?ix:ix+iw-330;sy=compactMode?yy+34:yy;
      auth.forEach(([label,selected,ww])=>{d.rect(sx,sy,ww,32,selected?p.accentSoft:p.surface,7,selected?p.accent:p.line);d.text(sx+ww/2,sy+21,label,12,selected?p.accent:p.ink,500,'middle');sx+=ww+6;});yy+=compactMode?98:68;
      if(filled){drawReviewInput(d,ix,yy,iw,'Token','••••••••••••••••');yy+=85;}
      d.text(ix,yy,'请求头（Headers）',12,p.ink,600);d.text(ix+iw,yy,'＋ 添加',11,p.accent,500,'end');yy+=13;
      d.rect(ix,yy,(iw-44)/2,36,p.surface,6,p.line);d.text(ix+12,yy+23,'名称',12,p.subtle);d.rect(ix+(iw-44)/2+10,yy,(iw-44)/2,36,p.surface,6,p.line);d.text(ix+(iw-44)/2+22,yy+23,'值',12,p.subtle);d.text(ix+iw-14,yy+24,'×',17,p.muted,400,'middle');yy+=69;
    }
  }
  d.line(ix,yy-5,ix+iw,yy-5);d.text(ix,yy+24,'使用的客户端',12,p.ink,600);drawClientTileGrid(d,ix,yy+40,iw,[{key:'workbuddy',selected:true},{key:'codex'},{key:'tiangong',unsupported:local||json}],3,true);yy+=144;
  d.text(ix,yy,'›  从已配置客户端复用',11,p.muted);d.line(x,y+h-65,x+w,y+h-65);d.button(x+w-238,y+h-48,80,'取消');d.button(x+w-144,y+h-48,118,'添加并使用',true);return d;
}
function mcpAddTypedLayout(local=false){return mcpEditorLayout({local,filled:true});}
function mcpEntryLayout(theme='light',compactMode=false){return mcpEditorLayout({theme,compactMode});}
function selectorOpenLayout(){
  const d=sourceAddLayout(),p=d.p,x=(d.w-650)/2+26,y=238,w=598;
  d.rect(x,y,w,326,p.surface,11,p.line);d.rect(x+8,y+8,w-16,35,p.soft,7);d.text(x+20,y+31,'⌕  搜索选项',12,p.muted);
  ['OpenAI Responses','ChatGPT 账号','Grok','OpenAI Chat Completions','DeepSeek','Kimi','Anthropic','智谱 Anthropic'].forEach((v,i)=>{const yy=y+52+i*32;if(i===7)d.rect(x+6,yy-16,w-12,30,p.accentSoft,7);d.text(x+17,yy+5,v,12,i===7?p.accent:p.ink,i===7?600:400);if(i===7)d.text(x+w-17,yy+5,'✓',13,p.accent,600,'end');});return d;
}
function navExpandedLayout(){
  const d=runtimeFlow('light',false,'capacity'),p=d.p;
  d.rect(0,153,203,d.h-335,p.surface,0);d.text(25,184,'客户端',11,p.subtle);d.text(175,184,'收起',11,p.accent,500,'end');
  ['ChatGPT(Codex)','WorkBuddy','天工 Work','客户端 A','客户端 B','客户端 C','客户端 D','客户端 E'].forEach((v,i)=>{const yy=223+i*43;d.text(27,yy,i===0?'C':i===1?'W':i===2?'T':'◇',16,p.accent,600);d.text(54,yy,v,12,p.ink);if(i>2)d.text(183,yy,'规划',10,p.subtle,400,'end');});
  d.text(25,591,'共用能力',11,p.subtle);['模型与渠道','Skill 管理','MCP 管理','聊天接入','请求记录'].forEach((v,i)=>d.text(54,631+i*43,v,12,p.muted));d.rect(197,202,3,430,p.line,2);return d;
}

async function write(name,d){
  const only=process.argv.indexOf('--only');if(only>=0&&!process.argv[only+1].split(',').includes(name))return;
  if(!/^[a-z0-9-]+-layout$/.test(name))throw new Error('Unexpected output name');
  const svg=d.svg();fs.writeFileSync(path.join(__dirname,name+'.svg'),svg,'utf8');await sharp(Buffer.from(svg)).png().toFile(path.join(__dirname,name+'.png'));
  console.log(name+' '+d.w+'x'+d.h);
}
(async()=>{
  await write('mcp-json-v3-layout',mcpEditorLayout({json:true}));
  await write('mcp-entry-v3-layout',mcpEntryLayout());
  await write('mcp-entry-v3-dark-layout',mcpEntryLayout('dark'));
  await write('mcp-entry-v3-compact-layout',mcpEntryLayout('light',true));
  await write('client-targets-v3-layout',clientTargetsLayout());
  await write('client-targets-v3-compact-layout',clientTargetsLayout(true));
  await write('dumpling-password-v3-layout',dumplingEnvironmentLayout());
  await write('dumpling-application-v3-layout',dumplingEnvironmentLayout('application'));
  await write('dumpling-browser-v3-layout',dumplingEnvironmentLayout('browser','dark'));
  await write('mcp-remote-add-v3-layout',mcpAddTypedLayout());
  await write('mcp-local-add-v3-layout',mcpAddTypedLayout(true));
  await write('client-nav-expanded-v3-layout',navExpandedLayout());
  await write('selector-open-v3-layout',selectorOpenLayout());
  await write('usage-v3-layout',usage());await write('usage-v3-dark-layout',usage('dark'));
  await write('usage-v3-compact-layout',usage('light',true));await write('models-v3-layout',modelLibrary());
  await write('reuse-v3-layout',reuse());await write('source-update-v3-layout',sourceUpdate());
  await write('runtime-flow-v3-layout',runtimeFlow());
  await write('runtime-flow-v3-dark-layout',runtimeFlow('dark'));
  await write('runtime-flow-v3-compact-layout',runtimeFlow('light',true));
  await write('runtime-hub-only-v3-layout',runtimeFlow('light',false,'closed'));
  await write('runtime-capacity-v3-layout',runtimeFlow('light',false,'capacity'));
  await write('page-help-v3-layout',pageHelp());
  await write('skills-use-v3-layout',capabilityUseLayout('skills'));
  await write('mcp-use-v3-layout',capabilityUseLayout('mcp','dark'));
  await write('capability-add-v3-layout',capabilityAddLayout());
  await write('client-status-v3-layout',clientOverviewLayout());
  await write('client-status-v3-dark-layout',clientOverviewLayout('codex','dark'));
  await write('client-status-v3-compact-layout',clientOverviewLayout('tiangong','light',true));
  await write('route-requests-v3-layout',requestSetLayout());
  await write('channel-detail-v3-layout',channelLayout());
  await write('source-add-v3-layout',sourceAddLayout());

})().catch(e=>{console.error(e.message);process.exitCode=1;});
