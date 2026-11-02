// Installed in memory by enhanced launch. No session/configuration writes.
(function installHubHistoryRecovery(root) {
  if (root.__CODEXHUB_HISTORY_RECOVERY__?.version === 1) return;
  const LIMIT_MS = 15000;
  const POLL_MS = 500;
  const pages = new Map();
  const retries = new Map();
  const preparations = new WeakMap();
  const obsoleteErrors = new WeakSet();
  const stats = { waiting: 0, recovered: 0, timedOut: 0, canceled: 0 };
  const enabled = () => root.__CODEXHUB_ENHANCED_MODE__?.historyRecoveryEnabled === true;
  const route = () => root.location?.href;
  const key = (context) => JSON.stringify([context.hostId, context.conversationId]);
  const loading = (value) => value?.status === 'not-ready' && value.reason === 'execution-config-loading';
  const canceled = () => ({ status: 'not-ready', reason: 'canceled' });

  async function recover(product, args, context) {
    const { manager, isCurrent } = context;
    const initialRoute = route();
    const initialActivityRoute = manager.getWindowActivity?.()?.routePath;
    const current = () => enabled() && isCurrent()
      && route() === initialRoute
      && manager.getWindowActivity?.()?.routePath === initialActivityRoute;
    const invoke = async () => {
      try { return { value: await product.prepare(args), thrown: false }; }
      catch (value) { return { value, thrown: true }; }
    };
    const unwrap = (outcome) => { if (outcome.thrown) throw outcome.value; return outcome.value; };
    let outcome = await invoke();
    if (!loading(outcome.thrown ? outcome.value?.result : outcome.value)) return unwrap(outcome);
    const deadline = Date.now() + LIMIT_MS;
    let wake = null, dispose;
    let attempts = 1;
    stats.waiting++;
    const finish = (reason) => {
      stats[reason]++;
      // Fixed fields only: no conversation text, paths, IDs or configuration.
      try { manager.logger?.info('hub_history_configuration_wait', { safe: { reason, attempts }, sensitive: {} }); } catch {}
    };
    // Bound both idle waits and subsequent preparation reads. Only the original
    // caller can consume a successful preparation and issue thread/resume.
    const bounded = (promise, pause = false) => new Promise((resolve) => {
      const due = Math.min(deadline, Date.now() + POLL_MS);
      let settled = false;
      const end = (result) => {
        if (settled) return;
        settled = true; clearInterval(timer); wake = null; resolve(result);
      };
      const check = () => {
        if (!current()) end({ stopped: 'canceled' });
        else if (Date.now() >= deadline) end({ stopped: 'timedOut' });
        else if (pause && Date.now() >= due) end({});
      };
      const timer = setInterval(check, 100);
      if (pause) wake = () => { check(); if (!settled) end({}); };
      else promise.then((value) => { check(); if (!settled) end({ outcome: value }); });
      check();
    });
    try {
      try { dispose = manager.settings?.subscribeExecutionSettingsChanged?.(() => wake?.()); } catch {}
      for (;;) {
        const wait = await bounded(null, true);
        if (wait.stopped) { finish(wait.stopped); return wait.stopped === 'canceled' ? canceled() : unwrap(outcome); }
        if (!current()) { finish('canceled'); return canceled(); }
        attempts++;
        const next = await bounded(invoke());
        if (next.stopped) { finish(next.stopped); return next.stopped === 'canceled' ? canceled() : unwrap(outcome); }
        outcome = next.outcome;
        if (!loading(outcome.thrown ? outcome.value?.result : outcome.value)) {
          if (!outcome.thrown && outcome.value?.status !== 'not-ready') finish('recovered');
          return unwrap(outcome);
        }
      }
    } finally { stats.waiting--; wake = null; try { dispose?.(); } catch {} }
  }

  root.__CODEXHUB_HISTORY_RECOVERY__ = {
    version: 1, limitMs: LIMIT_MS, stats,
    product(product, context) {
      if (!enabled() || context.source !== 'view' || context.manager.getHostId() !== 'local') return product;
      // The native manager already coalesces resumes. Preserve its attempt
      // identity, permission/model inputs, cleanup and thread/resume boundary.
      return { ...product, prepare(args) {
        let active = preparations.get(context.manager);
        if (!active) preparations.set(context.manager, active = new Map());
        const previous = active.get(context.conversationId);
        if (previous && previous.context.isCurrent() && context.isCurrent()) return previous.promise;
        const entry = { context, promise: null };
        entry.promise = recover(product, args, context).finally(() => {
          if (active.get(context.conversationId) === entry) active.delete(context.conversationId);
        });
        active.set(context.conversationId, entry);
        return entry.promise;
      } };
    },
    page(context, load) {
      if (!enabled() || context.hostId !== 'local') return load();
      const id = key(context), href = route(), previous = pages.get(id);
      if (previous?.href === href) return previous.promise;
      const entry = { href, promise: null };
      entry.promise = Promise.resolve().then(load).catch((error) => {
        if (route() !== href && error !== null && (typeof error === 'object' || typeof error === 'function')) obsoleteErrors.add(error);
        throw error;
      }).finally(() => {
        if (pages.get(id) === entry) pages.delete(id);
      });
      pages.set(id, entry);
      return entry.promise;
    },
    showError(error) {
      return !obsoleteErrors.has(error) && error?.result?.reason !== 'canceled';
    },
    retry(context, clearError, load) {
      const id = key(context);
      if (retries.has(id)) return retries.get(id);
      clearError();
      // Use the existing page loader and its cursor/scroll-anchor handling.
      // No reload, transcript replacement or automatic endless retry loop.
      const promise = Promise.resolve().then(load).finally(() => {
        if (retries.get(id) === promise) retries.delete(id);
      });
      retries.set(id, promise);
      return promise;
    },
  };
})(globalThis);
