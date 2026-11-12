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
  let snapshot = { freshness: 'fresh', configuration: { agents: [{ name: 'cli', tags: ['cli', 'default'], models: ['fixture-model'] }], models: [{ id: 'fixture-model', name: 'Fixture',
    contextWindow: 128000, maxInputTokens: 100000, maxOutputTokens: 8000, supportsToolCall: true,
    supportsImages: false, supportsReasoning: true, url: 'https://secret.invalid', apiKey: 'fixture-upstream-secret' }] } };
  const listeners = new Set();
  let instance, resolve;
  class Native {
    constructor(deps) { this.deps = deps; this.closed = false; this.releases = 0; this.identity = deps.identity(); instance = this; }
    syncIdentity() { if (this.identity !== this.deps.identity()) { this.identity = this.deps.identity(); this.releases++; } }
    releaseSession() { this.releases++; }
    async resolveEnv() { if (resolve) await resolve(); return { WORKBUDDY_VLM_CHAT_URL: 'http://127.0.0.1:12345/skills/vlm/chat',
      WORKBUDDY_VLM_AUTH_TOKEN: 'fixture-local-ledger', WORKBUDDY_VLM_CONTEXT: 'fixture-signed-context' }; }
    async close() { this.closed = true; this.releases++; }
  }
  const deps = { identity: () => bridge.identity(session), getRequestHeaders: () => ({ 'X-IDE-Version': '5.9.3' }) };
  const native = new Native(deps);
  const host = { authenticationManager: { currentSessionSubject: {
    getValue: () => session, subscribe: listener => { listeners.add(listener); return { unsubscribe: () => listeners.delete(listener) }; }
  } }, coordinator: { getConfirmedRemoteModelSnapshot: () => snapshot },
    productManager: { getEndpoint: () => 'https://www.workbuddy.cn', getCurrentConfiguration: () => ({
      authentication: { type: 'account-scoped-external-link', attributes: { prefixPath: '/plugin' } },
      platformVersion: '2.165.0', deploymentType: 'SaaS' }) } };
  return { native, host, get instance() { return instance; },
    get session() { return session; }, setResolve: value => { resolve = value; },
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
  assert.deepEqual(bridge.projectModel({ id: 'input-only', maxInputTokens: 100000 }), { id: 'input-only', name: 'input-only', contextWindow: undefined, maxOutputTokens: undefined, billing: { unit: 'credits' } });

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

test('explicit authorization exports only the current whitelist once and token rotation replaces it', async t => {
  const endpoint = await receiver(); const f = fixture();
  f.session.auth.domain = 'workbuddy.cn'; f.session.auth.tokenType = 'Bearer';
  f.session.auth.expiresAt = 1900000000000; f.session.auth.refreshExpiresAt = 1900001000000;
  f.session.auth.masterKey = 'fixture-master-never-export';
  f.session.bootstrap = { protectionKey: 'fixture-bootstrap-never-export' };
  f.session.accounts = [{ uid: 'unselected-account' }];
  const owned = bridge.attach(f.native, f.host, { ...endpoint.options, captureAuthorization: true });
  t.after(async () => { await owned.close(); await endpoint.close(); });
  await until(() => endpoint.reports.some(report => report.state === 'ready'));
  const grants = () => endpoint.reports.filter(report => report.authOnly && report.authorization);
  assert.equal(grants().length, 1);
  const grant = grants()[0];
  assert.equal(grant.authorization.auth.accessToken, 'fixture-real-account-secret');
  assert.equal(grant.authorization.auth.refreshToken, 'fixture-refresh-secret');
  assert.equal(grant.authorization.auth.expiresAt, 1900000000000);
  assert.equal(grant.authorization.gatewayEndpoint, 'https://www.workbuddy.cn');
  assert.equal(grant.authorization.authEndpoint, 'https://www.workbuddy.cn');
  assert.equal(grant.authorization.authPrefixPath, '/plugin');
  assert.equal(grant.authorization.authType, 'account-scoped-external-link');
  assert.equal(grant.authorization.appVersion, '5.9.3');
  assert.equal(grant.authorization.cliVersion, '2.165.0');
  assert.equal(grant.models[0].id, 'fixture-model');
  for (const text of ['fixture-master-never-export', 'fixture-bootstrap-never-export', 'unselected-account', 'fixture-upstream-secret', 'secret.invalid']) {
    assert(!JSON.stringify(endpoint.reports).includes(text));
  }
  await owned.publish(); await owned.publish();
  assert.equal(grants().length, 1);
  for (const report of endpoint.reports.filter(report => !report.authOnly)) {
    assert(!Object.hasOwn(report, 'authorization'));
    assert(!JSON.stringify(report).includes('fixture-real-account-secret'));
    assert.equal(report.authorizationEpoch, grant.authorizationEpoch);
  }
  f.setSession({ ...f.session, auth: { ...f.session.auth, accessToken: 'fixture-rotated-access', refreshToken: 'fixture-rotated-refresh' } });
  await until(() => grants().length === 2);
  assert.equal(grants()[1].authorization.auth.accessToken, 'fixture-rotated-access');
  assert.equal(grants()[1].authorizationEpoch, grant.authorizationEpoch + 1);
  assert.equal(endpoint.reports.filter(report => report.authOnly && report.authorization === null).length, 0);
  await owned.publish();
  assert.equal(grants().length, 2);
  const numericSession = { ...f.session, account: { uid: 7822, enterpriseId: 77 } };
  const numeric = bridge.projectAuthorization(numericSession, f.host, f.native);
  assert.equal(numeric.account.uid, 7822); assert.equal(numeric.account.enterpriseId, 77);
  assert.equal(bridge.identity(numeric), bridge.identity(numericSession));
});

test('account or domain change revokes the old authorization before the new grant', async t => {
  const endpoint = await receiver(); const f = fixture();
  const owned = bridge.attach(f.native, f.host, { ...endpoint.options, captureAuthorization: true });
  t.after(async () => { await owned.close(); await endpoint.close(); });
  await until(() => endpoint.reports.some(report => report.state === 'ready'));
  const authReports = () => endpoint.reports.filter(report => report.authOnly);
  const initial = authReports()[0];
  f.setSession({ account: { uid: 'fixture-next-account', enterpriseId: 'fixture-next-org' },
    auth: { accessToken: 'fixture-next-access', refreshToken: 'fixture-next-refresh', domain: 'workbuddy.cn' } });
  await until(() => authReports().length === 3);
  assert.equal(authReports()[1].authorization, null);
  assert.equal(authReports()[1].loggedIn, false);
  assert.deepEqual(authReports()[1].models, []);
  assert.equal(authReports()[2].authorization.account.uid, 'fixture-next-account');
  assert.equal(authReports()[2].authorizationEpoch, initial.authorizationEpoch + 2);
  f.setSession({ ...f.session, auth: { ...f.session.auth, domain: 'changed-workbuddy-domain' } });
  await until(() => authReports().length === 5);
  assert.equal(authReports()[3].authorization, null);
  assert.equal(authReports()[4].authorization.auth.domain, 'changed-workbuddy-domain');
  assert.equal(authReports()[4].authorizationEpoch, initial.authorizationEpoch + 4);
});

test('logout revokes authorization while native reference acquisition is still pending', async t => {
  const endpoint = await receiver(); const f = fixture();
  const owned = bridge.attach(f.native, f.host, { ...endpoint.options, captureAuthorization: true });
  t.after(async () => { await owned.close(); await endpoint.close(); });
  await until(() => endpoint.reports.some(report => report.state === 'ready'));
  let unblock, entered = false;
  f.setResolve(() => { entered = true; return new Promise(resolve => { unblock = resolve; }); });
  const pending = owned.publish();
  await until(() => entered);
  f.setSession(undefined);
  await until(() => endpoint.reports.some(report => report.authOnly && report.authorization === null));
  const revoked = endpoint.reports.find(report => report.authOnly && report.authorization === null);
  assert.equal(revoked.accountId, null); assert.equal(revoked.authorizationEpoch, 2);
  const index = endpoint.reports.indexOf(revoked);
  f.setResolve(undefined); unblock(); await pending;
  await until(() => endpoint.reports.at(-1)?.state === 'login-required');
  assert(endpoint.reports.slice(index + 1).every(report => report.state !== 'ready'));
});

test('missing native authorization metadata fails explicitly without guessing endpoint or version', async t => {
  const endpoint = await receiver(); const f = fixture();
  delete f.native.deps.getRequestHeaders;
  const owned = bridge.attach(f.native, f.host, { ...endpoint.options, captureAuthorization: true });
  t.after(async () => { await owned.close(); await endpoint.close(); });
  await until(() => endpoint.reports.some(report => report.state === 'authorization-unavailable'));
  assert(!endpoint.reports.some(report => report.authorization));
  assert(endpoint.reports.at(-1).authorizationError.includes('无法取得'));
  assert(endpoint.reports.at(-1).reference);
  assert(!JSON.stringify(endpoint.reports).includes('fixture-real-account-secret'));
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
    await until(() => endpoint.reports.some(report => report.state === 'unsupported-host'));
    assert.equal(endpoint.reports.find(report => report.state === 'unsupported-host').stage, true);
  } finally { Module.prototype._compile = previous; await endpoint.close(); }
  await tick();
});

test('main hook preserves the native entry if its daemon anchor changed', async () => {
  const endpoint = await receiver(); const previous = Module.prototype._compile;
  const mainBootstrap = require('node:path').resolve('target/workbuddy-fixture/main-bootstrap.js');
  try {
    bridge.install({ ...endpoint.options, role: 'main', mainBootstrap, bootstrap: __filename, captureAuthorization: true });
    const childEnv = globalThis[Symbol.for('tiancaispace.workbuddy.builtin-start')].env({ NATIVE_SYNTHETIC: 'keep' });
    assert.equal(childEnv.TIANCAI_WORKBUDDY_CAPTURE_AUTHORIZATION, '1');
    assert.equal(childEnv.NATIVE_SYNTHETIC, 'keep');
    assert(!JSON.stringify(childEnv).includes('fixture-real-account-secret'));
    const elsewhere = new Module('unrelated-main');
    elsewhere._compile('module.exports=41;', mainBootstrap + '.other');
    assert.equal(elsewhere.exports, 41);
    assert.notEqual(Module.prototype._compile, previous);
    const module = new Module('changed-main');
    module._compile(`/*${bridge.BOOTSTRAP_ANCHOR}${bridge.BOOTSTRAP_ANCHOR}*/module.exports=42;`, mainBootstrap);
    assert.equal(module.exports, 42);
    assert.equal(Module.prototype._compile, previous);
    await until(() => endpoint.reports.some(report => report.state === 'unsupported-bootstrap'));
  } finally { Module.prototype._compile = previous; await endpoint.close(); }
});

// Exercise the boundary missed by the original tests: main starts with no
// NODE_OPTIONS, is initialized via a real inspector before its entry, then its
// native manager starts a second Node process using actual --require argv.
async function inspectorFor(child) {
  const address = await new Promise((resolve, reject) => {
    let text = '';
    const timer = setTimeout(() => reject(new Error('fixture inspector not published')), 5000);
    child.stderr.on('data', bytes => {
      text += String(bytes);
      const found = text.match(/ws:\/\/127\.0\.0\.1:\d+\/[\w-]+/);
      if (found) { clearTimeout(timer); resolve(found[0]); }
    });
    child.once('exit', () => { clearTimeout(timer); reject(new Error('fixture exited before inspector')); });
  });
  const socket = new WebSocket(address);
  await once(socket, 'open');
  let sequence = 0; const pending = new Map(); const pauses = [];
  const scripts = new Map();
  socket.addEventListener('message', event => {
    const message = JSON.parse(event.data);
    if (message.method === 'Debugger.scriptParsed') scripts.set(message.params.scriptId, message.params.url);
    if (message.method === 'Debugger.paused') pauses.push(message.params);
    const job = pending.get(message.id);
    if (!job) return;
    pending.delete(message.id); clearTimeout(job.timer);
    message.error ? job.reject(new Error('fixture protocol error')) : job.resolve(message.result);
  });
  const call = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++sequence;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error('fixture protocol timeout')); }, 3000);
    pending.set(id, { resolve, reject, timer }); socket.send(JSON.stringify({ id, method, params }));
  });
  const evaluate = async expression => {
    const result = await call('Runtime.evaluate', { expression, returnByValue: true });
    if (result.exceptionDetails) throw new Error('fixture evaluation failed');
    return result.result.value;
  };
  const entry = async filename => {
    const { pathToFileURL, fileURLToPath } = require('node:url');
    const path = require('node:path');
    const expected = path.resolve(filename).toLowerCase();
    await call('Runtime.enable');
    await call('Debugger.enable');
    await call('Debugger.setBreakpointByUrl', { url: pathToFileURL(filename).href, lineNumber: 0 });
    await call('Runtime.runIfWaitingForDebugger');
    for (let count = 0; count < 10; count++) {
      await until(() => pauses.length);
      const paused = pauses.shift(); const url = scripts.get(paused.callFrames[0].location.scriptId);
      const current = url?.startsWith('file:') ? fileURLToPath(url) : url;
      if (current && path.resolve(current).toLowerCase() === expected) return;
      await call('Debugger.resume');
    }
    assert.fail('fixture did not stop before its original entry');
  };
  return { call, evaluate, entry, socket, address };
}
async function withInspectedFixture(t, files, run) {
  const fs = require('node:fs'); const os = require('node:os'); const path = require('node:path');
  const { spawn } = require('node:child_process');
  const base = fs.realpathSync(os.tmpdir());
  const root = fs.mkdtempSync(path.join(base, 'hub startup with  spaces '));
  for (const [name, source] of Object.entries(files)) fs.writeFileSync(path.join(root, name), source);
  fs.copyFileSync(path.join(__dirname, 'host-bridge.cjs'), path.join(root, 'host-bridge.cjs'));
  fs.copyFileSync(path.join(__dirname, 'model-catalog.cjs'), path.join(root, 'model-catalog.cjs'));
  const child = spawn(process.execPath, ['--inspect-brk=127.0.0.1:0', path.join(root, 'main.cjs')], {
    env: { ...process.env, NODE_OPTIONS: '' }, stdio: ['ignore', 'ignore', 'pipe']
  });
  t.after(async () => { if (child.exitCode === null) { child.kill(); await once(child, 'exit'); }
    assert.equal(path.dirname(fs.realpathSync(root)), base);
    fs.rmSync(root, { recursive: true, force: true }); });
  const inspector = await inspectorFor(child);
  await inspector.entry(path.join(root, 'main.cjs'));
  assert.equal(await inspector.evaluate('process.pid'), child.pid);
  try { await run(root, child, inspector); }
  finally {
    // The same cleanup is required even when loading the bridge throws.
    await inspector.evaluate("(()=>{setImmediate(()=>process.getBuiltinModule('inspector').close());return true;})()");
    await inspector.call('Debugger.resume');
    inspector.socket.close();
  }
  const net = require('node:net'); const port = Number(new URL(inspector.address).port);
  await new Promise((resolve, reject) => {
    let count = 0;
    const check = () => {
      const connection = net.createConnection({ host: '127.0.0.1', port });
      connection.once('error', resolve);
      connection.once('connect', () => { connection.destroy();
        if (++count > 100) reject(new Error('fixture inspector stayed open'));
        else setTimeout(check, 10); });
    };
    check();
  });
  return { root, child };
}

test('an inspector bind failure can run the original entry without publishing a native URI', { timeout: 5000 }, async t => {
  const net = require('node:net'); const { spawn } = require('node:child_process');
  const reservation = net.createServer(); reservation.listen(0, '127.0.0.1');
  await once(reservation, 'listening'); t.after(() => reservation.close());
  const port = reservation.address().port;
  const child = spawn(process.execPath, [`--inspect-brk=127.0.0.1:${port}`, '-e', "process.stdout.write('original entry ran')"], {
    env: { ...process.env, NODE_OPTIONS: '' }, stdio: ['ignore', 'pipe', 'pipe']
  });
  t.after(() => { if (child.exitCode === null) child.kill(); });
  let output = '', diagnostics = '';
  child.stdout.on('data', bytes => { output += String(bytes); });
  child.stderr.on('data', bytes => { diagnostics += String(bytes); });
  const [code] = await once(child, 'exit');
  assert.equal(code, 0); assert.equal(output, 'original entry ran');
  assert(!diagnostics.includes('Debugger listening on '));
});

test('real inspector initializes main before entry and native child argv loads the host bridge', async t => {
  const endpoint = await receiver(); t.after(() => endpoint.close());
  const path = require('node:path');
  const main = `require('./main-bootstrap.js');`;
  const mainBootstrap = `const {spawn}=require('node:child_process');const path=require('node:path');
class DaemonAppServerProcessManager{constructor(options){
const child=spawn(process.execPath,[...(options.processExecArgv||[]),path.join(__dirname,'daemon.cjs')],{env:{...process.env,...options.env,ELECTRON_RUN_AS_NODE:'1'},stdio:'ignore'});
child.on('exit',code=>{process.exitCode=code||0;});process.on('SIGTERM',()=>{child.kill();process.exit();});}}
let u={NODE_OPTIONS:'',SYNTHETIC_NATIVE_FLAG:'keep'};new DaemonAppServerProcessManager({env:u,requestHandler:()=>{}});`;
  const server = `const keepAlive=setInterval(()=>{},1000);class Native{constructor(deps){this.deps=deps;}syncIdentity(){}releaseSession(){}
async resolveEnv(){return{WORKBUDDY_VLM_CHAT_URL:'http://127.0.0.1:12345/skills/vlm/chat',WORKBUDDY_VLM_AUTH_TOKEN:'fixture-native-ledger',WORKBUDDY_VLM_CONTEXT:'fixture-context'};}
async close(){clearInterval(keepAlive);}}
function createSkillVlmService(){return new Native({});}
function start(){let e={authenticationManager:{currentSessionSubject:{getValue:()=>({account:{uid:'fixture-account'},auth:{accessToken:'fixture-never-export'}}),subscribe:()=>({unsubscribe(){}})}},coordinator:{getConfirmedRemoteModelSnapshot:()=>({freshness:'fresh',configuration:{agents:[{name:'cli',tags:['cli','default'],models:['fixture-model']}],models:[{id:'fixture-model',name:'Fixture'}]}})}},r={appVersion:'fixture'},s=()=>{},O={};${bridge.ANCHOR}}start();`;
  const daemon = `if(process.env.SYNTHETIC_NATIVE_FLAG!=='keep')throw Error('native env lost');
if(Object.keys(process.env).some(k=>k.startsWith('TIANCAI_WORKBUDDY_')))throw Error('lease inherited by tools');
if(process.execArgv.some(arg=>arg.endsWith('host-bridge.cjs')))throw Error('preload inherited by tools');require('./server.js');`;
  const fixture = await withInspectedFixture(t, { 'main.cjs': main, 'main-bootstrap.js': mainBootstrap,
    'daemon.cjs': daemon, 'server.js': server }, async (root, _child, inspector) => {
      const bootstrap = path.join(root, 'host-bridge.cjs');
      const options = { ...endpoint.options, callback: String(endpoint.options.callback), role: 'main', bootstrap,
        server: path.join(root, 'server.js'), mainBootstrap: path.join(root, 'main-bootstrap.js') };
      const file = JSON.stringify(bootstrap);
      const expression = `process.getBuiltinModule('module').createRequire(${file})(${file}).install(${JSON.stringify(options)})`;
      assert.deepEqual(await inspector.evaluate(expression), { installed: true, role: 'main' });
    });
  await until(() => endpoint.reports.some(report => report.state === 'ready'));
  const states = endpoint.reports.map(report => report.state);
  for (const state of ['main-loader-ready', 'main-bootstrap-ready', 'daemon-preload-ready', 'server-module-ready', 'ready']) assert(states.includes(state));
  assert(!JSON.stringify(endpoint.reports).includes('fixture-never-export'));
  endpoint.reject();
  if (fixture.child.exitCode === null) await once(fixture.child, 'exit');
});

test('failed inspector preload is resumed and original entry still runs', async t => {
  const fs = require('node:fs'); const path = require('node:path');
  const fixture = await withInspectedFixture(t, { 'main.cjs': `require('node:fs').writeFileSync(require('node:path').join(__dirname,'ran.txt'),'original');` },
    async (root, _child, inspector) => {
      const missing = JSON.stringify(path.join(root, 'missing-bridge.cjs'));
      await assert.rejects(inspector.evaluate(`process.getBuiltinModule('module').createRequire(${missing})(${missing})`));
    });
  if (fixture.child.exitCode === null) await once(fixture.child, 'exit');
  assert.equal(fs.readFileSync(path.join(fixture.root, 'ran.txt'), 'utf8'), 'original');
});
