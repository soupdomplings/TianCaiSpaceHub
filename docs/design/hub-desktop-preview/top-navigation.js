/* A single surface stays attached to the active disclosure while its content changes. */
(() => {
  'use strict';
  const root = document.querySelector('.top-navigation'), parent = document.getElementById('crumb-parent');
  const entries = [...document.querySelectorAll('#nav .navbtn[data-page]')];
  const group = key => key === 'client' ? 'clients' : 'space';
  const groups = [
    {id: 'space', name: '工作空间', width: 212},
    {id: 'clients', name: '客户端', width: 244}
  ];
  const byId = id => document.getElementById(id);
  const track = (node, target) => window.hubTrack(node, target);
  const strip = document.createElement('div');
  strip.className = 'top-strip';
  strip.dataset.track = '';
  strip.setAttribute('role','group');
  strip.setAttribute('aria-label','按分组前往页面');
  const fly = document.createElement('div');
  fly.id = 'top-panel';
  fly.className = 'top-panel';
  fly.inert = true;
  fly.innerHTML = '<div class="top-glass"></div>';
  const glass = fly.firstElementChild;
  glass.setAttribute('role','group');
  const triggers = new Map(), lists = new Map();
  groups.forEach(({id, name}) => {
    const t = document.createElement('button');
    t.type = 'button'; t.className = 'top-trigger'; t.id = 'top-nav-' + id;
    t.dataset.topGroup = id;
    t.setAttribute('aria-expanded','false'); t.setAttribute('aria-controls', fly.id);
    t.setAttribute('aria-label','浏览'+name); t.title='浏览'+name;
    t.innerHTML = '<i class="top-group-icon" aria-hidden="true">'+window.hubIcon(id==='space'?'overview':'client',15)+'</i><span>'+name+'</span>'+window.hubIcon('chevron',12);
    strip.append(t); triggers.set(id,t);
    const list = document.createElement('div');
    list.className = 'top-list'; list.dataset.topList = id; list.dataset.track = ''; list.inert = true;
    entries.filter(e => group(e.dataset.page) === id).forEach(e => {
      const b = document.createElement('button');
      b.type = 'button'; b.className = 'top-entry'; b.dataset.topPage = e.dataset.page; b.dataset.topName = e.dataset.name;
      const glyph = document.createElement('i');
      glyph.className = 'top-glyph'; glyph.setAttribute('aria-hidden','true');
      glyph.innerHTML = e.querySelector('.navglyph')?.innerHTML || window.hubIcon('folder');
      b.append(glyph, document.createTextNode(e.dataset.name)); list.append(b);
    });
    glass.append(list); lists.set(id,list);
  });
  root.replaceChildren(strip, fly);
  let opened = null, clicked = false, leaveTimer;
  const currentGroup = () => group(document.querySelector('#nav [aria-current="page"]')?.dataset.page);
  function updateCurrent() {
    const currentPage = document.querySelector('#nav .navbtn[aria-current="page"]');
    parent.textContent = currentPage.closest('.navgroup').querySelector('.fold-toggle').textContent;
    for (const [key, list] of lists) {
      let selected;
      for (const row of list.querySelectorAll('.top-entry')) {
        if (row.dataset.topName === currentPage?.dataset.name) {
          selected = row; row.setAttribute('aria-current','page');
        } else row.removeAttribute('aria-current');
      }
      track(list, selected || null);
    }
    track(strip,triggers.get(opened || currentGroup()));
  }
  function place() {
    if (!opened) return;
    const trigger = triggers.get(opened).getBoundingClientRect(), base = root.getBoundingClientRect();
    const bounds = root.closest('.window').getBoundingClientRect();
    const width = Math.min(groups.find(g=>g.id===opened).width, bounds.width - 20);
    const x = Math.max(bounds.left + 10, Math.min(trigger.left, bounds.right - 10 - width)) - base.left;
    const list = lists.get(opened);
    fly.style.setProperty('--panel-x', x + 'px');
    fly.style.setProperty('--panel-width', width + 'px');
    fly.style.setProperty('--panel-height', Math.max(0, Math.min(list.scrollHeight + 2, bounds.bottom - base.bottom - 16)) + 'px');
    fly.style.setProperty('--panel-origin', (trigger.left + trigger.width / 2 - base.left - x) + 'px');
  }
  function open(id) {
    clearTimeout(leaveTimer);
    if (opened === id) return;
    if (opened && lists.get(opened).contains(document.activeElement)) triggers.get(id).focus({preventScroll:true});
    const entering = opened === null && getComputedStyle(fly).visibility === 'hidden';
    opened = id;
    clicked = false;
    fly.inert = false;
    glass.setAttribute('aria-labelledby',triggers.get(id).id);
    for (const [key,t] of triggers) {
      const on = key === id;
      t.setAttribute('aria-expanded', String(on));
      const list = lists.get(key);
      list.inert = !on;
      list.classList.toggle('shown',on);
    }
    if (entering) fly.classList.add('at-anchor');
    place();
    if (entering) {fly.getBoundingClientRect();fly.classList.remove('at-anchor')}
    root.classList.add('show-panel');
    updateCurrent();
  }
  function close(returnFocus=false) {
    clearTimeout(leaveTimer);
    if (!opened) return;
    const t=triggers.get(opened);
    if (returnFocus || fly.contains(document.activeElement)) t.focus({preventScroll:true});
    t.setAttribute('aria-expanded','false');
    opened = null; clicked = false; fly.inert = true;
    root.classList.remove('show-panel');
    track(strip,triggers.get(currentGroup()));
  }
  function focusRow(idx=0) {
    const items=[...lists.get(opened).querySelectorAll('.top-entry')];
    items[(idx+items.length)%items.length]?.focus();
  }
  for (const [id,t] of triggers) {
    t.addEventListener('pointerenter',e=>{if(e.pointerType==='mouse') open(id)});
    t.addEventListener('focus',()=>{if(opened && opened!==id) open(id)});
    t.addEventListener('click',()=>{
      if (opened===id && clicked) { close(); return; }
      open(id); clicked=true;
    });
  }
  root.addEventListener('pointerenter',()=>clearTimeout(leaveTimer));
  root.addEventListener('pointerleave', e=>{
    if (e.pointerType==='mouse') leaveTimer=setTimeout(close,120);
  });
  root.addEventListener('focusout',e=>{if (!root.contains(e.relatedTarget) && opened) close()});
  document.addEventListener('pointerdown',e=>{if(!root.contains(e.target)) close()});
  window.addEventListener('blur',()=>close());
  document.addEventListener('keydown',e=>{
    if(e.key==='Escape' && opened) {
      e.preventDefault(); e.stopPropagation(); close(true); return;
    }
    if(!root.contains(e.target)) return;
    const t=e.target.closest('.top-trigger'), row=e.target.closest('.top-entry');
    if(!t && !row) return;
    if(e.key==='ArrowLeft'||e.key==='ArrowRight') {
      e.preventDefault();
      const i=groups.findIndex(g=>g.id===(t?.dataset.topGroup||opened));
      const next=groups[(i+(e.key==='ArrowRight'?1:-1)+groups.length)%groups.length].id;
      triggers.get(next).focus();
      if(opened) open(next);
    } else if(e.key==='ArrowDown'||e.key==='ArrowUp'||(row && (e.key==='Home'||e.key==='End'))) {
      e.preventDefault();
      if(t){open(t.dataset.topGroup);focusRow(e.key==='ArrowDown'?0:-1);return}
      const i=[...lists.get(opened).querySelectorAll('.top-entry')].indexOf(row);
      focusRow(e.key==='Home'?0:e.key==='End'?-1:i+(e.key==='ArrowDown'?1:-1));
    }
  },true);
  glass.addEventListener('click',e=>{
    const b=e.target.closest('.top-entry');
    if(!b) return;
    close();
    window.showHub(b.dataset.topPage,b.dataset.topName);
    byId('view').focus({preventScroll:true});
  });
  // The shell owns routing. This view only mirrors the current page.
  document.addEventListener('hub:navigate',()=>{close();updateCurrent()});
  window.addEventListener('resize',()=>{place();updateCurrent()});
  const resizeLists = new ResizeObserver(changes=>{
    for (const {target} of changes) track(target,target.querySelector('[aria-current="page"]'));
  });
  for (const list of lists.values()) resizeLists.observe(list);
  updateCurrent();
})();
