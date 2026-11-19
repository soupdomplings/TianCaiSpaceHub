/* One moving surface per set of alternatives. The buttons remain in normal flow. */
(() => {
  'use strict';
  const states = new WeakMap();
  const sized = new Set();
  const pref = matchMedia('(prefers-reduced-motion: reduce)');
  const reduce = () => pref.matches || document.documentElement.dataset.reduce === 'on';

  function place(host, target, selecting) {
    let state = states.get(host);
    if (!state && !target) return;
    if (!state) {
      const mark = document.createElement('span');
      mark.className = 'selection-track';
      mark.setAttribute('aria-hidden', 'true');
      host.prepend(mark);
      state = {mark, target:null, anim:null};
      states.set(host, state);
      sized.add(host);
      layout.observe(host);
    }
    const {mark} = state;
    const oldTarget = state.target;
    state.target = target;
    if (!target || !target.isConnected || !target.getClientRects().length) {
      state.anim?.cancel(); mark.hidden = true; return;
    }
    const end = target.getBoundingClientRect(), base = host.getBoundingClientRect();
    if (!end.width || !end.height) { mark.hidden = true; return; }
    const moving = selecting && oldTarget && oldTarget !== target && !mark.hidden && !reduce();
    // Read the painted rectangle *before* cancelling an in-flight transition.
    const start = moving ? mark.getBoundingClientRect() : null;
    if ((selecting && oldTarget !== target) || reduce()) state.anim?.cancel();
    mark.hidden = false;
    Object.assign(mark.style, {
      left:(end.left - base.left - host.clientLeft + host.scrollLeft)+'px',
      top:(end.top - base.top - host.clientTop + host.scrollTop)+'px',
      width:end.width+'px', height:end.height+'px'
    });
    if (start) state.anim = mark.animate([
      {transform:'translate('+ (start.left-end.left) +'px,'+ (start.top-end.top) +'px) scale('+ (start.width/end.width) +','+ (start.height/end.height) +')'},
      {transform:'none'}
    ], {duration:235, easing:'cubic-bezier(.22,.72,.18,1)'});
  }

  window.hubTrack = (host,target) => { if(host) place(host,target,true); };
  function align() {
    for (const host of document.querySelectorAll('[data-track]')) {
      const state = states.get(host); if (state?.target) place(host,state.target,false);
    }
  }
  // Folding, group-panel resizing and zoom change geometry without changing selection.
  const layout = new ResizeObserver(align);
  ['.window','#rail','#view'].forEach(sel=>layout.observe(document.querySelector(sel)));
  document.addEventListener('hub:folds-ready',()=>document.querySelectorAll('.navfold').forEach(f=>layout.observe(f)));
  document.addEventListener('hub:navigate',()=>{for(const host of sized)if(!host.isConnected){layout.unobserve(host);sized.delete(host)}});
  document.fonts.ready.then(align);
  pref.addEventListener('change',align);
  new MutationObserver(align).observe(document.documentElement,{attributes:true,attributeFilter:['data-reduce']});
})();
