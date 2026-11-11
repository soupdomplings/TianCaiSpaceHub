'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const http = require('node:http');
const { once } = require('node:events');
const Module = require('node:module');
const bridge = require('./host-bridge.cjs');

const tick = () => new Promise(resolve => setImmediate(resolve));
function fixture() {
  let session = { account: { uid: 'fixture-account', enterpriseId: 'fixture-org', displayName: 'Test account' },
    auth: { accessToken: 'fixture-real-account-secret', refreshToken: 'fixture-refresh-secret' } };
  let snapshot = { freshness: 'fresh', configuration: { models: [{ id: 'fixture-model', name: 'Fixture',
    contextWindow: 128000, maxInputTokens: 100000, maxOutputTokens: 8000, supportsToolCall: true,
    supportsImages: false, supportsReasoning: true, url: 'https://secret.invalid', apiKey: 'fixture-upstream-secret' }] } };
  const listeners = new Set();
  let instance;
  class Native {
    constructor(deps) { this.deps = deps; this.closed = false; this.releases = 0; this.identity = deps.identity(); instance = this; }
    syncIdentity() { if (this.identity !== this.deps.identity()) { this.identity = this.deps.identity(); this.releases++; } }
    releaseSession() { this.releases++; }
    async resolveEnv() { return { WORKBUDDY_VLM_CHAT_URL: 'http://127.0.0.1:12345/skills/vlm/chat',
      WORKBUDDY_VLM_AUTH_TOKEN: 'fixture-local-ledger', WORKBUDDY_VLM_CONTEXT: 'fixture-signed-context' }; }
    async close() { this.closed = true; this.releases++; }
  }
  const deps = { identity: () => bridge.identity(session) };
  const native = new Native(deps);
  const host = { authenticationManager: { currentSessionSubject: {
    getValue: () => session, subscribe: listener => { listeners.add(listener); return { unsubscribe: () => listeners.delete(listener) }; }
  } }, coordinator: { getConfirmedRemoteModelSnapshot: () => snapshot } };
  return { native, host, get instance() { return instance; },
    setSnapshot: value => { snapshot = value; },
    setSession: value => { session = value; for (const listener of listeners) listener(); } };
}
async function receiver() {
  const reports = [];
  let accept = true;
  const server = http.createServer((request, response) => {
    assert.equal(request.headers.authorization, 'Bearer fixture-lease');
    let bytes = '';
    request.setEncoding('utf8'); request.on('data', chunk => { bytes += chunk; });
    request.on('end', () => { reports.push(JSON.parse(bytes)); response.writeHead(accept ? 204 : 403); response.end(); });
  });
  server.listen(0, '127.0.0.1'); await once(server, 'listening');
  return { options: { callback: new URL(`http://127.0.0.1:${server.address().port}/`), lease: 'fixture-lease', bridgeId: 'fixture-bridge' },
    reports, reject: () => { accept = false; }, close: () => new Promise(resolve => server.close(resolve)) };
}
async function until(check) {
  for (let index = 0; index < 100; index++) { if (check()) return; await new Promise(resolve => setTimeout(resolve, 10)); }
  assert.fail('fixture state did not settle');
}

test('current fresh directory projects only public capability metadata and closes its own lease', async t => {
  const endpoint = await receiver(); const f = fixture();
  const owned = bridge.attach(f.native, f.host, endpoint.options);
  t.after(async () => { await owned.close(); await endpoint.close(); });
  await until(() => endpoint.reports.length === 1);
  const first = endpoint.reports[0];
  assert.equal(first.state, 'ready'); assert.equal(first.models.length, 1);
  assert.equal(first.models[0].contextWindow, 128000);
  assert.equal(first.reference.token, 'fixture-local-ledger');
  assert.notEqual(f.instance, f.native);
  assert.equal(f.instance.deps, f.native.deps);
  const serialized = JSON.stringify(first);
  for (const secret of ['fixture-real-account-secret', 'fixture-refresh-secret', 'fixture-upstream-secret', 'secret.invalid']) assert(!serialized.includes(secret));
  assert.deepEqual(bridge.projectModel({ id: 'input-only', maxInputTokens: 100000 }), { id: 'input-only', name: 'input-only', contextWindow: undefined, maxOutputTokens: undefined });

  f.setSnapshot(null); await owned.publish();
  await until(() => endpoint.reports.at(-1)?.state === 'catalog-pending');
  assert.deepEqual(endpoint.reports.at(-1).models, []);
  assert.equal(endpoint.reports.at(-1).reference, undefined);
  f.setSession(undefined); await until(() => endpoint.reports.at(-1)?.state === 'login-required');
  assert.equal(endpoint.reports.at(-1).accountId, null);
  assert(f.instance.releases > 0);
  endpoint.reject(); await owned.publish();
  await until(() => f.instance.closed);
  assert.equal(f.instance.closed, true);
  assert.equal(f.native.closed, false);
});

test('compile hook is exact-path only and refuses duplicate host anchors', async () => {
  const endpoint = await receiver();
  const previous = Module.prototype._compile;
  const fakePath = require('node:path').resolve('target/workbuddy-fixture/server.js');
  try {
    bridge.install({ ...endpoint.options, server: fakePath });
    const elsewhere = new Module('unrelated'); elsewhere._compile('module.exports = 42;', fakePath + '.other');
    assert.equal(elsewhere.exports, 42);
    assert.notEqual(Module.prototype._compile, previous);
    // Comments contain the anchors, so executing this synthetic module never
    // loads a real vendor module or opens WorkBuddy.
    const wrong = new Module('duplicate'); wrong._compile(`/*${bridge.ANCHOR}${bridge.ANCHOR}*/module.exports=7;`, fakePath);
    assert.equal(wrong.exports, 7);
    assert.equal(Module.prototype._compile, previous);
    await until(() => endpoint.reports.length === 1);
    assert.equal(endpoint.reports[0].state, 'unsupported-host');
  } finally { Module.prototype._compile = previous; await endpoint.close(); }
  await tick();
});
