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
  'function renderSidebar(_cache = []) { return _cache[58] || (_cache[58] = createBaseVNode("span", { class: "sidebar-user-menu-action-label" }, "退出登录", -1)); }',
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
  '    return {loadDbModels, ensureModelAvailable, selectedModelId, selectedModelName, dbModels, runActionError, showModelDropdown, draft, hasActiveRun};',
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
  '    async function refreshScenarios() { fixture.scenarioReads++; if (fixture.scenarioError) return false; scenarios.value = [{tasks: []}]; return true; }',
  '    function ensureInitialInstance() { if (!chatInstances.value.length) chatInstances.value.push({taskId: null, draft: "保留草稿", modelId: ""}); }',
  '    async function restoreActiveTask() { fixture.recoveries++; }',
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
  '            onOpenUserPanel: _cache[0] || (_cache[0] = ($event) => showUserPanel.value = true),',
  '          })',
  '        ])) : createCommentVNode("", true)',
  '      ]);',
  '    }',
  '    return {initializeSession, bootstrapAppData, onSessionInvalid, onClawLoginSuccess, onLogout, render, clawLoggedIn, authReady, authError, bootstrapError, bootstrapLoading, user, chatInstances, modelListVersion, defaultWorkspacePath, lastModelPreferenceKey, activeTaskRecoveryKey, syncMcpCatalogOnBootstrap};',
  '  }',
  '});',
  'const App = /* @__PURE__ */ _export_sfc(Root, []);',
  'globalThis.makeApp = () => App.setup({});',
  'globalThis.makePanel = (props) => ChatPanel.setup(props);',
  'globalThis.sidebarLabel = () => renderSidebar().children;',
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
    focusReturns: 0, ...behavior};
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
  const context = vm.createContext({
    fixture, ref, computed: (get) => ({get value() { return get(); }}),
    watch: (target, callback) => {if (target && typeof target === 'object') watched.set(target, [...watched.get(target) || [], callback]);},
    defineComponent: (value) => value, _export_sfc: (value) => value,
    modelDisplayName: (model) => model?.display_name || model?.model_id || '',
    modelApi: {list: async () => fixture.models},
    withDeadline: (work) => work, nextTick: (callback) => Promise.resolve().then(() => callback?.()),
    openBlock: () => {}, createVNode: node, createBaseVNode: node, createBlock: node,
    createElementBlock: node, createCommentVNode: () => null,
    UpdateBanner: 'banner', Sidebar: 'sidebar', ClawQrLoginPage: 'login', Teleport: 'teleport', _hoisted_4: {},
    ElMessage: {error: () => {}},
    document: {activeElement: {isConnected: true, focus() {fixture.focusReturns++;}}},
    window: {electronAPI: {
      getDataUrl: async () => 'http://127.0.0.1:18768', getAuthToken: async () => 'synthetic',
      getDefaultWorkspace: async () => {fixture.workspaceReads++; return fixture.workspacePromise ? fixture.workspacePromise : 'D:/synthetic/workspace';},
      skillhubLogin: async () => {fixture.skillLogins++;},
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
    assert.equal(app.user.value.display_name, '本地工作区');
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
    fixture.loggedIn = true; await app.onClawLoginSuccess(); await flush();
    assert.equal(app.clawLoggedIn.value, true); assert.equal(app.user.value.userId, 'synthetic-user');
    assert.equal(app.chatInstances.value[0], instance); assert.equal(fixture.workspaceReads, 1);
    assert.equal(app.activeTaskRecoveryKey(), recovery); assert.equal(context.sidebarLabel(), '退出登录');
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
      assert.equal(result.source.split('/*TIANCAISPACEHUB_EXTERNAL_WORKSPACE_V1*/').length, 2);
      const parsed = spawnSync(process.execPath, ['--input-type=module', '--check'], {input: result.source, encoding: 'utf8'});
      assert.equal(parsed.status, 0, 'adapted installed renderer must be valid ECMAScript');
      // No bundle contents or source excerpts enter logs.
      process.stdout.write(JSON.stringify({rendererBytes: renderer.length, adaptedBytes: result.source.length, startupLoaderPreserved, parsePassed: true}) + '\n');
    });
  }
  process.stdout.write(JSON.stringify({passed, failed: 0, productStarted: false, realModelCalled: false}) + '\n');
}
main().catch((error) => {process.stderr.write(error.stack + '\n'); process.exitCode = 1;});
