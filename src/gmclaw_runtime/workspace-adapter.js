// Pure, atomic renderer adapter. The caller only supplies Hub-owned model IDs.
// It never reads credentials, edits an installation, or marks an account logged in.
(function adaptTiangongWorkspace(source, options = {}) {
  const marker = 'TIANCAISPACEHUB_EXTERNAL_WORKSPACE_V2';
  const unchanged = (reason) => ({source, adapted: false, reason});
  if (typeof source !== 'string') return unchanged('invalid_source');
  if (source.includes(marker)) return unchanged('already_adapted');
  if (/TIANCAISPACEHUB_EXTERNAL_WORKSPACE_V\d+/.test(source)) return unchanged('incompatible_adapter');
  const ids = options && options.externalModelIds;
  if (!Array.isArray(ids) || !ids.length || ids.some((id) => typeof id !== 'string' ||
      !id || id.length > 128 || /[\u0000-\u001f]/.test(id) ||
      id === 'tiangong-builtin' || id === 'tiangong-builtin-intranet')) {
    return unchanged('invalid_models');
  }
  const selectedIds = [...new Set(ids)];
  const preferred = selectedIds.includes(options.preferredModelId) ? options.preferredModelId : selectedIds[0];
  let changed = source.replace(/\r\n/g, '\n');
  const replace = (text, before, after, count = 1) => {
    const parts = text.split(before);
    if (parts.length !== count + 1) throw new Error('unsupported_renderer');
    return parts.join(after);
  };
  const section = (text, begin, end, replacement) => {
    if (text.split(begin).length !== 2 || text.split(end).length !== 2) throw new Error('unsupported_renderer');
    const start = text.indexOf(begin), finish = text.indexOf(end, start + begin.length);
    if (finish < start) throw new Error('unsupported_renderer');
    return text.slice(0, start) + replacement + text.slice(finish);
  };
  try {
    const appHeader = /const ([\w$]+) = \/\* @__PURE__ \*\/ defineComponent\(\{\n  __name: "App",\n  setup\(__props\) \{/g;
    const appMatches = [...changed.matchAll(appHeader)];
    const panelHeader = /const ([\w$]+) = \/\* @__PURE__ \*\/ defineComponent\(\{\n  __name: "ChatPanel",/g;
    const panelMatches = [...changed.matchAll(panelHeader)];
    // Current packages declare a small startup/error shell in index.html. Keep
    // that shell intact and follow only its unique local renderer import.
    if (!appMatches.length && !panelMatches.length) {
      const loaders = [...changed.matchAll(/void __vitePreload\(\(\) => import\("(\.\/main-[\w-]+\.js)"\), true \? __vite__mapDeps\(\[[\d,]+\]\) : void 0, import\.meta\.url\)\.catch\(\(error\) => \{/g)];
      if (loaders.length === 1 && [...changed.matchAll(/\bimport\s*\(/g)].length === 1 &&
          changed.includes('document.getElementById("startup-notice")') &&
          changed.includes('window.addEventListener("app-mounted",') &&
          changed.includes('function showFailure() {')) {
        return {source, adapted: false, reason: 'startup_loader', nextModule: loaders[0][1]};
      }
    }
    if (appMatches.length !== 1 || panelMatches.length !== 1) return unchanged('unsupported_renderer');
    const appStart = appMatches[0].index;
    const appEnd = changed.indexOf('\nconst App = /* @__PURE__ */ _export_sfc(', appStart);
    const panelStart = panelMatches[0].index;
    const panelEnd = changed.indexOf('\nconst ChatPanel = /* @__PURE__ */ _export_sfc(', panelStart);
    if (panelStart >= appStart || panelEnd < panelStart || panelEnd >= appStart || appEnd < appStart) {
      return unchanged('unsupported_renderer');
    }
    for (const required of ['const scenarioApi = {', 'async function req(method, path, body, options)',
      'list: () => req("GET", "/data/scenarios").then((r) => r.scenarios)',
      'searchMessages: (query) => req("GET", `/data/messages/search?q=${enc(query)}`).then((r) => r.results)',
      'createTask: (scenarioId, title = "新任务", modelId = "", modelOptions = {}) => req("POST", "/data/tasks", { scenario_id: scenarioId, title, model_id: modelId, model_options: modelOptions })',
      'deleteTask: (id) => req("DELETE", `/data/tasks/${enc(id)}`)']) {
      if (!changed.includes(required)) throw new Error('unsupported_renderer');
    }
    let app = changed.slice(appStart, appEnd);
    let panel = changed.slice(panelStart, panelEnd);
    for (const required of ['async function checkClawSession(', 'async function bootstrapAppData()',
      'async function restoreActiveTask()', 'async function onClawLoginSuccess()', 'function onSessionInvalid()',
      'async function onLogout()', 'const scenarios = ref([]);', 'const chatInstances = ref([]);']) {
      if (!app.includes(required)) throw new Error('unsupported_renderer');
    }
    for (const required of ['async function loadDbModels()', 'async function ensureModelAvailable()']) {
      if (!panel.includes(required)) throw new Error('unsupported_renderer');
    }
    for (const required of ['function isTaskVisible(', 'function activeTaskRecoveryKey()', 'function lastModelPreferenceKey()']) {
      if (!app.includes(required)) throw new Error('unsupported_renderer');
    }
    app = replace(app, '      display_name: "Admin",', '      display_name: "AI Gateway",');
    app = replace(app, '    const scenarios = ref([]);',
      '    const scenarios = ref([]);\n' +
      '    __tcwVisibleTaskIds = () => new Set(scenarios.value.flatMap((scenario) => scenario.tasks.map((task) => task.task_id)));');
    app = replace(app, '    const clawLoggedIn = ref(false);',
      '    const clawLoggedIn = ref(false);\n' +
      '    const hubWorkspaceReady = computed(() => clawLoggedIn.value || __tcwLocalReady.value);\n' +
      '    const hubLoginVisible = ref(false);\n' +
      '    let hubPreviousFocus = null;\n' +
      '    function hubOpenLogin() { hubPreviousFocus = document.activeElement; hubLoginVisible.value = true; }\n' +
      '    function hubCloseLogin() { hubLoginVisible.value = false; void nextTick(() => hubPreviousFocus?.isConnected && hubPreviousFocus.focus?.()); }\n');
    app = replace(app, '    const modelListVersion = ref(0);',
      '    const modelListVersion = ref(0);\n' +
      '    __tcwState.updateModels = (next) => {\n' +
      '      const ids = next?.externalModelIds;\n' +
      '      if (!Array.isArray(ids) || !ids.length || ids.some((id) => typeof id !== "string" || !id || id.length > 128 || /[\\u0000-\\u001f]/.test(id) || id === "tiangong-builtin" || id === "tiangong-builtin-intranet")) return false;\n' +
      '      const nextIds = [...new Set(ids)];\n' +
      '      if (next.preferredModelId !== undefined && typeof next.preferredModelId !== "string") return false;\n' +
      '      const preferred = next.preferredModelId || nextIds[0];\n' +
      '      if (typeof preferred !== "string" || !nextIds.includes(preferred)) return false;\n' +
      '      __tcwModelIds = nextIds;\n' +
      '      __tcwPreferredModel = preferred;\n' +
      '      modelListVersion.value++;\n' +
      '      return true;\n' +
      '    };\n' +
      '    watch(clawLoggedIn, (loggedIn) => { __tcwOfficialLoggedIn.value = loggedIn; modelListVersion.value++; }, { flush: "sync" });');
    app = replace(app, 'clawAuthApi.introspect({ logoutOnFail: true })',
      'clawAuthApi.introspect({ logoutOnFail: false })');
    // These seven gates control local workspace lifecycle, never server auth.
    app = replace(app, 'computed(() => clawLoggedIn.value ? scenarios.value.reduce',
      'computed(() => hubWorkspaceReady.value ? scenarios.value.reduce');
    app = replace(app, 'return clawLoggedIn.value && windowFocused.value',
      'return hubWorkspaceReady.value && windowFocused.value');
    app = replace(app, 'if (attempt !== bootstrapAttempt || !clawLoggedIn.value) return;',
      'if (attempt !== bootstrapAttempt || !hubWorkspaceReady.value) return;', 2);
    app = replace(app, 'watch(() => [clawLoggedIn.value, activeInst.value?.taskId]',
      'watch(() => [hubWorkspaceReady.value, activeInst.value?.taskId]');
    app = replace(app, ')) : !clawLoggedIn.value ? (openBlock(), createBlock(ClawQrLoginPage, {',
      ')) : !hubWorkspaceReady.value ? (openBlock(), createBlock(ClawQrLoginPage, {');
    app = replace(app, '})) : authReady.value && clawLoggedIn.value ? (openBlock(), createElementBlock("div",',
      '})) : authReady.value && hubWorkspaceReady.value ? (openBlock(), createElementBlock("div",');
    app = section(app, '    async function initializeSession() {', '    async function bootstrapAppData() {',
      '    async function initializeSession() {\n' +
      '      const attempt = ++authAttempt;\n' +
      '      authReady.value = false;\n' +
      '      authError.value = "";\n' +
      '      try {\n' +
      '        if (!window.electronAPI?.getDataUrl || !window.electronAPI?.getAuthToken) throw new Error("应用组件未就绪，请重启天工后重试。");\n' +
      '        const models = await withDeadline(modelApi.list(), 15e3, "读取外部模型超时，请重试。");\n' +
      '        if (attempt !== authAttempt) return;\n' +
      '        if (!Array.isArray(models) || !models.some(__tcwExternalModel)) throw new Error("Hub 配置的外部模型尚未出现在天工中，请回 Hub 保存模型后重新增强启动。");\n' +
      '        __tcwLocalReady.value = true;\n' +
      '        authReady.value = true;\n' +
      '        await bootstrapAppData();\n' +
      '        if (attempt !== authAttempt) return;\n' +
      '        void checkClawSession(attempt).then((loggedIn) => {\n' +
      '          if (attempt !== authAttempt || !loggedIn) return;\n' +
      '          void syncMcpCatalogOnBootstrap();\n' +
      '          void window.electronAPI?.skillhubLogin?.().catch(() => {});\n' +
      '        }).catch(() => {});\n' +
      '      } catch (error) {\n' +
      '        if (attempt !== authAttempt) return;\n' +
      '        authError.value = error instanceof Error ? error.message : "本地工作区未能加载，请重试。";\n' +
      '        __tcwState.ready = false;\n' +
      '        __tcwState.error = authError.value;\n' +
      '      }\n' +
      '    }\n');
    app = replace(app, '      void syncMcpCatalogOnBootstrap();\n      try {\n        void window.electronAPI?.skillhubLogin?.().catch',
      '      if (clawLoggedIn.value) void syncMcpCatalogOnBootstrap();\n      try {\n        if (clawLoggedIn.value) void window.electronAPI?.skillhubLogin?.().catch');
    app = replace(app, '        if (attempt === bootstrapAttempt) bootstrapLoading.value = false;',
      '        if (attempt === bootstrapAttempt) {\n' +
      '          bootstrapLoading.value = false;\n' +
      '          __tcwState.ready = hubWorkspaceReady.value && !bootstrapError.value;\n' +
      '          __tcwState.error = bootstrapError.value;\n' +
      '        }');
    app = replace(app, '    async function syncMcpCatalogOnBootstrap() {\n',
      '    async function syncMcpCatalogOnBootstrap() {\n      if (!clawLoggedIn.value) return;\n');
    app = section(app, '    function onSessionInvalid() {', '    async function onClawLoginSuccess() {',
      '    function onSessionInvalid() {\n' +
      '      authAttempt++;\n' +
      '      mcpSyncAttempt++;\n' +
      '      clawLoggedIn.value = false;\n' +
      '      user.value = { ...DEFAULT_USER, userId: "" };\n' +
      '      mcpSyncError.value = "";\n' +
      '      if (!__tcwLocalReady.value) { bootstrapAttempt++; scenarioRefreshRequest++; }\n' +
      '    }\n');
    app = replace(app, '      await bootstrapAppData();\n    }\n    async function onLogout() {',
      '      if (__tcwLocalReady.value && chatInstances.value.length) {\n' +
      '        void syncMcpCatalogOnBootstrap();\n' +
      '        void window.electronAPI?.skillhubLogin?.().catch(() => {});\n' +
      '      } else await bootstrapAppData();\n' +
      '      hubCloseLogin();\n' +
      '    }\n    async function onLogout() {');
    app = section(app, '    async function onLogout() {', '    async function onCheckUpdate() {',
      '    async function onLogout() {\n' +
      '      if (!clawLoggedIn.value) { hubOpenLogin(); return; }\n' +
      '      showUserPanel.value = false;\n' +
      '      try { await clawAuthApi.logout(); }\n' +
      '      catch (error) { ElMessage.error("退出登录未能确认，请稍后重试。"); return; }\n' +
      '      onSessionInvalid();\n' +
      '    }\n');
    // Local preferences stay in one namespace across optional cloud login changes.
    const templateEnd = String.fromCharCode(96) + ';';
    app = replace(app, String.fromCharCode(36) + '{user.value.userId || "default"}' + templateEnd,
      String.fromCharCode(36) + '{__tcwScope}' + templateEnd, 2);
    app = replace(app,
      '"user-name": user.value.userId || user.value.display_name || user.value.username,\n            "running-task-ids":',
      '"user-name": __tcwOfficialLoggedIn.value ? user.value.userId || user.value.display_name || user.value.username : "AI Gateway",\n            "running-task-ids":');
    app = replace(app,
      '"user-name": user.value.userId || user.value.display_name || user.value.username,\n                  "is-active":',
      '"user-name": __tcwScope,\n                  "is-active":');
    app = replace(app, '    async function onOpenTask(task, messageId, query) {\n',
      '    async function onOpenTask(task, messageId, query) {\n' +
      '      try { await __tcwRequireSession(task.session_id); }\n' +
      '      catch (error) { ElMessage.error(error.message || "该任务不属于 AI Gateway，请在 Hub 历史管理中确认归属。"); return; }\n');
    app = replace(app, '    async function onDeleteTask(task) {\n',
      '    async function onDeleteTask(task) {\n' +
      '      try { await __tcwRequireSession(task.session_id); }\n' +
      '      catch (error) { ElMessage.error(error.message || "会话归属未能确认，尚未删除任务。"); return; }\n');
    // Native project removal deletes the whole shared .tiangong directory.
    // Keep that entry explicit, while the existing per-task deletion stays usable.
    app = section(app, '    async function onDeleteScenario(scenarioId) {', '    async function onRenameTask(task, title) {',
      '    async function onDeleteScenario(scenarioId) {\n' +
      '      ElMessage.warning("项目包含共享历史和记忆，请分别删除当前可见任务；本地工作区保留项目及工作目录。");\n' +
      '    }\n');
    app = replace(app, 'onOpenUserPanel: _cache[0] || (_cache[0] = ($event) => showUserPanel.value = true),',
      'onOpenUserPanel: _cache[0] || (_cache[0] = ($event) => { if (clawLoggedIn.value) showUserPanel.value = true; else hubOpenLogin(); }),');
    app = replace(app, '        createVNode(UpdateBanner, { class: "app-root" }),',
      '        createVNode(UpdateBanner, { class: "app-root" }),\n' +
      '        hubLoginVisible.value ? createVNode(Teleport, { to: "body" }, [\n' +
      '          createVNode("dialog", {\n' +
      '            "aria-label": "登录天工官方账号",\n' +
      '            ref: (element) => { if (element && !element.open) element.showModal(); },\n' +
      '            onCancel: (event) => { event.preventDefault(); hubCloseLogin(); },\n' +
      '            onClose: hubCloseLogin,\n' +
      '            style: { width: "min(640px, calc(100vw - 32px))", maxHeight: "calc(100vh - 32px)", padding: "0", border: "1px solid var(--el-border-color)", borderRadius: "12px", background: "var(--el-bg-color)", color: "var(--el-text-color-primary)", overflow: "auto" }\n' +
      '          }, [\n' +
      '            createVNode("header", { style: { display: "flex", alignItems: "center", justifyContent: "space-between", padding: "12px 16px" } }, [\n' +
      '              createVNode("strong", null, "登录天工官方账号"),\n' +
      '              createVNode("button", { type: "button", autofocus: true, onClick: hubCloseLogin, style: { padding: "8px 12px", color: "inherit", background: "transparent", border: "1px solid var(--el-border-color)", borderRadius: "6px", cursor: "pointer" } }, "返回本地工作区")\n' +
      '            ]),\n' +
      '            createVNode(ClawQrLoginPage, { onSuccess: onClawLoginSuccess })\n' +
      '          ])\n' +
      '        ]) : createCommentVNode("", true),');
    panel = replace(panel, '        const models = await modelApi.list();',
      '        const availableModels = await modelApi.list();\n' +
      '        const models = __tcwOfficialLoggedIn.value ? availableModels : availableModels.filter(__tcwExternalModel);');
    panel = replace(panel, '          selectedModelId.value = models[0]?.model_id || "";',
      '          if (!props.taskId && !hasActiveRun.value) selectedModelId.value = __tcwChooseExternal(models) || (__tcwOfficialLoggedIn.value ? models[0]?.model_id || "" : "");');
    panel = replace(panel, '      return modelDisplayName(m || dbModels.value[0]);',
      '      return m ? modelDisplayName(m) : selectedModelId.value ? "原模型暂不可用，请选择模型" : modelDisplayName(dbModels.value[0]);');
    panel = replace(panel, '      if (dbModels.value.some((model) => model.model_id === selectedModelId.value && (!isBuiltinModel(model) || model.is_configured))) return true;',
      '      if (!__tcwOfficialLoggedIn.value && !dbModels.value.some((model) => model.model_id === selectedModelId.value && __tcwExternalModel(model))) {\n' +
      '        runActionError.value = "当前使用本地工作区，请明确选择已配置的外部模型；如需官方模型，请从头像入口登录天工账号。";\n' +
      '        showModelDropdown.value = true;\n' +
      '        return false;\n' +
      '      }\n' +
      '      if (dbModels.value.some((model) => model.model_id === selectedModelId.value && (!isBuiltinModel(model) || model.is_configured))) return true;');
    panel = replace(panel, '          console.warn("[chat] 自动创建默认任务失败:", e);',
      '          console.warn("[chat] 自动创建默认任务失败:", e);\n          throw e;');
    panel = replace(panel, '      const chatId = recovery?.run.chat_id || await window.electronAPI.generateId();',
      '      const hubSession = await __tcwRequireSession(sessionId.value);\n' +
      '      const chatId = recovery?.run.chat_id || await window.electronAPI.generateId();');
    panel = replace(panel, '          user_id: "local",\n          agent_id: "desktop-orchestration",\n          project_id: "default",',
      '          user_id: hubSession.user_id,\n          agent_id: hubSession.agent_id,\n          project_id: hubSession.project_id,');
    changed = replace(changed,
      '_cache[58] || (_cache[58] = createBaseVNode("span", { class: "sidebar-user-menu-action-label" }, "退出登录", -1))',
      'createBaseVNode("span", { class: "sidebar-user-menu-action-label" }, __tcwOfficialLoggedIn.value ? "退出登录" : "登录官方账号")');
    for (const [cache, className] of [[54, 'sidebar-user-status'], [55, 'sidebar-user-menu-status']]) {
      changed = replace(changed,
        '_cache[' + cache + '] || (_cache[' + cache + '] = createBaseVNode("small", { class: "' + className + '" }, "当前已登录", -1))',
        'createBaseVNode("small", { class: "' + className + '" }, __tcwOfficialLoggedIn.value ? "当前已登录" : "本地账号")');
    }
    const helpers = '\n/*' + marker + '*/\n' +
      'let __tcwModelIds = ' + JSON.stringify(selectedIds) + ';\n' +
      'let __tcwPreferredModel = ' + JSON.stringify(preferred) + ';\n' +
      'const __tcwOfficialLoggedIn = ref(false);\n' +
      'const __tcwLocalReady = ref(false);\n' +
      'const __tcwScope = "ai-gateway";\n' +
      'const __tcwState = window.__tiancaiWorkspace = { version: 2, scope: __tcwScope, mode: "external", ready: false, error: "" };\n' +
      'function __tcwExternalModel(model) { return !!model && __tcwModelIds.includes(model.model_id) && !isBuiltinModel(model); }\n' +
      'function __tcwChooseExternal(models) { return models.find((model) => model.model_id === __tcwPreferredModel && __tcwExternalModel(model))?.model_id || models.find(__tcwExternalModel)?.model_id || ""; }\n' +
      String.raw`
const __tcwNativeScenarios = { list: scenarioApi.list, searchMessages: scenarioApi.searchMessages, createTask: scenarioApi.createTask, deleteTask: scenarioApi.deleteTask };
let __tcwVisibleTaskIds = () => new Set();
async function __tcwRequireSession(id) {
  if (typeof id !== "string" || !id) throw new Error("任务缺少会话归属，请在 Hub 历史管理中核对。");
  const session = await req("GET", "/data/sessions/" + enc(id));
  if (session?.session_id !== id || session.user_id !== __tcwScope) throw new Error("该任务不属于 AI Gateway，请先在 Hub 历史管理中迁入。");
  if (typeof session.agent_id !== "string" || typeof session.project_id !== "string") throw new Error("会话的智能体或项目身份不完整，请在 Hub 历史管理中核对。");
  return session;
}
async function __tcwOwnedSessionIds() {
  const result = new Set();
  const limit = 200;
  for (let offset = 0; ; offset += limit) {
    const page = await req("GET", "/data/sessions?user_id=" + enc(__tcwScope) + "&limit=" + limit + "&offset=" + offset);
    if (!Array.isArray(page?.sessions)) throw new Error("读取本地会话归属失败，请重试加载。");
    for (const session of page.sessions) if (session.user_id === __tcwScope) result.add(session.session_id);
    if (page.sessions.length < limit) return result;
  }
}
scenarioApi.list = async () => {
  const [scenarios, owned] = await Promise.all([__tcwNativeScenarios.list(), __tcwOwnedSessionIds()]);
  return scenarios.map((scenario) => ({ ...scenario, tasks: scenario.tasks.filter((task) => owned.has(task.session_id)) }));
};
scenarioApi.searchMessages = async (query) => {
  const results = await __tcwNativeScenarios.searchMessages(query);
  const visible = __tcwVisibleTaskIds();
  return results.filter((row) => visible.has(row.task_id));
};
scenarioApi.createTask = async (...args) => {
  const task = await __tcwNativeScenarios.createTask(...args);
  try {
    await req("POST", "/data/sessions", { session_id: task.session_id, title: task.title, user_id: __tcwScope,
      agent_id: "desktop-orchestration", project_id: "default", model_name: task.model_id || "", max_steps: 1000 });
  } catch (error) {
    // Only this new, still empty task is rolled back; no legacy owner is changed.
    await __tcwNativeScenarios.deleteTask(task.task_id).catch(() => {});
    throw new Error("本地会话归属未能保存，消息尚未发送，请重试。");
  }
  return task;
};
`;
    const originalPanel = source.replace(/\r\n/g, '\n').slice(panelStart, panelEnd);
    const originalApp = source.replace(/\r\n/g, '\n').slice(appStart, appEnd);
    changed = replace(changed, originalPanel, panel);
    changed = replace(changed, originalApp, helpers + app);
    return {source: changed, adapted: true, reason: 'adapted'};
  } catch {
    return unchanged('unsupported_renderer');
  }
})
