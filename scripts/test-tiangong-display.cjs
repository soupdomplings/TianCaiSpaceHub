// Offline tests of the exact renderer helpers; no desktop, browser or real API.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const root = path.resolve(__dirname, '..', 'src', 'gmclaw_runtime');
const load = (name) => vm.runInThisContext(`(${fs.readFileSync(path.join(root, name), 'utf8')})`);
const setup = load('display-setup-schema.js');
const schema = load('display-schema.js');
const sync = load('display-sync.js');
const ref = (value) => ({__v_isRef: true, value});
let passed = 0;

function fixture(legacy = false) {
  const taskId = 'fixture-task';
  const sessionId = 'fixture-session';
  const instance = {taskId, sessionId, seedMessages: [], messageSummaries: [], hasOlderMessages: false,
    selectedModel: 'chosen-model', draft: '保留草稿'};
  const panel = [ref([]), ref(false), ref(null), ref(true), ref(false), () => {}, ref(false), ref(true),
    ref(taskId), ref(sessionId), null,
    ...(legacy ? [null, null, null] : [ref(null), ref(false), ref(false)])];
  const app = [ref([instance]), ref([]), new Set(), new Set()];
  const props = {taskId, conversationSessionId: sessionId, isActive: false};
  return {panel, app, props, instance, args: [...panel, ...app, props, taskId, sessionId, [1]]};
}

async function run(f, hook) {
  let requests = 0;
  let hooked = false;
  const previous = {fetch: global.fetch, window: global.window, document: global.document};
  global.window = {electronAPI: {getDataUrl: async () => 'http://127.0.0.1:18768', getAuthToken: async () => 'fixture'}};
  global.document = {querySelectorAll: () => []};
  global.fetch = async (url, options) => {
    requests++;
    assert.equal(options.method, 'GET');
    assert.equal(options.headers.Authorization, 'Bearer fixture');
    if (!hooked && hook) { hooked = true; hook(f); }
    let value;
    if (url.endsWith('/data/scenarios')) {
      value = {scenarios: [{scenario_id: 'project', tasks: [{task_id: f.props.taskId,
        session_id: f.props.conversationSessionId, model_id: 'chosen-model'}]}]};
    } else if (url.endsWith('/messages?limit=100')) {
      value = {has_more: false, messages: [{id: 1, task_id: f.props.taskId, role: 'system', content: 'IM 回复',
        created_at: '2026-10-22 10:00:00', files: [], thinking_steps: []}]};
    } else if (url.endsWith('/messages/summaries')) {
      value = {messages: [{id: 1, role: 'system', content: 'IM 回复'}]};
    } else { throw new Error('Unexpected fixture request'); }
    return new Response(JSON.stringify(value));
  };
  try { return {outcome: await sync(...f.args), requests}; }
  finally { Object.assign(global, previous); }
}

async function main() {
  for (const legacy of [true, false]) {
    const f = fixture(legacy);
    assert.equal(setup('panel', ...f.panel), true);
    assert.equal(setup('app', ...f.app), true);
    assert.equal(schema(...f.panel, ...f.app, f.props), 'ready');
    assert.equal((await run(f)).outcome, 'updated');
    assert.equal(f.panel[0].value[0].content, 'IM 回复');
    assert.equal(f.instance.selectedModel, 'chosen-model');
    assert.equal(f.instance.draft, '保留草稿');
    passed++;
  }
  for (const status of ['completed', 'stopped', 'failed', 'interrupted']) {
    const f = fixture();
    f.panel[11].value = {status, phase: 'done', prompts: []};
    assert.equal((await run(f)).outcome, 'updated');
    passed++;
  }
  const busyCases = [
    f => { f.panel[11].value = {status: 'running', phase: 'executing', prompts: []}; },
    f => { f.panel[11].value = {status: 'completed', phase: 'finalizing', prompts: []}; },
    f => { f.panel[12].value = true; },
    f => { f.panel[13].value = true; },
    f => { f.panel[11].value = {status: 'completed', prompts: [{status: 'queued'}]}; },
    f => { f.panel[11].value = {status: 'new-unknown-state', prompts: []}; },
    f => { f.panel[1].value = true; },
    f => { f.panel[2].value = {call_id: 'pending'}; },
  ];
  for (const change of busyCases) {
    const f = fixture();
    change(f);
    assert.deepEqual(await run(f), {outcome: 'busy', requests: 0});
    assert.equal(f.panel[0].value.length, 0);
    passed++;
  }
  for (const change of [f => { f.panel[12].value = true; }, f => { f.panel[13].value = true; },
    f => { f.panel[11].value = {status: 'completed', prompts: [{status: 'queued'}]}; }]) {
    const f = fixture();
    assert.equal((await run(f, change)).outcome, 'changed');
    assert.equal(f.panel[0].value.length, 0);
    assert.equal(f.instance.seedMessages.length, 0);
    passed++;
  }
  for (const change of [f => { f.panel[0].__v_isReadonly = true; }, f => { Object.freeze(f.instance); }]) {
    const f = fixture();
    change(f);
    assert.notEqual(schema(...f.panel, ...f.app, f.props), 'ready');
    assert.notEqual((await run(f)).outcome, 'updated');
    assert.equal(f.panel[0].value.length, 0);
    passed++;
  }
  const partial = fixture(true);
  partial.panel[11] = partial.args[11] = ref(null);
  assert.equal(setup('panel', ...partial.panel), false);
  assert.equal(schema(...partial.panel, ...partial.app, partial.props), 'panel_fields');
  assert.equal((await run(partial)).outcome, 'panel_fields');
  passed++;
  console.log(`TianGong display: ${passed} offline cases passed`);
}
main().catch(error => { console.error(error); process.exitCode = 1; });
