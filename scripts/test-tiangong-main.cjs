// Offline synthetic main-process LiveEdit. Never starts Electron or a product.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const inspector = require('node:inspector');
const {test} = require('node:test');
const root = path.join(__dirname, '..');
const adapter = vm.runInNewContext(fs.readFileSync(path.join(root, 'src/gmclaw_runtime/main-workspace-adapter.js'), 'utf8'));
const context = fs.readFileSync(path.join(root, 'src/gmclaw_runtime/main-workspace-context.js'), 'utf8');
const fixture = `
(() => {
const db = {session: {userId: 'official-user', clientId: 'native-client'}};
function readClawSession(value) { return value.session; }
const opts = {
    conversationAccount: () => db ? readClawSession(db)?.userId || "" : "",
};
let claims = 0;
const conversationAccounts = {claimLegacy() { claims++; }};
function claim(owner) {
      conversationAccounts.claimLegacy(owner);
}
let sessionRoute;
function add(method, url, callback) { sessionRoute = callback; }
add("GET", "/data/claw/session", ({ db: db2 }) => {
  const session = readClawSession(db2);
  if (!session) return { loggedIn: false };
  return {
    loggedIn: true,
    userId: session.userId,
  };
});
function run(db2,task,project) {
  return {user_id: "local", agent_id: "desktop-orchestration", project_id: "default", project,};
}
function revision(db2,task) {
  const workspace = 'original-workspace';
  return {
      session_id: task.session_id,
      user_id: "local",
      project: { id: task.scenario_id === "default" ? "0" : task.scenario_id, path: workspace,},
  };
}
function resend(db2,task) {
  const defaultWorkspace = () => 'original-workspace';
  const taskWorkspacePath = () => 'original-workspace';
  return {
        project: { path: taskWorkspacePath(task, task.work_dir, defaultWorkspace()) },
        user_id: "local",
        agent_id: "desktop-orchestration",
  };
}
function scheduled(user) {
  return {
      user_id: "local",
      agent_id: "desktop-orchestration",
  };
}
globalThis.__tgSynthetic = {opts,db,sessionRoute,run,revision,resend,scheduled,claim,claims:()=>claims};
})();`;

test('1.1 transform is atomic, idempotent and parses as JavaScript', () => {
  const result = adapter(fixture);
  assert.equal(result.adapted, true);
  new vm.Script(result.source);
  assert.equal(adapter(result.source).reason, 'already_adapted');
  assert.equal(adapter(fixture.replace('conversationAccount:', 'changedAccount:')).adapted, false);
  assert.equal(adapter(fixture.replace('      conversationAccounts.claimLegacy(owner);', '')).source, undefined);
});

test('live native callbacks change scope without changing the official account; rollback works', async () => {
  const connection = new inspector.Session();
  connection.connect();
  const post = (method, params = {}) => new Promise((resolve, reject) =>
    connection.post(method, params, (error, value) => error ? reject(error) : resolve(value)));
  let scriptId;
  connection.on('Debugger.scriptParsed', ({params}) => {
    if (params.url === 'tiangong-main-synthetic.cjs') scriptId = params.scriptId;
  });
  try {
    await post('Debugger.enable');
    vm.runInThisContext(fixture, {filename: 'tiangong-main-synthetic.cjs'});
    const api = globalThis.__tgSynthetic;
    assert.equal(api.opts.conversationAccount(), 'official-user');
    vm.runInThisContext(context);
    const result = await post('Debugger.setScriptSource', {scriptId, scriptSource: adapter(fixture).source});
    assert.equal(result.status, 'Ok');
    assert.equal(api.opts.conversationAccount(), 'ai-gateway');
    assert.deepEqual(api.sessionRoute({db: api.db}), {loggedIn: true, userId: 'official-user', hubConversationOwner: 'ai-gateway'});
    api.db.session = null;
    assert.deepEqual(api.sessionRoute({db: api.db}), {loggedIn: false, hubConversationOwner: 'ai-gateway'});
    api.claim('ai-gateway');
    assert.equal(api.claims(), 0);
    api.claim('official-user');
    assert.equal(api.claims(), 1);
    const db = {prepare(sql) { assert.match(sql, /FROM sessions WHERE session_id/); return {get: () => ({user_id: 'ai-gateway', agent_id: 'original-agent', project_id: 'original-project'})}; }};
    const task = {session_id: 'same-session', owner_id: 'ai-gateway'};
    for (const scope of [api.run(db,task,{}), api.revision(db,task), api.resend(db,task)]) {
      assert.equal(scope.user_id, 'ai-gateway');
      assert.equal(scope.agent_id, 'original-agent');
      assert.equal(scope.project_id, 'original-project');
      assert.equal(scope.project.id, 'original-project');
    }
    assert.equal(api.scheduled('native-owner').user_id, 'native-owner');
    const restored = await post('Debugger.setScriptSource', {scriptId, scriptSource: fixture});
    assert.equal(restored.status, 'Ok');
    api.db.session = {userId: 'official-user'};
    assert.equal(api.opts.conversationAccount(), 'official-user');
    assert.equal(api.sessionRoute({db: api.db}).hubConversationOwner, undefined);
  } finally {
    connection.disconnect();
    delete globalThis.__tgSynthetic;
    delete globalThis.__tiancaiHubWorkspaceMain;
  }
});

test('task without native metadata uses its saved owner and native defaults', () => {
  const sandbox = {};
  vm.runInNewContext(context, sandbox);
  const scope = sandbox.__tiancaiHubWorkspaceMain.taskScope({prepare:()=>({get:()=>undefined})}, {session_id:'new',owner_id:'ai-gateway'});
  assert.equal(scope.user_id, 'ai-gateway');
  assert.equal(scope.agent_id, 'desktop-orchestration');
  assert.equal(scope.project_id, 'default');
});

if (process.argv[2]) test('installed 1.1.0 entry transforms and parses without executing vendor code', () => {
  const source = fs.readFileSync(path.resolve(process.argv[2]), 'utf8');
  const result = adapter(source);
  assert.equal(result.adapted, true);
  new vm.Script(result.source);
});
