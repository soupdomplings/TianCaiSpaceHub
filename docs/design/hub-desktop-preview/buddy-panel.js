/* Local interaction sample: does not contact a model or modify user data. */
(() => {
  'use strict';
  const panel = document.getElementById('chatbox'), opener = document.getElementById('buddy');
  const shell = opener.closest('.window');
  shell.append(panel); // measure the half-width view against the Hub window, including its sidebar
  const glyph = (key,size) => window.hubIcon(key,size);
  const icon = (d,n=16) => '<svg aria-hidden="true" width="'+n+'" height="'+n+'" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="'+d+'"/></svg>';
  const symbols = {
    expand:icon('M14 4h6v6m0-6-7 7M4 14v6h6m-6 0 7-7'),
    restore:icon('M14 4v6h6m-6 0 6-6M4 14h6v6m0-6-6 6'),
    fold:icon('m5 9 7 7 7-7'), clear:icon('M5 7h14M9 7V4h6v3M7 7l1 13h8l1-13'),
    send:icon('M12 19V5m-6 7 6-7 6 7',17), stop:icon('M7 7h10v10H7z'),
    welcome:icon('M3.5 10.5a6.8 6.8 0 0 1 6.8-6.8h.9a6.8 6.8 0 0 1 0 13.6H8l-4.6 2 .8-5.2a6.4 6.4 0 0 1-.7-3.6Zm13.8-3c2.5.6 4.2 2.7 4.2 5.4 0 1.3-.4 2.5-1.1 3.4l.5 3.7-3.4-1.5h-2.1',26)
  };
  const tool = (id,label,content) => '<button class="buddy-tool" type="button" id="'+id+'" title="'+label+'" aria-label="'+label+'">'+content+'</button>';
  const actions = [
    ['models','查看模型','浏览已配置的模型与渠道','models'],
    ['repair','检查项目','找回项目里的会话','folder']
  ];
  const quick = full => actions.map(([action,label,detail,key])=>
    '<button type="button" data-act="'+action+'" aria-label="'+label+'">'+(full?'<i>'+glyph(key,18)+'</i>':'')+
    '<span><b>'+label+'</b>'+(full?'<small>'+detail+'</small>':'')+'</span>'+(full?glyph('chevron',14):'')+'</button>').join('');
  panel.innerHTML = '<header class="buddy-head"><span class="buddy-status"><i></i>在线</span><div class="buddy-controls">'+
    tool('buddy-clear','清空对话',symbols.clear)+tool('buddy-expand','展开至半窗',symbols.expand)+tool('foldchat','收起对话',symbols.fold)+'</div></header>'+
    '<div class="buddy-scroll" tabindex="0" aria-label="对话记录"><div class="buddy-start"><span class="buddy-mark">'+symbols.welcome+'</span>'+
    '<h2>想先做点什么？</h2><p>我可以陪你找回项目，也可以看看有哪些模型可用。</p><div id="suggest">'+quick(true)+'</div></div>'+
    '<div id="chatfeed" role="log" aria-label="消息" aria-live="polite"></div></div>'+
    '<div class="buddy-foot"><form id="composer"><textarea rows="2" aria-label="消息" placeholder="说说你想做的事…" maxlength="4000"></textarea>'+
    '<div class="buddy-compose-tools"><small>Enter 发送 · Shift + Enter 换行</small><button type="submit" aria-label="发送" title="发送">'+symbols.send+'</button></div></form></div>';
  const $ = q => panel.querySelector(q);
  const input = $('textarea'), form = $('#composer'), send = $('button[type="submit"]'), feed = $('#chatfeed');
  const welcome = $('.buddy-start'), scroll = $('.buddy-scroll'), expand = $('#buddy-expand'), clear = $('#buddy-clear');
  const reduce = () => document.documentElement.dataset.reduce==='on' || matchMedia('(prefers-reduced-motion:reduce)').matches;
  opener.setAttribute('aria-controls',panel.id); expand.setAttribute('aria-pressed','false'); clear.hidden=true;
  let displayed=false, composed=false, motion, replyTimer, pending, wasAtBottom=true;
  const bottom = () => scroll.scrollHeight-scroll.clientHeight-scroll.scrollTop < 26;
  function toggle(on) {
    if(on===displayed)return;
    const from=motion?getComputedStyle(panel):null;
    const start=from?{opacity:from.opacity,transform:from.transform}:null;
    motion?.cancel(); motion=null; displayed=on;
    opener.setAttribute('aria-expanded',String(on));
    opener.setAttribute('aria-label',on?'收起对话':'打开对话'); opener.title=on?'收起对话':'打开对话';
    if(on) {panel.hidden=false;panel.inert=false;input.focus({preventScroll:true});if(wasAtBottom)scroll.scrollTop=scroll.scrollHeight}
    else {wasAtBottom=bottom();opener.focus({preventScroll:true});panel.inert=true}
    const resting={opacity:1,transform:'none'}, tucked={opacity:0,transform:'translateY(9px) scale(.98)'};
    const finish=()=>{motion=null;if(!displayed)panel.hidden=true};
    if(reduce()){finish();return}
    motion=panel.animate([start||(on?tucked:resting),on?resting:tucked],{duration:on?205:125,easing:on?'cubic-bezier(.2,.8,.22,1)':'ease-in'});
    motion.onfinish=finish;
  }
  function sizeInput() {
    input.style.height='auto';
    input.style.height=Math.min(110,input.scrollHeight)+'px';
    send.disabled=!pending&&!input.value.trim();
  }
  function add(text,mine=false) {
    const follow=mine || (displayed?bottom():wasAtBottom);
    welcome.hidden=true;clear.hidden=false;
    const row=document.createElement('div');row.className='buddy-message '+(mine?'user':'reply');
    const body=document.createElement('p');body.textContent=text;row.append(body);feed.append(row);
    if(follow) {scroll.scrollTop=scroll.scrollHeight;wasAtBottom=true}
    return row;
  }
  function setBusy(on) {
    send.innerHTML=on?symbols.stop:symbols.send;send.title=on?'停止回复':'发送';send.setAttribute('aria-label',send.title);
    opener.classList.toggle('buddy-working',on); sizeInput();
  }
  function stop() {clearTimeout(replyTimer);pending?.remove();pending=null;setBusy(false)}
  function navigate(action) {
    if(action==='models') {
      window.showHub('models'); add('已打开「模型与渠道」，可以按厂商查看可用模型。');
      toggle(false);document.getElementById('view').focus({preventScroll:true});
    } else {
      window.showHub('client','ChatGPT / Codex');toggle(false);window.HubDemo.openRepair();
      add('已打开项目检查，可以勾选要放回原项目的会话。');
    }
  }
  function post(text,action) {
    if(pending)return;
    add(text,true);
    // Explicit quick actions are synchronous, so users can immediately operate the destination.
    if(action){navigate(action);return}
    pending=add('正在回复…');pending.classList.add('buddy-wait');setBusy(true);
    replyTimer=setTimeout(()=>{
      stop();
      const reply=/你好|您好|哈喽|hello/i.test(text)?'你好，在呢。今天想看看模型，还是检查项目里的会话？':
        '这个样稿可以带你查看模型或检查项目。选一个，我帮你打开。';
      const row=add(reply), choices=document.createElement('div');choices.className='buddy-choices';choices.innerHTML=quick(false);row.append(choices);
      if((displayed?bottom():wasAtBottom))scroll.scrollTop=scroll.scrollHeight;
    },460);
  }
  opener.onclick=()=>toggle(!displayed);
  $('#foldchat').onclick=()=>toggle(false);
  expand.onclick=()=>{
    const wide=panel.classList.toggle('wide');expand.setAttribute('aria-pressed',String(wide));
    expand.title=wide?'还原窗口':'展开至半窗';expand.setAttribute('aria-label',expand.title);
    expand.innerHTML=wide?symbols.restore:symbols.expand;
  };
  clear.onclick=()=>{
    stop();feed.replaceChildren();input.value='';sizeInput();welcome.hidden=false;clear.hidden=true;scroll.scrollTop=0;wasAtBottom=true;input.focus();
  };
  panel.addEventListener('click',e=>{
    const button=e.target.closest('button[data-act]');
    if(button&&!pending)post(actions.find(a=>a[0]===button.dataset.act)[1],button.dataset.act);
  });
  input.addEventListener('compositionstart',()=>{composed=true});input.addEventListener('compositionend',()=>{composed=false});
  input.addEventListener('input',sizeInput);
  input.addEventListener('keydown',e=>{
    if(e.key==='Enter'&&!e.shiftKey&&!e.isComposing&&!composed&&e.keyCode!==229){
      e.preventDefault();if(!pending)form.requestSubmit();
    }
  });
  form.onsubmit=e=>{
    e.preventDefault();if(pending){stop();add('已停止回复。');input.focus();return}
    const text=input.value.trim();if(!text||composed)return;
    input.value='';sizeInput();post(text,/项目|修复/.test(text)?'repair':/模型|渠道/.test(text)?'models':null);
  };
  document.addEventListener('keydown',e=>{
    if(e.key==='Escape' && displayed && !e.isComposing && !composed && !document.querySelector('dialog[open]') &&
      (panel.contains(document.activeElement)||document.activeElement===opener)){e.preventDefault();toggle(false)}
  });
  send.disabled=true;
})();
