/* One result at a time, anchored inside the preview window, including when a sheet is open. */
(() => {
  'use strict';
  const frame = document.querySelector('.window'), sheet = document.getElementById('sheet');
  const toolbar = frame.querySelector('.toolbar');
  const island = document.createElement('div');
  island.id = 'toast';
  island.className = 'toast notice-island';
  island.hidden = true;
  const topLayer = typeof island.showPopover === 'function';
  if (topLayer) island.setAttribute('popover', 'manual');
  island.innerHTML = `<span class="notice-mark" aria-hidden="true"></span>
    <div class="notice-copy"><strong class="notice-heading" aria-hidden="true">操作未完成</strong><span class="notice-message" aria-atomic="true"></span></div>
    <button type="button" class="notice-close" aria-label="关闭提示" title="关闭提示">${window.hubIcon('close', 13)}</button>
    <span class="notice-countdown" aria-hidden="true"><i class="toast-timer"></i></span>`;
  frame.append(island);
  const mark = island.querySelector('.notice-mark'), message = island.querySelector('.notice-message');
  const copy = island.querySelector('.notice-copy'), close = island.querySelector('.notice-close');
  const bar = island.querySelector('.toast-timer'), motion = matchMedia('(prefers-reduced-motion: reduce)');
  const reduced = () => motion.matches || document.documentElement.dataset.reduce === 'on';
  const countdownCurve = (ms) => reduced() ? `steps(${Math.max(1, Math.ceil(ms / 1000))},end)` : 'linear';
  let active = false, lifetime, departure, duration = 0, focusedBefore, hovered = false;

  // Put a manual popover above native modal sheets, but keep it in their focus scope.
  // On older engines without popovers use normal window positioning or the sheet itself.
  function place() {
    if (!active) return;
    const parent = sheet.open ? sheet : frame;
    if (parent !== island.parentNode) {
      parent.append(island);
      hovered = false;
    }
    if (topLayer && !island.matches(':popover-open')) island.showPopover();
    if (!topLayer && sheet.open) { island.dataset.position = 'sheet'; return; }
    island.dataset.position = topLayer ? 'viewport' : 'window';
    const r = frame.getBoundingClientRect(), vp = window.visualViewport;
    const left = Math.max(r.left, vp?.offsetLeft ?? 0), right = Math.min(r.right, vp ? vp.offsetLeft + vp.width : innerWidth);
    const top = Math.max(r.top, vp?.offsetTop ?? 0), bottom = Math.min(r.bottom, vp ? vp.offsetTop + vp.height : innerHeight);
    const pad = Math.min(12, Math.max(2, (right - left) / 12)), x = (left + right) / 2;
    let y = top + Math.min(9, pad);
    island.style.setProperty('--notice-width', Math.max(0, right - left - 2 * pad) + 'px');
    const w = island.offsetWidth;
    // On narrower windows leave navigation controls visible beneath the pointer.
    const overlapsTools = [...toolbar.querySelectorAll('button, .route-path li, .sample')].some((tool) => {
      const b = tool.getBoundingClientRect();
      return b.width > 2 && b.height > 2 && b.top < y + 40 && b.bottom > y && b.right > x - w / 2 - 4 && b.left < x + w / 2 + 4;
    });
    if (overlapsTools) y = Math.max(y, Math.min(toolbar.getBoundingClientRect().bottom + 5, bottom - 75));
    island.style.setProperty('--notice-x', (topLayer ? x : x - r.left - frame.clientLeft) + 'px');
    island.style.setProperty('--notice-y', (topLayer ? y : y - r.top - frame.clientTop) + 'px');
    island.style.setProperty('--notice-height', Math.max(0, bottom - y - pad) + 'px');
    syncPause();
  }
  function syncPause() {
    if (!active || !lifetime) return;
    if (hovered || island.contains(document.activeElement) || document.hidden) lifetime.pause();
    else if (lifetime.playState === 'paused') lifetime.play();
  }
  function stopLifetime() {
    if (lifetime) { lifetime.onfinish = null; lifetime.cancel(); lifetime = null; }
  }
  function dismiss() {
    if (!active) return;
    active = false;
    stopLifetime();
    if (island.contains(document.activeElement)) {
      const focus = focusedBefore?.isConnected && (!sheet.open || sheet.contains(focusedBefore))
        ? focusedBefore : (sheet.open ? sheet.querySelector('button') : document.getElementById('view'));
      focus?.focus({preventScroll: true});
    }
    island.classList.remove('show');
    const hide = () => {
      if (topLayer && island.matches(':popover-open')) island.hidePopover();
      island.hidden = true;
      if (island.parentNode !== frame) frame.append(island);
      hovered = false;
    };
    if (reduced()) hide();
    else departure = setTimeout(hide, 135);
  }
  window.hubToast = (text, error = false) => {
    const content = String(text ?? '').trim() || (error ? '请重试。' : '操作已完成。');
    const first = island.hidden;
    clearTimeout(departure); stopLifetime();
    if (!island.contains(document.activeElement)) focusedBefore = document.activeElement;
    active = true; island.hidden = false;
    island.classList.toggle('error', !!error);
    mark.innerHTML = error
      ? '<svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor"><path d="M7.1 3h1.8l-.24 6.5H7.34z"/><circle cx="8" cy="12" r="1"/></svg>'
      : window.hubIcon('check', 14);
    message.setAttribute('role', error ? 'alert' : 'status');
    copy.tabIndex = error ? 0 : -1;
    place();
    message.textContent = content;
    place();
    if (first && !reduced()) void island.offsetWidth;
    island.classList.add('show');
    duration = error ? Math.max(8200, Math.min(24000, 3500 + content.length * 140)) : 3000;
    lifetime = bar.animate([{transform:'scaleX(1)'}, {transform:'scaleX(0)'}], {
      duration, easing:countdownCurve(duration), fill:'both'
    });
    lifetime.onfinish = dismiss;
    syncPause();
  };
  close.onclick = dismiss;
  island.addEventListener('pointerenter', (event) => { if (event.pointerType !== 'touch') { hovered = true; syncPause(); } });
  island.addEventListener('pointerleave', () => { hovered = false; syncPause(); });
  island.addEventListener('focusin', syncPause);
  island.addEventListener('focusout', () => queueMicrotask(syncPause));
  document.addEventListener('visibilitychange', syncPause);
  const updateMotion = () => lifetime?.effect.updateTiming({easing:countdownCurve(duration)});
  motion.addEventListener('change', updateMotion);
  new MutationObserver(updateMotion).observe(document.documentElement, {attributes:true, attributeFilter:['data-reduce']});
  new MutationObserver(place).observe(sheet, {attributes:true, attributeFilter:['open']});
  new ResizeObserver(place).observe(frame);
  window.addEventListener('resize', place);
  window.addEventListener('scroll', place, {passive:true});
  window.visualViewport?.addEventListener('resize', place);
  window.visualViewport?.addEventListener('scroll', place);
})();
