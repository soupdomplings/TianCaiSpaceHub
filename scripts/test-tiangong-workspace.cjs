// Offline source/lifecycle regression. No Electron, real API, account or model call.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const {spawnSync} = require('node:child_process');
const root = path.resolve(__dirname, '..');
const adapterFile = path.join(root, 'src/gmclaw_runtime/workspace-adapter.js');
const adapt = vm.runInThisContext(fs.readFileSync(adapterFile, 'utf8'), {filename: adapterFile});
const options = {externalModelIds: ['external-a', 'external-b'], preferredModelId: 'external-b'};
const template = (text) => String.fromCharCode(96) + text + String.fromCharCode(96);
const preference = String.fromCharCode(36) + '{user.value.userId || "default"}';
// Small renderer contract fixture. API implementations and Vue values are synthetic.
const original = [
  'function isBuiltinModel(model) { return ["tiangong-builtin", "tiangong-builtin-intranet"].includes(model?.model_id); }',
  'async function req(method, path, body, options) { return fixtureRequest(method, path, body, options); }',
  'const enc = encodeURIComponent;',
  'const scenarioApi = {',
  '  list: () => req("GET", "/data/scenarios").then((r) => r.scenarios),',
  '  searchMessages: (query) => req("GET", `/data/messages/search?q=${enc(query)}`).then((r) => r.results),',
  '  createTask: (scenarioId, title = "新任务", modelId = "", modelOptions = {}) => req("POST", "/data/tasks", { scenario_id: scenarioId, title, model_id: modelId, model_options: modelOptions }),',
  '  deleteTask: (id) => req("DELETE", `/data/tasks/${enc(id)}`),',
  '};',
  'function renderSidebar(_cache = []) { return _cache[58] || (_cache[58] = createBaseVNode("span", { class: "sidebar-user-menu-action-label" }, "退出登录", -1)); }',
  'function renderAccountStatus(_cache = []) { return [_cache[54] || (_cache[54] = createBaseVNode("small", { class: "sidebar-user-status" }, "当前已登录", -1)), _cache[55] || (_cache[55] = createBaseVNode("small", { class: "sidebar-user-menu-status" }, "当前已登录", -1))]; }',
  'const Panel = /* @__PURE__ */ defineComponent({',
  '  __name: "ChatPanel",',
  '  setup(props) {',
  '    const dbModels = ref([]);',
  '    const dbModelsLoaded = ref(false);',
  '    const selectedModelId = ref(props.initialModelId);',
  '    const hasActiveRun = ref(false);',
  '    const selectedModelName = computed(() => {',
  '      if (!dbModels.value.length) return "无可用模型";',
  '      const m = dbModels.value.find((model) => model.model_id === selectedModelId.value);',
  '      return modelDisplayName(m || dbModels.value[0]);',
  '    });',
  '    const runActionError = ref("");',
  '    const showModelDropdown = ref(false);',
  '    const draft = ref("保留草稿");',
  '    const inputValue = ref("待发送消息");',
  '    const sessionId = ref(props.conversationSessionId || "");',
  '    const localTaskId = ref(props.taskId || "");',
  '    async function startChatRun(recovery) {',
  '      const modelId = selectedModelId.value;',
  '      let activeTaskId = localTaskId.value || "";',
  '      if (!activeTaskId) {',
  '        try {',
  '          const task = await scenarioApi.createTask(props.activeScenarioId || "default", "新任务", modelId, {});',
  '          sessionId.value = task.session_id;',
  '          activeTaskId = task.task_id;',
  '          localTaskId.value = task.task_id;',
  '        } catch (e) {',
  '          console.warn("[chat] 自动创建默认任务失败:", e);',
  '        }',
  '      }',
  '      const chatId = recovery?.run.chat_id || await window.electronAPI.generateId();',
  '        const requestBody = {',
  '          session_id: sessionId.value, chat_id: chatId, query: inputValue.value,',
  '          user_id: "local",',
  '          agent_id: "desktop-orchestration",',
  '          project_id: "default",',
  '          project: props.conversationProject || {id:"0",path:"D:/synthetic/workspace"}, model_id: modelId,',
  '        };',
  '      const result = await chatRunsApi.create(requestBody);',
  '      inputValue.value = "";',
  '      return result;',
  '    }',
  '    async function loadDbModels() {',
  '      dbModelsLoaded.value = false;',
  '      try {',
  '        const models = await modelApi.list();',
  '        dbModels.value = models;',
  '        const previousSelectionStillExists = models.some((model) => model.model_id === selectedModelId.value);',
  '        if (!previousSelectionStillExists) {',
  '          selectedModelId.value = models[0]?.model_id || "";',
  '        }',
  '      } finally { dbModelsLoaded.value = true; }',
  '    }',
  '    async function ensureModelAvailable() {',
  '      if (!dbModelsLoaded.value) await loadDbModels();',
  '      if (dbModels.value.some((model) => model.model_id === selectedModelId.value && (!isBuiltinModel(model) || model.is_configured))) return true;',
  '      showNoModelPrompt(); return false;',
  '    }',
  '    function showNoModelPrompt() { showModelDropdown.value = true; }',
  '    return {loadDbModels, ensureModelAvailable, selectedModelId, selectedModelName, dbModels, runActionError, showModelDropdown, draft, hasActiveRun, startChatRun, inputValue, sessionId};',
  '  }',
  '});',
  'const ChatPanel = /* @__PURE__ */ _export_sfc(Panel, []);',
  'const Root = /* @__PURE__ */ defineComponent({',
  '  __name: "App",',
  '  setup(__props) {',
  '    const DEFAULT_USER = {',
  '      username: "admin", role: "admin",',
  '      display_name: "Admin",',
  '      userId: ""',
  '    };',
  '    const user = ref(DEFAULT_USER);',
  '    const authReady = ref(false), authError = ref(""), bootstrapLoading = ref(false), bootstrapError = ref(""), mcpSyncError = ref("");',
  '    let authAttempt = 0, bootstrapAttempt = 0, mcpSyncAttempt = 0, scenarioRefreshRequest = 0;',
  '    const clawLoggedIn = ref(false);',
  '    const showUserPanel = ref(false);',
  '    const modelListVersion = ref(0);',
  '    const scenarios = ref([]);',
  '    const chatInstances = ref([]);',
  '    const defaultWorkspacePath = ref("");',
  '    const activeInst = computed(() => chatInstances.value[0] || null);',
  '    const activePage = ref("chat"), windowFocused = ref(true), windowVisible = ref(true);',
  '    const unreadCompletionCount = computed(() => clawLoggedIn.value ? scenarios.value.reduce((n, s) => n + s.tasks.length, 0) : 0);',
  '    function isTaskVisible(taskId) { return clawLoggedIn.value && windowFocused.value && windowVisible.value && activePage.value === "chat" && activeInst.value?.taskId === taskId; }',
  '    function lastModelPreferenceKey() { return ' + template('gmclaw-last-model:' + preference) + '; }',
  '    function activeTaskRecoveryKey() { return ' + template('tiangong-active-task:' + preference) + '; }',
  '    async function refreshScenarios() { fixture.scenarioReads++; if (fixture.scenarioError) return false; try { scenarios.value = await scenarioApi.list(); return true; } catch { return false; } }',
  '    function ensureInitialInstance() { if (!chatInstances.value.length) chatInstances.value.push({taskId: null, draft: "保留草稿", modelId: ""}); }',
  '    async function restoreActiveTask() { fixture.recoveries++; const taskId = readRecovery(activeTaskRecoveryKey()); const task = scenarios.value.flatMap((item) => item.tasks).find((item) => item.task_id === taskId); if (task) await onOpenTask(task); }',
  '    async function onOpenTask(task, messageId, query) {',
  '      fixture.opened.push(task.task_id);',
  '    }',
  '    async function onDeleteTask(task) {',
  '      await scenarioApi.deleteTask(task.task_id);',
  '    }',
  '    async function onDeleteScenario(scenarioId) { await req("DELETE", "/data/scenarios/" + enc(scenarioId)); }',
  '    async function onRenameTask(task, title) { }',
  '    async function checkClawSession(attempt = authAttempt) {',
  '      const session = await clawAuthApi.getSession();',
  '      if (attempt !== authAttempt) return false;',
  '      if (!session.loggedIn) { clawLoggedIn.value = false; user.value = {...DEFAULT_USER, userId: ""}; return false; }',
  '      const intro = await clawAuthApi.introspect({ logoutOnFail: true });',
  '      if (attempt !== authAttempt) return false;',
  '      if (!intro.ok) { clawLoggedIn.value = false; return false; }',
  '      user.value = {...user.value, userId: session.userId}; clawLoggedIn.value = true; return true;',
  '    }',
  '    async function initializeSession() {',
  '      const attempt = ++authAttempt; authReady.value = false;',
  '      const ok = await checkClawSession(attempt); authReady.value = true; if (ok) await bootstrapAppData();',
  '    }',
  '    async function bootstrapAppData() {',
  '      const attempt = ++bootstrapAttempt;',
  '      bootstrapLoading.value = true;',
  '      bootstrapError.value = "";',
  '      void syncMcpCatalogOnBootstrap();',
  '      try {',
  '        void window.electronAPI?.skillhubLogin?.().catch(() => {});',
  '      } catch {}',
  '      try {',
  '        const workspace = await withDeadline(window.electronAPI.getDefaultWorkspace(), 1e4, "workspace timeout");',
  '        if (attempt !== bootstrapAttempt || !clawLoggedIn.value) return;',
  '        defaultWorkspacePath.value = workspace;',
  '        if (!await refreshScenarios()) throw new Error("场景未能加载，请重试。");',
  '        if (attempt !== bootstrapAttempt || !clawLoggedIn.value) return;',
  '        ensureInitialInstance();',
  '        await restoreActiveTask();',
  '      } catch (error) { if (attempt === bootstrapAttempt) bootstrapError.value = error.message; }',
  '      finally {',
  '        if (attempt === bootstrapAttempt) bootstrapLoading.value = false;',
  '      }',
  '    }',
  '    async function syncMcpCatalogOnBootstrap() {',
  '      const attempt = ++mcpSyncAttempt; mcpSyncError.value = "";',
  '      try { await clawAuthApi.syncMcp(); } catch { if (attempt === mcpSyncAttempt) mcpSyncError.value = "cloud sync unavailable"; }',
  '    }',
  '    watch(() => [clawLoggedIn.value, activeInst.value?.taskId], () => {});',
  '    function onSessionInvalid() { authAttempt++; bootstrapAttempt++; mcpSyncAttempt++; scenarioRefreshRequest++; clawLoggedIn.value = false; }',
  '    async function onClawLoginSuccess() {',
  '      const loggedIn = await checkClawSession();',
  '      clawLoggedIn.value = loggedIn;',
  '      if (!loggedIn) return;',
  '      await bootstrapAppData();',
  '    }',
  '    async function onLogout() { await clawAuthApi.logout(); onSessionInvalid(); }',
  '    async function onCheckUpdate() {}',
  '    function render(_ctx, _cache = []) {',
  '      return openBlock(), createElementBlock("div", null, [',
  '        createVNode(UpdateBanner, { class: "app-root" }),',
  '        !authReady.value ? (openBlock(), createElementBlock("section", null, [])) : !clawLoggedIn.value ? (openBlock(), createBlock(ClawQrLoginPage, {',
  '          onSuccess: onClawLoginSuccess',
  '        })) : authReady.value && clawLoggedIn.value ? (openBlock(), createElementBlock("div", _hoisted_4, [',
  '          createVNode(Sidebar, {',
  '            "user-name": user.value.userId || user.value.display_name || user.value.username,',
  '            "running-task-ids": new Set(),',
  '            onOpenUserPanel: _cache[0] || (_cache[0] = ($event) => showUserPanel.value = true),',
  '          }),',
  '          createVNode(ChatPanel, {',
  '                  "user-name": user.value.userId || user.value.display_name || user.value.username,',
  '                  "is-active": true,',
  '          })',
  '        ])) : createCommentVNode("", true)',
  '      ]);',
  '    }',
  '    return {initializeSession, bootstrapAppData, onSessionInvalid, onClawLoginSuccess, onLogout, render, clawLoggedIn, authReady, authError, bootstrapError, bootstrapLoading, user, chatInstances, modelListVersion, defaultWorkspacePath, lastModelPreferenceKey, activeTaskRecoveryKey, syncMcpCatalogOnBootstrap, onOpenTask, onDeleteTask, onDeleteScenario, scenarios, refreshScenarios};',
  '  }',
  '});',
  'const App = /* @__PURE__ */ _export_sfc(Root, []);',
  'globalThis.makeApp = () => App.setup({});',
  'globalThis.makePanel = (props) => ChatPanel.setup(props);',
  'globalThis.sidebarLabel = () => renderSidebar().children;',
  'globalThis.accountStatus = () => renderAccountStatus().map((node) => node.children);',
  'globalThis.fixtureScenarioApi = scenarioApi;',
].join('\n');
const startupLoader = [
  'const notice = document.getElementById("startup-notice");',
  'function showFailure() { notice.hidden = false; }',
  'window.addEventListener("app-mounted", () => { notice.hidden = true; });',
  'void __vitePreload(() => import("./main-native.js"), true ? __vite__mapDeps([0,1]) : void 0, import.meta.url).catch((error) => {',
  '  showFailure();',
  '});',
].join('\n');
const models = [
  {model_id: 'tiangong-builtin', display_name: 'official', is_configured: true},
  {model_id: 'external-a', display_name: 'A'},
  {model_id: 'external-b', display_name: 'B'},
];
const flush = () => new Promise((resolve) => setImmediate(resolve));
let passed = 0;

function environment(source = original, behavior = {}) {
  const watched = new WeakMap();
  const fixture = {models: models.map((model) => ({...model})), recoveries: 0, scenarioReads: 0,
    workspaceReads: 0, cloudSyncs: 0, skillLogins: 0, introspects: 0, logouts: 0, loggedIn: false,
    focusReturns: 0, scenarios: [{scenario_id: 'default', tasks: []}], sessions: [], searchResults: [],
    requests: [], modelCalls: [], opened: [], notifications: [], nextTaskId: 1, recovery: {}, ...behavior};
  const ref = (initial) => {
    let value = initial;
    const target = {__v_isRef: true};
    Object.defineProperty(target, 'value', {get: () => value, set: (next) => {
      const previous = value; value = next;
      if (previous !== next) for (const callback of watched.get(target) || []) callback(next, previous);
    }});
    return target;
  };
  const node = (type, props, children) => ({type, props, children});
  const fixtureRequest = async (method, route, body) => {
    fixture.requests.push({method, route, body});
    const parsed = new URL(route, 'http://fixture.invalid');
    if (method === 'GET' && parsed.pathname === '/data/scenarios') return {scenarios: fixture.scenarios};
    if (method === 'GET' && parsed.pathname === '/data/sessions') {
      if (fixture.ownerReadError) throw new Error('synthetic ownership read failure');
      const owned = fixture.sessions.filter((session) => session.user_id === parsed.searchParams.get('user_id'));
      const offset = Number(parsed.searchParams.get('offset')), limit = Number(parsed.searchParams.get('limit'));
      return {sessions: owned.slice(offset, offset + limit)};
    }
    if (method === 'GET' && parsed.pathname.startsWith('/data/sessions/')) {
      const id = decodeURIComponent(parsed.pathname.slice('/data/sessions/'.length));
      const session = fixture.sessions.find((row) => row.session_id === id);
      if (!session) throw new Error('synthetic missing session');
      return {...session};
    }
    if (method === 'GET' && parsed.pathname === '/data/messages/search') return {results: fixture.searchResults};
    if (method === 'POST' && parsed.pathname === '/data/tasks') {
      if (fixture.taskCreateError) throw new Error('synthetic task creation failure');
      const id = fixture.nextTaskId++;
      const task = {...body, task_id: 'new-task-' + id, session_id: 'new-session-' + id};
      fixture.scenarios.find((scenario) => scenario.scenario_id === body.scenario_id).tasks.push(task);
      return task;
    }
    if (method === 'POST' && parsed.pathname === '/data/sessions') {
      if (fixture.registerError) throw new Error('synthetic session registration failure');
      fixture.sessions.push({...body}); return {session_id: body.session_id};
    }
    if (method === 'DELETE' && parsed.pathname.startsWith('/data/tasks/')) {
      const id = decodeURIComponent(parsed.pathname.slice('/data/tasks/'.length));
      for (const scenario of fixture.scenarios) scenario.tasks = scenario.tasks.filter((task) => task.task_id !== id);
      return {status: 'deleted'};
    }
    if (method === 'DELETE' && parsed.pathname.startsWith('/data/scenarios/')) return {status: 'deleted'};
    throw new Error('unhandled synthetic route: ' + method + ' ' + route);
  };
  const context = vm.createContext({
    fixture, fixtureRequest, ref, computed: (get) => ({get value() { return get(); }}),
    watch: (target, callback) => {if (target && typeof target === 'object') watched.set(target, [...watched.get(target) || [], callback]);},
    defineComponent: (value) => value, _export_sfc: (value) => value,
    modelDisplayName: (model) => model?.display_name || model?.model_id || '',
    modelApi: {list: async () => fixture.models},
    chatRunsApi: {create: async (body) => {fixture.modelCalls.push(body); return {run_id: 'fixture-run'};}},
    readRecovery: (key) => fixture.recovery[key],
    withDeadline: (work) => work, nextTick: (callback) => Promise.resolve().then(() => callback?.()),
    openBlock: () => {}, createVNode: node, createBaseVNode: node, createBlock: node,
    createElementBlock: node, createCommentVNode: () => null,
    UpdateBanner: 'banner', Sidebar: 'sidebar', ClawQrLoginPage: 'login', Teleport: 'teleport', _hoisted_4: {},
    ElMessage: {error: (message) => fixture.notifications.push({kind: 'error', message}), warning: (message) => fixture.notifications.push({kind: 'warning', message})},
    console: {warn: () => {}},
    document: {activeElement: {isConnected: true, focus() {fixture.focusReturns++;}}},
    window: {electronAPI: {
      getDataUrl: async () => 'http://127.0.0.1:18768', getAuthToken: async () => 'synthetic',
      getDefaultWorkspace: async () => {fixture.workspaceReads++; return fixture.workspacePromise ? fixture.workspacePromise : 'D:/synthetic/workspace';},
      skillhubLogin: async () => {fixture.skillLogins++;},
      generateId: async () => 'synthetic-generated-id',
    }},
    clawAuthApi: {
      getSession: async () => fixture.sessionPromise ? fixture.sessionPromise : {loggedIn: fixture.loggedIn, userId: fixture.loggedIn ? 'synthetic-user' : ''},
      introspect: async (parameters) => {fixture.introspects++; fixture.lastIntrospection = parameters; return fixture.introspectionPromise ? fixture.introspectionPromise : {ok: true};},
      syncMcp: async () => {fixture.cloudSyncs++;},
      logout: async () => {fixture.logouts++; fixture.loggedIn = false;},
    },
  });
  vm.runInContext(source, context);
  return {context, fixture, app: context.makeApp()};
}

async function test(name, run) {
  await run(); passed++;
  process.stdout.write('PASS ' + name + '\n');
}

async function main() {
  const output = adapt(original, options);
  await test('atomic adaptation and idempotence', () => {
    assert.equal(output.adapted, true);
    assert.equal(adapt(output.source, options).reason, 'already_adapted');
    assert.equal(adapt(output.source, options).source, output.source);
  });
  await test('startup shell remains byte-identical and identifies its unique main module', () => {
    for (const source of [startupLoader, startupLoader.replace(/\n/g, '\r\n')]) {
      const result = adapt(source, options);
      assert.equal(result.adapted, false); assert.equal(result.source, source);
      assert.equal(result.reason, 'startup_loader'); assert.equal(result.nextModule, './main-native.js');
    }
  });
  await test('ambiguous or nonlocal startup imports keep the original unsupported response', () => {
    for (const source of [startupLoader + '\n' + startupLoader,
      startupLoader.replace('./main-native.js', '../main-native.js'),
      startupLoader.replace('./main-native.js', 'https://example.invalid/main-native.js'),
      startupLoader.replace('startup-notice', 'another-view'),
      startupLoader + '\nvoid import("./another.js");']) {
      const result = adapt(source, options);
      assert.equal(result.adapted, false); assert.equal(result.source, source);
      assert.equal(result.reason, 'unsupported_renderer'); assert.equal(result.nextModule, undefined);
    }
  });
  await test('unknown or ambiguous renderer keeps exact original bytes', () => {
    for (const text of [original.replace('const clawLoggedIn', 'const renamedLogin'),
      original.replace('    async function restoreActiveTask()', '    async function changedRecovery()'),
      original + '\n' + original, original.replace('const modelListVersion = ref(0);', '')]) {
      const result = adapt(text, options);
      assert.equal(result.adapted, false); assert.equal(result.source, text);
    }
  });
  await test('invalid and builtin-only IDs do not enable local mode', () => {
    for (const externalModelIds of [[], ['tiangong-builtin'], ['tiangong-builtin-intranet'], [null], ['a\nb']]) {
      const result = adapt(original, {externalModelIds});
      assert.equal(result.adapted, false); assert.equal(result.source, original);
    }
  });
  await test('ordinary renderer retains original login gate', async () => {
    const {app, fixture} = environment();
    await app.initializeSession();
    assert.equal(app.clawLoggedIn.value, false); assert.equal(fixture.workspaceReads, 0);
  });
  await test('local bootstrap loads workspace and restores once without cloud login', async () => {
    const {app, fixture, context} = environment(output.source);
    await app.initializeSession(); await flush();
    assert.equal(context.window.__tiancaiWorkspace.ready, true);
    assert.equal(app.clawLoggedIn.value, false); assert.equal(app.authReady.value, true);
    assert.equal(app.user.value.display_name, 'AI Gateway');
    assert.equal(context.window.__tiancaiWorkspace.version, 2);
    assert.equal(context.window.__tiancaiWorkspace.scope, 'ai-gateway');
    assert.equal(app.activeTaskRecoveryKey(), 'tiangong-active-task:ai-gateway');
    assert.equal(app.lastModelPreferenceKey(), 'gmclaw-last-model:ai-gateway');
    assert.deepEqual(Array.from(context.accountStatus()), ['本地账号', '本地账号']);
    assert.equal(fixture.workspaceReads, 1); assert.equal(fixture.recoveries, 1);
    assert.equal(fixture.cloudSyncs, 0); assert.equal(fixture.skillLogins, 0);
    assert.equal(context.sidebarLabel(), '登录官方账号');
  });
  await test('cloud introspection never delays local readiness', async () => {
    const {app, context, fixture} = environment(output.source, {loggedIn: true, introspectionPromise: new Promise(() => {})});
    await app.initializeSession(); await flush();
    assert.equal(context.window.__tiancaiWorkspace.ready, true);
    assert.equal(fixture.introspects, 1); assert.equal(app.clawLoggedIn.value, false);
    assert.equal(fixture.lastIntrospection.logoutOnFail, false);
  });
  await test('missing external models report startup error and stay unready', async () => {
    const {app, fixture, context} = environment(output.source, {models: [models[0]]});
    await app.initializeSession();
    assert.equal(context.window.__tiancaiWorkspace.ready, false);
    assert.match(app.authError.value, /外部模型/); assert.equal(fixture.workspaceReads, 0);
  });
  await test('workspace failure cannot claim ready', async () => {
    const {app, fixture, context} = environment(output.source, {scenarioError: true});
    await app.initializeSession();
    assert.equal(context.window.__tiancaiWorkspace.ready, false);
    assert.match(app.bootstrapError.value, /场景/); assert.equal(fixture.recoveries, 0);
  });
  await test('session expiry preserves active local draft and avoids rebootstrap', async () => {
    const {app, fixture, context} = environment(output.source);
    await app.initializeSession();
    const instance = app.chatInstances.value[0];
    app.onSessionInvalid();
    assert.equal(app.chatInstances.value[0], instance); assert.equal(instance.draft, '保留草稿');
    assert.equal(context.window.__tiancaiWorkspace.ready, true);
    assert.equal(fixture.workspaceReads, 1); assert.equal(fixture.logouts, 0);
  });
  await test('session expiry during local bootstrap does not cancel it', async () => {
    let finish;
    const {app, context} = environment(output.source, {workspacePromise: new Promise((resolve) => {finish = resolve;})});
    const work = app.initializeSession(); await flush();
    app.onSessionInvalid(); finish('D:/synthetic/workspace'); await work;
    assert.equal(context.window.__tiancaiWorkspace.ready, true);
    assert.equal(app.bootstrapLoading.value, false);
  });
  await test('optional official login preserves local instance and recovery namespace', async () => {
    const {app, fixture, context} = environment(output.source);
    await app.initializeSession();
    const instance = app.chatInstances.value[0], recovery = app.activeTaskRecoveryKey();
    const localNodes = app.render().children.find((child) => child?.type === 'div').children;
    assert.equal(localNodes[0].props['user-name'], 'AI Gateway');
    assert.equal(localNodes[1].props['user-name'], 'ai-gateway');
    fixture.loggedIn = true; await app.onClawLoginSuccess(); await flush();
    assert.equal(app.clawLoggedIn.value, true); assert.equal(app.user.value.userId, 'synthetic-user');
    assert.equal(app.chatInstances.value[0], instance); assert.equal(fixture.workspaceReads, 1);
    assert.equal(app.activeTaskRecoveryKey(), recovery); assert.equal(context.sidebarLabel(), '退出登录');
    const officialNodes = app.render().children.find((child) => child?.type === 'div').children;
    assert.equal(officialNodes[0].props['user-name'], 'synthetic-user');
    assert.equal(officialNodes[1].props['user-name'], 'ai-gateway');
    assert.deepEqual(Array.from(context.accountStatus()), ['当前已登录', '当前已登录']);
    assert.equal(await context.window.electronAPI.getAuthToken(), 'synthetic');
    await app.onLogout();
    assert.equal(app.clawLoggedIn.value, false); assert.equal(app.chatInstances.value[0], instance);
    assert.equal(context.window.__tiancaiWorkspace.ready, true); assert.equal(fixture.logouts, 1);
  });
  await test('login dialog is cancellable without unmounting local workspace', async () => {
    const {app, fixture, context} = environment(output.source);
    await app.initializeSession();
    const instance = app.chatInstances.value[0];
    await app.onLogout(); // The unauthenticated menu explicitly requests login.
    const dialog = app.render().children.find((child) => child?.type === 'teleport').children[0];
    let modalOpened = 0, prevented = 0;
    dialog.props.ref({open: false, showModal() {modalOpened++;}});
    dialog.props.onCancel({preventDefault() {prevented++;}});
    await flush();
    assert.equal(modalOpened, 1); assert.equal(prevented, 1); assert.equal(fixture.focusReturns, 1);
    assert.equal(app.chatInstances.value[0], instance); assert.equal(fixture.logouts, 0);
    assert.equal(context.window.__tiancaiWorkspace.ready, true);
  });
  await test('new task chooses preferred external model instead of first builtin', async () => {
    const {context} = environment(output.source);
    const panel = context.makePanel({taskId: null, initialModelId: ''});
    await panel.loadDbModels();
    assert.equal(panel.selectedModelId.value, 'external-b'); assert.equal(await panel.ensureModelAvailable(), true);
    assert.equal(panel.dbModels.value.length, 2);
  });
  await test('existing valid external binding is retained', async () => {
    const {context} = environment(output.source);
    const panel = context.makePanel({taskId: 'existing', initialModelId: 'external-a'});
    await panel.loadDbModels();
    assert.equal(panel.selectedModelId.value, 'external-a'); assert.equal(await panel.ensureModelAvailable(), true);
  });
  await test('old unavailable or empty task binding is never silently rewritten', async () => {
    for (const initialModelId of ['tiangong-builtin', 'deleted-external', '']) {
      const {context} = environment(output.source);
      const panel = context.makePanel({taskId: 'existing', initialModelId});
      await panel.loadDbModels();
      assert.equal(panel.selectedModelId.value, initialModelId);
      assert.equal(await panel.ensureModelAvailable(), false); assert.match(panel.runActionError.value, /明确选择/);
      assert.equal(panel.draft.value, '保留草稿');
    }
  });
  await test('dynamic model changes refresh choices atomically without changing active work', async () => {
    const {app, fixture, context} = environment(output.source);
    await app.initializeSession(); await flush();
    const state = context.window.__tiancaiWorkspace;
    const instance = app.chatInstances.value[0];
    const existing = context.makePanel({taskId: 'existing', initialModelId: 'external-a'});
    const unsavedActive = context.makePanel({taskId: null, initialModelId: 'external-a'});
    existing.hasActiveRun.value = true; unsavedActive.hasActiveRun.value = true;
    await existing.loadDbModels(); await unsavedActive.loadDbModels();
    fixture.models.push({model_id: 'external-c', display_name: 'C'});
    const initialVersion = app.modelListVersion.value;
    assert.equal(state.updateModels({externalModelIds: ['external-a', 'external-b', 'external-c'], preferredModelId: 'external-c'}), true);
    assert.equal(app.modelListVersion.value, initialVersion + 1);
    const added = context.makePanel({taskId: null, initialModelId: ''});
    await added.loadDbModels();
    assert.equal(added.selectedModelId.value, 'external-c'); assert.equal(added.dbModels.value.length, 3);
    assert.equal(state.updateModels({externalModelIds: ['external-b', 'external-c'], preferredModelId: 'external-b'}), true);
    await existing.loadDbModels(); await unsavedActive.loadDbModels();
    assert.equal(existing.selectedModelId.value, 'external-a');
    assert.equal(unsavedActive.selectedModelId.value, 'external-a');
    assert.equal(existing.hasActiveRun.value, true); assert.equal(existing.draft.value, '保留草稿');
    const version = app.modelListVersion.value;
    for (const next of [null, {externalModelIds: []}, {externalModelIds: ['tiangong-builtin']},
      {externalModelIds: ['external-b\nbad']}, {externalModelIds: ['external-c'], preferredModelId: 'missing'},
      {externalModelIds: ['external-c'], preferredModelId: 0}, {externalModelIds: ['external-c'], preferredModelId: null}]) {
      assert.equal(state.updateModels(next), false);
    }
    assert.equal(app.modelListVersion.value, version);
    const afterInvalid = context.makePanel({taskId: null, initialModelId: ''});
    await afterInvalid.loadDbModels();
    assert.equal(afterInvalid.selectedModelId.value, 'external-b'); assert.equal(afterInvalid.dbModels.value.length, 2);
    assert.equal(app.chatInstances.value[0], instance); assert.equal(instance.draft, '保留草稿');
    assert.equal(fixture.workspaceReads, 1); assert.equal(fixture.recoveries, 1); assert.equal(state.ready, true);
  });
  await test('stale bootstrap result cannot override a later successful retry', async () => {
    let finish;
    const {app, fixture, context} = environment(output.source, {workspacePromise: new Promise((resolve) => {finish = resolve;})});
    const first = app.initializeSession(); await flush();
    fixture.workspacePromise = null;
    await app.initializeSession(); finish('D:/old/stale'); await first;
    assert.equal(app.defaultWorkspacePath.value, 'D:/synthetic/workspace');
    assert.equal(context.window.__tiancaiWorkspace.ready, true);
  });
  const owned = (session_id, extra = {}) => ({session_id, user_id: 'ai-gateway', agent_id: 'desktop-orchestration', project_id: 'default', ...extra});
  const historyTask = (id, session_id = id) => ({task_id: 'task-' + id, session_id, scenario_id: 'default', model_id: 'external-a', model_options: {temperature: 0.4}});
  await test('owner-scoped list traverses every page and preserves project and task identities', async () => {
    const tasks = Array.from({length: 203}, (_, i) => historyTask('owned-' + i));
    const old = historyTask('legacy'), cloud = historyTask('cloud'), missing = historyTask('orphan');
    const sessions = tasks.map((task) => owned(task.session_id));
    sessions.push(owned(old.session_id, {user_id: 'local'}), owned(cloud.session_id, {user_id: 'official-user'}));
    const {app, fixture, context} = environment(output.source, {sessions,
      scenarios: [{scenario_id: 'default', name: '原项目', work_dir: 'D:/original/project', tasks: [...tasks, old, cloud, missing]}],
      searchResults: [...tasks.slice(0, 1), old, cloud, missing].map((task) => ({task_id: task.task_id, snippet: task.session_id}))});
    await app.initializeSession();
    const visible = app.scenarios.value[0];
    assert.equal(visible.tasks.length, 203); assert.equal(visible.work_dir, 'D:/original/project');
    assert.equal(visible.tasks[202], tasks[202]); assert.equal(fixture.scenarios[0].tasks.length, 206);
    assert.equal(fixture.requests.filter((request) => request.route.startsWith('/data/sessions?')).length, 2);
    const results = await context.fixtureScenarioApi.searchMessages('synthetic');
    assert.deepEqual(Array.from(results, (row) => row.task_id), ['task-owned-0']);
    assert.equal(fixture.requests.some((request) => request.method !== 'GET'), false);
  });
  await test('legacy local history is invisible until explicit owner migration and new key restoration', async () => {
    const task = historyTask('legacy');
    const session = owned(task.session_id, {user_id: 'local', agent_id: 'original-agent', project_id: 'original-project'});
    const {app, fixture} = environment(output.source, {sessions: [session], scenarios: [{scenario_id: 'default', tasks: [task]}],
      recovery: {'tiangong-active-task:default': task.task_id, 'tiangong-active-task:ai-gateway': task.task_id}});
    await app.initializeSession();
    assert.equal(app.scenarios.value[0].tasks.length, 0); assert.equal(fixture.opened.length, 0);
    await app.onOpenTask(task); assert.equal(fixture.opened.length, 0);
    assert.match(fixture.notifications[0].message, /不属于 AI Gateway/);
    session.user_id = 'ai-gateway'; // Represents the separate, explicit Hub migration.
    await app.bootstrapAppData();
    assert.equal(app.scenarios.value[0].tasks[0], task);
    assert.deepEqual(fixture.opened, [task.task_id]);
    assert.equal(session.agent_id, 'original-agent'); assert.equal(session.project_id, 'original-project');
  });
  await test('new conversation registers local ownership before generation and reuses the task on later messages', async () => {
    const {context, fixture} = environment(output.source);
    const project = {id: 'scene-original', name: '原项目', path: 'D:/original/project'};
    const panel = context.makePanel({initialModelId: 'external-a', conversationProject: project});
    await panel.startChatRun();
    assert.equal(fixture.modelCalls.length, 1); assert.equal(fixture.sessions.length, 1);
    const session = fixture.sessions[0], request = fixture.modelCalls[0];
    assert.equal(session.user_id, 'ai-gateway'); assert.equal(session.agent_id, 'desktop-orchestration');
    assert.equal(session.project_id, 'default'); assert.equal(request.session_id, session.session_id);
    assert.equal(request.user_id, 'ai-gateway'); assert.equal(request.project, project); assert.equal(request.model_id, 'external-a');
    assert.equal(fixture.requests[0].route, '/data/tasks'); assert.equal(fixture.requests[1].route, '/data/sessions');
    assert.equal(fixture.requests[2].route, '/data/sessions/' + session.session_id);
    panel.inputValue.value = '第二轮'; await panel.startChatRun();
    assert.equal(fixture.modelCalls.length, 2); assert.equal(fixture.sessions.length, 1);
    assert.equal(fixture.modelCalls[1].session_id, session.session_id);
    assert.equal(fixture.requests.filter((request) => request.route === '/data/tasks').length, 1);
  });
  await test('existing run preserves migrated agent project model and IDs while ignoring cloud account changes', async () => {
    const session = owned('preserved-session', {agent_id: 'agent-from-history', project_id: 'project-from-history', model_name: 'original-model'});
    const {app, context, fixture} = environment(output.source, {sessions: [session], loggedIn: true});
    await app.initializeSession(); await flush();
    const project = {id: 'original-project-object', path: 'D:/original/project', name: '原项目'};
    const panel = context.makePanel({taskId: 'original-task', conversationSessionId: session.session_id,
      initialModelId: 'external-a', conversationProject: project});
    await panel.startChatRun();
    const request = fixture.modelCalls[0];
    assert.equal(request.user_id, 'ai-gateway'); assert.equal(request.agent_id, session.agent_id);
    assert.equal(request.project_id, session.project_id); assert.equal(request.project, project);
    assert.equal(request.session_id, 'preserved-session'); assert.equal(request.model_id, 'external-a');
    assert.equal(app.user.value.userId, 'synthetic-user');
    assert.equal(fixture.requests.some((request) => request.method !== 'GET'), false);
    session.user_id = 'local';
    panel.inputValue.value = '不能发送';
    await assert.rejects(panel.startChatRun(), /不属于 AI Gateway/);
    assert.equal(fixture.modelCalls.length, 1); assert.equal(panel.inputValue.value, '不能发送');
  });
  await test('failed task creation or session registration never falls through to generation or clears input', async () => {
    for (const behavior of [{taskCreateError: true}, {registerError: true}]) {
      const {context, fixture} = environment(output.source, behavior);
      const panel = context.makePanel({initialModelId: 'external-a'});
      await assert.rejects(panel.startChatRun());
      assert.equal(fixture.modelCalls.length, 0); assert.equal(panel.inputValue.value, '待发送消息');
      assert.equal(fixture.scenarios[0].tasks.length, 0);
      assert.equal(fixture.sessions.length, 0);
      assert.equal(fixture.requests.filter((request) => request.method === 'DELETE').length, behavior.registerError ? 1 : 0);
    }
  });
  await test('ownership loading failure stays unready instead of showing unfiltered history', async () => {
    const {app, fixture, context} = environment(output.source, {ownerReadError: true,
      scenarios: [{scenario_id: 'default', tasks: [historyTask('legacy')]}]});
    await app.initializeSession();
    assert.equal(context.window.__tiancaiWorkspace.ready, false);
    assert.equal(app.scenarios.value.length, 0); assert.equal(fixture.opened.length, 0);
  });
  await test('enhanced project removal explains shared memory while individual owned task deletion remains available', async () => {
    const task = historyTask('owned'), legacy = historyTask('legacy');
    const {app, fixture} = environment(output.source, {sessions: [owned(task.session_id), owned(legacy.session_id, {user_id:'local'})],
      scenarios: [{scenario_id:'default', tasks:[task, legacy]}]});
    await app.initializeSession();
    await app.onDeleteScenario('default');
    assert.match(fixture.notifications[0].message, /共享历史和记忆/);
    assert.equal(fixture.requests.some((request) => request.method === 'DELETE'), false);
    await app.onDeleteTask(legacy);
    assert.equal(fixture.requests.some((request) => request.method === 'DELETE'), false);
    await app.onDeleteTask(task);
    assert.equal(fixture.scenarios[0].tasks.length, 1); assert.equal(fixture.scenarios[0].tasks[0], legacy);
  });
  await test('legacy adapter and changed ownership API contract are not overwritten', () => {
    const old = original + '\n/*TIANCAISPACEHUB_EXTERNAL_WORKSPACE_V1*/';
    const legacy = adapt(old, options);
    assert.equal(legacy.source, old); assert.equal(legacy.reason, 'incompatible_adapter');
    const unknown = original.replace('async function req(method, path, body, options)', 'async function changedRequest(method, path, body, options)');
    const unsupported = adapt(unknown, options);
    assert.equal(unsupported.source, unknown); assert.equal(unsupported.reason, 'unsupported_renderer');
  });
  await test('Windows CRLF renderer is supported atomically', () => {
    assert.equal(adapt(original.replace(/\n/g, '\r\n'), options).adapted, true);
  });
  const at = process.argv.indexOf('--renderer');
  if (at !== -1) {
    const rendererPath = path.resolve(process.argv[at + 1]);
    await test('installed startup route reaches one adapted renderer without executing it', () => {
      let renderer = fs.readFileSync(rendererPath, 'utf8');
      let result = adapt(renderer, options);
      const startupLoaderPreserved = result.reason === 'startup_loader';
      if (startupLoaderPreserved) {
        assert.equal(result.adapted, false); assert.equal(result.source, renderer);
        const parsed = spawnSync(process.execPath, ['--input-type=module', '--check'], {input: result.source, encoding: 'utf8'});
        assert.equal(parsed.status, 0, 'unchanged installed startup shell must be valid ECMAScript');
        renderer = fs.readFileSync(path.resolve(path.dirname(rendererPath), result.nextModule), 'utf8');
        result = adapt(renderer, options);
      }
      assert.equal(result.adapted, true);
      assert.equal(result.source.split('/*TIANCAISPACEHUB_EXTERNAL_WORKSPACE_V2*/').length, 2);
      const parsed = spawnSync(process.execPath, ['--input-type=module', '--check'], {input: result.source, encoding: 'utf8'});
      assert.equal(parsed.status, 0, 'adapted installed renderer must be valid ECMAScript');
      // No bundle contents or source excerpts enter logs.
      process.stdout.write(JSON.stringify({rendererBytes: renderer.length, adaptedBytes: result.source.length, startupLoaderPreserved, parsePassed: true}) + '\n');
    });
  }
  process.stdout.write(JSON.stringify({passed, failed: 0, productStarted: false, realModelCalled: false}) + '\n');
}
main().catch((error) => {process.stderr.write(error.stack + '\n'); process.exitCode = 1;});
