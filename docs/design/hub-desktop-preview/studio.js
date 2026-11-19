(() => {
  "use strict";
  const $ = s => document.querySelector(s);
  const paths = {
    overview:'<rect x="3" y="3" width="7" height="8" rx="1.8"/><rect x="14" y="3" width="7" height="5" rx="1.6"/><rect x="14" y="12" width="7" height="9" rx="1.8"/><rect x="3" y="15" width="7" height="6" rx="1.6"/>',
    models:'<path d="m12 3 9 5-9 5-9-5zM3 12l9 5 9-5M3 16l9 5 9-5"/>',logs:'<rect x="4" y="3" width="16" height="18" rx="3"/><path d="M8 8h8M8 12h8M8 16h5"/>',
    client:'<rect x="3" y="4" width="18" height="13" rx="2.5"/><path d="M9 21h6m-3-4v4"/>',settings:'<path d="M4 7h8m4 0h4M4 17h3m4 0h9"/><circle cx="14" cy="7" r="2.4"/><circle cx="9" cy="17" r="2.4"/>',
    help:'<circle cx="12" cy="12" r="9"/><path d="M9 9a3 3 0 1 1 4.5 2.5c-1.5 1-1.5 1.4-1.5 2.3m0 2.4v.2"/>',
    chat:'<path d="M20 12a8 8 0 0 1-8 8c-1.4 0-2.6-.2-3.8-.8l-4.9 1 1.2-4.5A8.2 8.2 0 1 1 20 12Z"/><path d="M8.5 10.7h7m-7 3h4"/>',
    check:'<path d="m4.5 12 4.5 4.5 10-10"/>',chevron:'<path d="m9 6 6 6-6 6"/>',arrow:'<path d="M5 13h13m-5-5 5 5-5 5"/>',
    clock:'<circle cx="12" cy="12" r="9"/><path d="M12 6v6l4 2"/>',pulse:'<path d="M2 12h5l3-7 4 14 3-7h5"/>',
    link:'<path d="m9 15 6-6m-1-3 2-2a4 4 0 0 1 6 6l-4 4m-8 4-2 2a4 4 0 0 1-6-6l4-4"/>',
    shield:'<path d="m12 3 8 3v6c0 4.5-3.5 7.3-8 9-4.5-1.7-8-4.5-8-9V6z"/><path d="m8.8 12 2.2 2.2 4.5-4.5"/>',
    spark:'<path d="m11 4 2.3 6.2L19 12l-5.7 2.2L11 21l-2.2-6.8L3 12l5.8-1.8z"/>',bolt:'<path d="m13 2-9 12h7l-1 8 10-13h-7z"/>',more:'<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>',
    box:'<path d="m12 2 9 5v10l-9 5-9-5V7zm0 10 9-5m-9 5L3 7m9 5v10"/>',wrench:'<path d="m5 20-2-2 9-9a5 5 0 0 1 6-6l-3 3 3 3 3-3a5 5 0 0 1-6 6z"/>',folder:'<path d="M3 7V5a2 2 0 0 1 2-2h5l3 3h6a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
    moon:'<path d="M20.3 14.5A8.8 8.8 0 0 1 9.5 3.7a8.8 8.8 0 1 0 10.8 10.8Z"/>',sun:'<circle cx="12" cy="12" r="4"/><path d="M12 2v2m0 16v2M2 12h2m16 0h2M5 5l1.4 1.4m11.2 11.2L19 19M5 19l1.4-1.4M17.6 6.4 19 5"/>',
    close:'<path d="m6 6 12 12M6 18 18 6"/>',plus:'<path d="M5 12h14m-7-7v14"/>',code:'<path d="m8 7-5 5 5 5m8-10 5 5-5 5"/>',command:'<rect x="7" y="7" width="10" height="10" rx="2"/><path d="M7 7H5a2 2 0 1 1 2-2zm10 0V5a2 2 0 1 1 2 2zm0 10h2a2 2 0 1 1-2 2zM7 17v2a2 2 0 1 1-2-2z"/>'
  };
  window.hubIcon=(key,size=18)=>'<svg aria-hidden="true" focusable="false" width="'+size+'" height="'+size+'" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">'+(paths[key]||paths.box)+'</svg>';
  const clients=["ChatGPT / Codex","WorkBuddy","天工 Work","DeepSeek","ZCode"], names={overview:"概览",models:"模型与渠道",logs:"请求日志"}, badges=['code','bolt','spark','link','command'];
  const groups=[['工作空间',[['overview','概览','overview'],['models','模型与渠道','models'],['logs','请求日志','logs']]],['客户端',clients.map((n,i)=>['client',n,badges[i]])]];
  $('#nav').innerHTML='<div id="nav-stack" data-track>'+groups.map(([label,items],g)=>
    '<div class="navgroup"><button type="button" class="fold-toggle" aria-expanded="true" aria-controls="fold-'+g+'">'+label+hubIcon('chevron',13)+'</button>'+
    '<div id="fold-'+g+'" class="navfold"><div class="fold-clip">'+items.map(([page,name,icon],i)=>'<button class="navbtn" style="--row-delay:'+i*16+'ms" data-page="'+page+'" data-name="'+name+'"><span class="navglyph '+(page==='client'?'clientglyph c'+i:'')+'">'+hubIcon(icon)+'</span>'+name+'</button>').join('')+'</div></div></div>').join('')+'</div>';
  document.dispatchEvent(new Event('hub:folds-ready'));
  function fold(button,open){
    button.setAttribute('aria-expanded',String(open));
    const children=document.getElementById(button.getAttribute('aria-controls'));
    if(!open&&children.contains(document.activeElement))button.focus({preventScroll:true});
    children.inert=!open;
  }
  function railSelection(){
    const item=$('#nav .navbtn[aria-current="page"]');
    const header=item?.closest('.navgroup').querySelector('.fold-toggle');
    const folded=header?.getAttribute('aria-expanded')==='false';
    $('#nav').querySelectorAll('.fold-toggle').forEach(b=>b.classList.toggle('is-current',b===header&&folded));
    hubTrack($('#nav-stack'),folded?header:item);
  }
  let current='overview',currentSheetFocus,closeTimer, navAnim;
  const historyLog=[], historyTools=document.createElement('div');
  let cursor=-1;
  historyTools.className='history-controls';historyTools.setAttribute('role','group');historyTools.setAttribute('aria-label','页面历史');
  historyTools.innerHTML='<button type="button" class="tool" data-step="-1" title="返回上一页" aria-label="返回上一页">'+hubIcon('chevron',16)+'</button><button type="button" class="tool" data-step="1" title="前往下一页" aria-label="前往下一页">'+hubIcon('chevron',16)+'</button>';
  $('.route-path').before(historyTools);
  historyTools.onclick=e=>{const b=e.target.closest('[data-step]');if(!b||b.disabled)return;cursor+=Number(b.dataset.step);const {page,name}=historyLog[cursor];showHub(page,name,true)};
  const sheet=$('#sheet'), reduced=()=>document.documentElement.dataset.reduce==='on'||matchMedia('(prefers-reduced-motion:reduce)').matches;
  let theme=matchMedia('(prefers-color-scheme:dark)').matches?'dark':'light';
  function setTheme(next){theme=next;document.documentElement.dataset.theme=theme;$('#theme').innerHTML=hubIcon(theme==='dark'?'sun':'moon');$('#theme').setAttribute('aria-pressed',String(theme==='dark'))}
  setTheme(theme);$('#prefs').innerHTML=hubIcon('settings');$('#buddy').innerHTML=hubIcon('chat',25);$('#sheetClose').innerHTML=hubIcon('close');
  window.showHub=(page,client,fromHistory=false)=>{if(page==='client')window.demoClient=client||window.demoClient||clients[0];current=page;const name=page==='client'?window.demoClient:names[page];$('#crumb').textContent=name;
    if(!fromHistory&&historyLog[cursor]?.name!==name){historyLog.splice(++cursor);historyLog.push({page,name})}
    historyTools.firstElementChild.disabled=cursor<1;historyTools.lastElementChild.disabled=cursor>=historyLog.length-1;
    document.querySelectorAll('.navbtn').forEach(b=>{const on=b.dataset.page===page&&(page!=='client'||b.dataset.name===name);if(on)b.setAttribute('aria-current','page');else b.removeAttribute('aria-current')});
    const selected=$('#nav .navbtn[aria-current="page"]');if(selected?.closest('.navfold').inert)fold(selected.closest('.navgroup').querySelector('.fold-toggle'),true);
    railSelection();$('#rail').classList.remove('open');$('#menu').setAttribute('aria-expanded','false');HubDemo.render(page);$('#scroller').scrollTop=0;
    document.dispatchEvent(new CustomEvent('hub:navigate',{detail:{page,name}}));
    navAnim?.cancel();if(!reduced())navAnim=$('#view').animate([{opacity:.9,transform:'translateY(2px)'},{opacity:1,transform:'none'}],{duration:145,easing:'ease-out'});
  };
  $('#nav').onclick=e=>{const header=e.target.closest('.fold-toggle');if(header){fold(header,header.getAttribute('aria-expanded')!=='true');railSelection();return}const b=e.target.closest('.navbtn');if(!b)return;if(b.getAttribute('aria-current')==='page'){$('#rail').classList.remove('open');$('#menu').setAttribute('aria-expanded','false');return}showHub(b.dataset.page,b.dataset.name)};
  $('#menu').onclick=()=>{const open=$('#rail').classList.toggle('open');$('#menu').setAttribute('aria-expanded',String(open));if(open)railSelection()};
  $('#theme').onclick=()=>setTheme(theme==='dark'?'light':'dark');
  window.hubSheet=(title,html)=>{clearTimeout(closeTimer);sheet.classList.remove('closing');if(!sheet.open)currentSheetFocus=document.activeElement;$('#sheetTitle').textContent=title;$('#sheetBody').innerHTML=html;if(!sheet.open)sheet.showModal();return sheet};
  window.closeHubSheet=()=>{if(!sheet.open||sheet.classList.contains('closing'))return;sheet.classList.add('closing');closeTimer=setTimeout(()=>{sheet.close();sheet.classList.remove('closing');currentSheetFocus?.focus?.()},reduced()?0:115)};
  $('#sheetClose').onclick=closeHubSheet;sheet.oncancel=e=>{e.preventDefault();closeHubSheet()};sheet.onclick=e=>{if(e.target===sheet){const r=sheet.getBoundingClientRect();if(e.clientX<r.left||e.clientX>r.right||e.clientY<r.top||e.clientY>r.bottom)closeHubSheet()}};
  $('#prefs').onclick=()=>{const s=hubSheet('预览设置','<p class="subtle">只调整此样稿的外观。</p><label class="prefrow">深色外观<input id="dark" type="checkbox" role="switch"></label><label class="prefrow">减少动态效果<input id="red" type="checkbox" role="switch"></label><div class="sheetactions"><button type="button" class="quiet" id="demo-error">预览错误提示</button></div>');const dark=s.querySelector('#dark'),red=s.querySelector('#red');s.querySelector('#demo-error').onclick=()=>{s.addEventListener('close',()=>hubToast('连接超时。请检查网络后重试。',true),{once:true});closeHubSheet()};dark.checked=theme==='dark';red.checked=reduced();red.disabled=matchMedia('(prefers-reduced-motion:reduce)').matches;dark.onchange=()=>setTheme(dark.checked?'dark':'light');red.onchange=()=>document.documentElement.dataset.reduce=red.checked?'on':'off'};
  document.addEventListener('keydown',e=>{if(e.key==='Escape'&&!sheet.open){$('#rail').classList.remove('open');$('#menu').setAttribute('aria-expanded','false')}});
  showHub('overview');
})();
