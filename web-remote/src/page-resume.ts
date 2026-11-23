/** A restored page may receive pageshow and visibilitychange for the same return. */
export function onPageResume(resume: () => void): () => void {
  let pending = document.hidden;
  let resumed = false;
  const leave = () => { pending = true; resumed = false; };
  const enter = (restored = false) => {
    if (document.hidden || (!pending && (!restored || resumed))) return;
    pending = false; resumed = true;
    resume();
  };
  const visibility = () => { if (document.hidden) leave(); else enter(); };
  const show = (event: PageTransitionEvent) => { if (event.persisted) enter(true); };
  const online = () => { leave(); enter(); };
  document.addEventListener('visibilitychange', visibility);
  window.addEventListener('pagehide', leave);
  window.addEventListener('pageshow', show);
  window.addEventListener('blur', leave);
  window.addEventListener('focus', visibility);
  window.addEventListener('online', online);
  return () => {
    document.removeEventListener('visibilitychange', visibility);
    window.removeEventListener('pagehide', leave);
    window.removeEventListener('pageshow', show);
    window.removeEventListener('blur', leave);
    window.removeEventListener('focus', visibility);
    window.removeEventListener('online', online);
  };
}

/** Poll in the foreground; a held request from before a freeze may never settle. */
export function pollCatalog(read: (signal: AbortSignal) => Promise<void | false>, interval = 15000): () => void {
  let pending: AbortController | undefined;
  let stopped = false;
  const refresh = (replace = false) => {
    if (stopped || document.hidden || (pending && !replace)) return;
    pending?.abort();
    const request = new AbortController();
    pending = request;
    void read(request.signal).then(result => {
      if (request === pending && result === false) stopped = true;
    }).finally(() => { if (request === pending) pending = undefined; });
  };
  refresh();
  const stopWatching = onPageResume(() => refresh(true));
  const timer = window.setInterval(() => { if (!document.hidden) refresh(); }, interval);
  return () => { stopWatching(); clearInterval(timer); pending?.abort(); };
}
