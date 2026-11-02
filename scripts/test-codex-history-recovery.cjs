'use strict';
// Offline tests only. No Codex/Hub process, app-server or model requests.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { test } = require('node:test');
const root = path.resolve(__dirname, '..');
const runtimeSource = fs.readFileSync(path.join(root, 'assets/codex-repair/history-recovery.js'), 'utf8');
const adapt = vm.runInNewContext(fs.readFileSync(path.join(root, 'assets/codex-repair/history-adapter.js'), 'utf8'));
const blocked = () => ({ status: 'not-ready', reason: 'execution-config-loading' });
const flush = async () => { for (let n = 0; n < 25; n++) await Promise.resolve(); };

function setup(options = {}) {
  let now = 0, id = 0, valid = true, activityRoute = '/thread/a', listener, unsubscribed = 0;
  const timers = new Map();
  const context = vm.createContext({
    __CODEXHUB_ENHANCED_MODE__: { historyRecoveryEnabled: options.enabled !== false, modelControlsEnabled: false },
    location: { href: 'app://-/thread/a' },
    Date: { now: () => now },
    setInterval(fn, ms) { timers.set(++id, { fn, ms, at: now + ms }); return id; },
    clearInterval(id) { timers.delete(id); },
  });
  vm.runInContext(runtimeSource, context);
  const manager = {
    getHostId: () => options.host || 'local',
    getWindowActivity: () => ({ routePath: activityRoute }),
    settings: { subscribeExecutionSettingsChanged(fn) { listener = fn; return () => { listener = null; unsubscribed++; }; } },
    logger: { info() {} },
  };
  const api = context.__CODEXHUB_HISTORY_RECOVERY__;
  return { context, api, manager, timers,
    args: { manager, conversationId: 'a', source: options.source || 'view', isCurrent: () => valid },
    signal() { listener?.(); },
    cancel() { valid = false; },
    navigate() { activityRoute = '/thread/b'; context.location.href = 'app://-/thread/b'; },
    unsubscribed: () => unsubscribed,
    async advance(ms) {
      await flush(); const target = now + ms;
      for (;;) {
        const due = [...timers.values()].filter(t => t.at <= target).sort((a, b) => a.at - b.at)[0];
        if (!due) break;
        now = due.at; due.at += due.ms; due.fn(); await flush();
      }
      now = target; await flush();
    },
  };
}

test('delayed configuration automatically resumes the same preparation with original model/permissions', async () => {
  const t = setup(); let ready = false, calls = 0;
  const input = { model: 'synthetic-model', permissions: { approval_policy: 'on-request' }, workspaceRoots: ['synthetic-workspace'] };
  const success = { start: { model: input.model }, requestOptions: { priority: 'critical' } };
  const product = { async prepare(args) { calls++; assert.equal(args, input); return ready ? success : blocked(); }, complete: true };
  const wrapped = t.api.product(product, t.args);
  const promise = wrapped.prepare(input); await flush();
  assert.equal(t.api.stats.waiting, 1); assert.equal(wrapped.complete, true);
  ready = true; t.signal(); await flush();
  assert.equal(await promise, success); assert.equal(calls, 2);
  assert.equal(t.api.stats.recovered, 1); assert.equal(t.api.stats.waiting, 0);
  assert.equal(t.unsubscribed(), 1); assert.equal(t.timers.size, 0);
});

test('poll fallback works without an execution-settings subscription', async () => {
  const t = setup(); t.manager.settings = {}; let calls = 0;
  const p = t.api.product({ prepare: async () => ++calls < 3 ? blocked() : { start: {} } }, t.args).prepare({});
  await t.advance(1000); assert.ok((await p).start); assert.equal(calls, 3); assert.equal(t.timers.size, 0);
});

test('persistent configuration loading stops at 15 seconds and a later explicit retry can succeed', async () => {
  const t = setup(); let ready = false, calls = 0;
  const product = { prepare: async () => { calls++; return ready ? { start: {} } : blocked(); } };
  const wrapped = t.api.product(product, t.args); const p = wrapped.prepare({});
  await t.advance(15000); assert.equal((await p).reason, 'execution-config-loading');
  assert.ok(calls <= 31); const stoppedCalls = calls;
  await t.advance(20000); assert.equal(calls, stoppedCalls);
  assert.equal(t.api.stats.timedOut, 1); assert.equal(t.timers.size, 0);
  ready = true; assert.ok((await wrapped.prepare({})).start);
});

test('a subsequent hung preparation is also bounded, without an unhandled rejection or late resume', async () => {
  const t = setup(); let calls = 0, rejectRead;
  const p = t.api.product({ prepare: async () => ++calls === 1 ? blocked() : new Promise((_, reject) => { rejectRead = reject; }) }, t.args).prepare({});
  await t.advance(15000); assert.equal((await p).reason, 'execution-config-loading');
  rejectRead(Error('late read failure')); await flush();
  assert.equal(t.api.stats.recovered, 0); assert.equal(t.timers.size, 0);
});

for (const mode of ['navigate', 'cancel']) test(`${mode} cancels waiting before another preparation or resume`, async () => {
  const t = setup(); let calls = 0;
  const p = t.api.product({ prepare: async () => { calls++; return blocked(); } }, t.args).prepare({});
  await flush(); t[mode](); await t.advance(100);
  assert.equal((await p).reason, 'canceled'); assert.equal(calls, 1);
  assert.equal(t.api.stats.canceled, 1); assert.equal(t.unsubscribed(), 1); assert.equal(t.timers.size, 0);
});

test('navigation during an outstanding preparation cannot publish its late success', async () => {
  const t = setup(); let calls = 0, resolveRead;
  const p = t.api.product({ prepare: async () => ++calls === 1 ? blocked() : new Promise(resolve => { resolveRead = resolve; }) }, t.args).prepare({});
  await t.advance(500); t.navigate(); resolveRead({ start: {} }); await flush();
  assert.equal((await p).reason, 'canceled'); assert.equal(t.api.stats.recovered, 0);
});

test('identical active conversation preparations share one promise and clean up after completion', async () => {
  const t = setup(); let ready = false, calls = 0;
  const product = { prepare: async () => { calls++; return ready ? { start: {} } : blocked(); } };
  const one = t.api.product(product, t.args).prepare({});
  const two = t.api.product(product, t.args).prepare({});
  assert.equal(one, two); await flush(); ready = true; t.signal();
  await one; assert.equal(calls, 2);
  await t.api.product(product, t.args).prepare({}); assert.equal(calls, 3);
});

for (const options of [{ enabled: false }, { host: 'durable' }, { host: 'remote-control:fixture' }, { source: 'executor' }, { source: 'recovery' }]) {
  test(`unmanaged/remote/executor preparation stays native: ${JSON.stringify(options)}`, () => {
    const t = setup(options), product = { prepare: async () => blocked() };
    assert.equal(t.api.product(product, t.args), product); assert.equal(t.timers.size, 0);
  });
}

test('unrelated not-ready reasons and ordinary exceptions are never retried', async () => {
  const t = setup(); let calls = 0;
  const failure = Error('EBADF');
  const p = t.api.product({ prepare: async () => { calls++; throw failure; } }, t.args);
  await assert.rejects(p.prepare({}), error => error === failure);
  const reason = { status: 'not-ready', reason: 'inactive-window' };
  assert.equal(await t.api.product({ prepare: async () => reason }, t.args).prepare({}), reason);
  assert.equal(calls, 1); assert.equal(t.timers.size, 0);
});

test('thrown execution-config-loading preserves its original error after timeout', async () => {
  const t = setup(), error = Object.assign(Error('App-server task is not ready: execution-config-loading'), { result: blocked() });
  const p = t.api.product({ prepare: async () => { throw error; } }, t.args).prepare({});
  const rejected = assert.rejects(p, actual => actual === error);
  await t.advance(15000); await rejected; assert.equal(t.api.stats.timedOut, 1);
});

test('page requests coalesce, retain cursor and anchor on failure, and permit a later retry', async () => {
  const t = setup(); let calls = 0, fail = true;
  const state = { cursor: 'older-page', anchor: 120, turns: ['recent'] };
  const load = async () => { calls++; if (fail) throw Error('temporary'); state.turns.unshift('older'); state.cursor = 'next-page'; return 'continue'; };
  const context = { hostId: 'local', conversationId: 'a' };
  const one = t.api.page(context, load), two = t.api.page(context, load);
  assert.equal(one, two); await assert.rejects(one);
  assert.deepEqual(state, { cursor: 'older-page', anchor: 120, turns: ['recent'] });
  fail = false; let clearCount = 0;
  const retry = () => t.api.retry(context, () => clearCount++, () => t.api.page(context, load));
  const three = retry(), four = retry(); assert.equal(three, four);
  await three; assert.equal(clearCount, 1); assert.equal(calls, 2);
  assert.deepEqual(state, { cursor: 'next-page', anchor: 120, turns: ['older', 'recent'] });
});

const resumeFixture = 'function resume(e,t){let r=false,a=()=>true;let p=Promise.resolve().then(()=>hydrate({manager:this,hostMode:this.runtime.threadMode(this.hostId),threadRecognition:this.threadRecognition,workspace:this.projectlessConversations,params:e,product:product,historyPolicy:this.history,requireBoundedHistory:t===`follow-up`,schedule:this.scheduler.schedule,isReconnectRecovery:r,canAcquireOwnership:a,isCurrentResumeAttempt:()=>!this.disposed&&this.inFlightConversationResumes.get(e.conversationId)===m,setHydrationCleanup:c=>{cleanup=c}})),m={promise:p};this.inFlightConversationResumes.set(e.conversationId,m);return p}';
const pageFixture = 'const mt=async()=>{try{let host=scope.get(hostAtom,thread);await manager(scope,host).loadOlderConversationHistoryPage(thread,[],onPage,permissions(host),trace?.trace)}catch(error){let cause=error;return log.warning(`Failed to load older thread history`,{safe:{conversationId:thread},sensitive:{error:cause}}),oldError(thread),`stop`}return`continue`};const button={onPrimaryCtaClick:()=>{status===`error`?window.location.reload():setError(null)},primaryCtaColor:`ghostActive`,primaryCtaText:jsx(k,{id:`localConversation.retryHistoryLoad`})};const props={onLoadOlderHistory:mt,composerPresentation:presentation,composerSubmitDisabled:disabled||busy,dotTaskKey:hasKey?taskKey:void 0,conversationId:thread,hostId:hostId,isThreadHistoryLoading:loading};';

test('resume adaptation preserves native attempt identity and the original request boundary', async () => {
  const t = setup(), adapted = adapt(resumeFixture);
  assert.equal(adapted.historyResume, true); let ready = false, calls = 0, resumed = 0;
  Object.assign(t.manager, { runtime: { threadMode: () => 'local' }, hostId: 'local', scheduler: { schedule() {} }, inFlightConversationResumes: new Map() });
  Object.assign(t.context, { product: { prepare: async () => { calls++; return ready ? { start: {} } : blocked(); } },
    hydrate: async ({ product, params }) => { const result = await product.prepare(params); if (!result.status) resumed++; return result; } });
  const resume = vm.runInContext(adapted.source + ';resume', t.context);
  const p = resume.call(t.manager, { conversationId: 'a', model: 'fixture' }, 'view');
  await flush(); assert.equal(resumed, 0); ready = true; t.signal(); await p;
  assert.equal(calls, 2); assert.equal(resumed, 1);
});

test('page adaptation exposes partial-history failure and invokes the actual loader from Retry without reloading', async () => {
  const t = setup(); let failed = true, errorState = null, calls = 0, reloads = 0, oldErrors = 0;
  const adapted = adapt(pageFixture); assert.equal(adapted.historyRetry, true);
  Object.assign(t.context, { scope: { get: () => 'local' }, hostAtom: {}, thread: 'a', hostId: 'local', onPage() {}, permissions: () => ({}), trace: null,
    manager: () => ({ async loadOlderConversationHistoryPage() { calls++; if (failed) throw Error('temporary'); } }),
    log: { warning() {} }, oldError() { oldErrors++; }, setError(value) { errorState = value; },
    status: 'ready', window: { location: { reload() { reloads++; } } }, jsx: () => null, k: {}, presentation: null, disabled: false, busy: false, hasKey: false, taskKey: null, loading: false });
  const api = vm.runInContext(adapted.source + ';({mt,button})', t.context);
  assert.equal(await api.mt(), 'stop'); assert.equal(errorState, 'a'); assert.equal(oldErrors, 0);
  failed = false; api.button.onPrimaryCtaClick(); await flush();
  assert.equal(calls, 2); assert.equal(errorState, null); assert.equal(reloads, 0);
  t.context.status = 'error'; api.button.onPrimaryCtaClick(); assert.equal(reloads, 1);
});

test('a page failure after navigation cannot overwrite the new conversation error', async () => {
  const t = setup(); let rejectPage, errorState = null;
  Object.assign(t.context, { scope: { get: () => 'local' }, hostAtom: {}, thread: 'a', hostId: 'local', onPage() {}, permissions: () => ({}), trace: null,
    manager: () => ({ loadOlderConversationHistoryPage: () => new Promise((_, reject) => { rejectPage = reject; }) }),
    log: { warning() {} }, oldError() {}, setError(value) { errorState = value; },
    status: 'ready', window: { location: { reload() {} } }, jsx: () => null, k: {}, presentation: null, disabled: false, busy: false, hasKey: false, taskKey: null, loading: false });
  const load = vm.runInContext(adapt(pageFixture).source + ';mt', t.context);
  const pending = load(); await flush(); t.navigate(); errorState = 'new-conversation-error';
  rejectPage(Error('late history failure')); assert.equal(await pending, 'stop');
  assert.equal(errorState, 'new-conversation-error');
  assert.equal(t.api.showError({ result: { reason: 'canceled' } }), false);
});

for (const mode of ['disabled', 'remote']) test(`page adapter preserves native ${mode} error and retry behavior`, async () => {
  const t = setup({ enabled: mode !== 'disabled' }); let nativeError = 0, errorState = undefined, calls = 0;
  Object.assign(t.context, { scope: { get: () => mode === 'remote' ? 'durable' : 'local' }, hostAtom: {}, thread: 'a', hostId: mode === 'remote' ? 'durable' : 'local', onPage() {}, permissions: () => ({}), trace: null,
    manager: () => ({ async loadOlderConversationHistoryPage() { calls++; throw Error('failure'); } }),
    log: { warning() {} }, oldError() { nativeError++; }, setError(value) { errorState = value; },
    status: 'ready', window: { location: { reload() {} } }, jsx: () => null, k: {}, presentation: null, disabled: false, busy: false, hasKey: false, taskKey: null, loading: false });
  const api = vm.runInContext(adapt(pageFixture).source + ';({mt,button})', t.context);
  await api.mt(); assert.equal(nativeError, 1); assert.equal(errorState, undefined);
  api.button.onPrimaryCtaClick(); await flush(); assert.equal(errorState, null); assert.equal(calls, 1);
});

test('unknown, ambiguous or mismatched source layouts are preserved; adaptation is idempotent', () => {
  for (const source of ['export const unknown=true;', resumeFixture + resumeFixture, pageFixture + pageFixture,
    resumeFixture.replace('get(e.conversationId)', 'get(other.conversationId)'),
    pageFixture.replace('conversationId:thread},sensitive', 'conversationId:other},sensitive'),
    pageFixture.replace('setError(null)', 'unknown(null,null)')]) {
    const result = adapt(source); assert.equal(result.source, source); assert.equal(result.historyResume || result.historyRetry, false);
  }
  for (const source of [resumeFixture, pageFixture]) { const once = adapt(source); assert.equal(adapt(once.source).source, once.source); }
});

test('runtime installation is idempotent and keeps active waits across reinjection', () => {
  const t = setup(), previous = t.api; vm.runInContext(runtimeSource, t.context);
  assert.equal(t.context.__CODEXHUB_HISTORY_RECOVERY__, previous);
});
