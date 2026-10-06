async function (messages, streaming, confirmation, welcome, older, scroll, loadingOlder, modelsLoaded, localTaskIdRef, sessionIdRef, nativeCleanup, instances, scenarios, openingTasks, deletingTaskIds, props, taskId, sessionId, hubRowIds) {
  // Requests run inside the verified renderer; only an outcome is returned.
  // Scope discovery may transiently receive primitive values in Hub memory.
  const isRef = (value) => value && value.__v_isRef === true;
  const isMutableObject = (value) => value !== null && typeof value === 'object' && value.__v_isReadonly !== true && !Object.isFrozen(value);
  const canAssign = (target, key, requireExisting = false) => {
    if (!isMutableObject(target)) return false;
    let owner = target;
    for (let depth = 0; owner && depth < 8; depth++, owner = Object.getPrototypeOf(owner)) {
      const descriptor = Object.getOwnPropertyDescriptor(owner, key);
      if (!descriptor) continue;
      if ('value' in descriptor) return descriptor.writable === true && (owner === target || Object.isExtensible(target));
      return typeof descriptor.set === 'function';
    }
    return !owner && !requireExisting && Object.isExtensible(target);
  };
  const isWritableRef = (value) => isRef(value) && canAssign(value, 'value', true);
  const canAssignCache = (target, key) => {
    if (!canAssign(target, key)) return false;
    if (target.__v_isReactive !== true) return true;
    for (let owner = target, depth = 0; owner && depth < 8; depth++, owner = Object.getPrototypeOf(owner)) {
      const descriptor = Object.getOwnPropertyDescriptor(owner, key);
      if (descriptor) return !('value' in descriptor) || !isRef(descriptor.value) || isWritableRef(descriptor.value);
    }
    return true;
  };
  const isBooleanRef = (value) => isRef(value) && typeof value.value === 'boolean';
  const isNullableStringRef = (value) => isRef(value) && (value.value === null || typeof value.value === 'string');
  const isConfirmationRef = (value) => isRef(value) && (value.value === null || (typeof value.value === 'object' && !Array.isArray(value.value)));
  const hasPanelProps = (value) => value && typeof value.taskId === 'string' && typeof value.conversationSessionId === 'string' && typeof value.isActive === 'boolean';
  if (!isRef(instances) || !Array.isArray(instances.value) || !isWritableRef(scenarios) || !Array.isArray(scenarios.value)) return 'app_fields';
  const opening = isRef(openingTasks) ? openingTasks.value : openingTasks;
  const deleting = isRef(deletingTaskIds) ? deletingTaskIds.value : deletingTaskIds;
  if (!(opening instanceof Set) || !(deleting instanceof Set)) return 'app_sets';
  if (!Array.isArray(hubRowIds) || hubRowIds.length > 128 || hubRowIds.some((id) => !Number.isSafeInteger(id) || id <= 0)) return 'incompatible';
  const ownedRows = new Set(hubRowIds);
  const instanceArray = instances.value;
  const scenarioArray = scenarios.value;
  const messageArray = props ? messages?.value : null;
  const bound = instanceArray.filter((instance) => instance.taskId === taskId);
  if (bound.length > 1 || bound.some((instance) => instance.sessionId !== sessionId)) return 'instance_identity';
  if (props && (props.taskId !== taskId || props.conversationSessionId !== sessionId)) return 'panel_identity';
  if (props && (!hasPanelProps(props) || !isRef(messages) || !Array.isArray(messages.value) || ![messages, welcome, older].every(isWritableRef) || ![streaming, welcome, older, loadingOlder, modelsLoaded].every(isBooleanRef) || !isConfirmationRef(confirmation) || ![localTaskIdRef, sessionIdRef].every(isNullableStringRef) || typeof scroll !== 'function' || (nativeCleanup !== null && typeof nativeCleanup !== 'function'))) return 'panel_fields';
  if (props && bound.length !== 1) return 'changed';
  const cacheFields = ['seedMessages', 'messageSummaries', 'hasOlderMessages'];
  const cacheSnapshot = props ? cacheFields.map((key) => bound[0][key]) : null;
  const cacheWritable = () => !props || (isMutableObject(bound[0]) && cacheFields.every((key) => canAssignCache(bound[0], key)));
  const refsWritable = () => isWritableRef(scenarios) && (!props || [messages, welcome, older].every(isWritableRef));
  if (!cacheWritable()) return 'app_fields';
  const idle = () => !opening.has(taskId) && !deleting.has(taskId) && (!props || (streaming.value === false && confirmation.value === null && loadingOlder.value === false && modelsLoaded.value === true && nativeCleanup === null && localTaskIdRef.value === taskId && (sessionIdRef.value || props.conversationSessionId) === sessionId && !messages.value.some((m) => m.awaitingConfirm)));
  if (!idle()) return 'busy';
  const snapshot = props ? JSON.stringify(messages.value) : null;
  const scenarioSnapshot = JSON.stringify(scenarios.value);
  if ((snapshot && snapshot.length > 8 * 1024 * 1024) || scenarioSnapshot.length > 8 * 1024 * 1024) return 'limited';
  const api = window.electronAPI;
  if (typeof api?.getAuthToken !== 'function' || typeof api?.getDataUrl !== 'function') return 'api_fields';
  if (await api.getDataUrl() !== 'http://127.0.0.1:18768') return 'data_endpoint';
  const authorization = await api.getAuthToken();
  if (typeof authorization !== 'string' || !authorization || authorization.length > 8192) return 'authorization';
  const abort = new AbortController();
  const timer = setTimeout(() => abort.abort(), 4500);
  let messageBytes = 0;
  const get = async (path, maximum) => {
    const response = await fetch('http://127.0.0.1:18768' + path, {
      method: 'GET', redirect: 'error', cache: 'no-store', signal: abort.signal,
      headers: {'Authorization': 'Bearer ' + authorization},
    });
    if (!response.ok || Number(response.headers.get('content-length') || 0) > maximum) throw new Error('data unavailable');
    const reader = response.body.getReader();
    const chunks = [];
    let size = 0;
    while (true) {
      const {done, value} = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > maximum) { await reader.cancel(); throw new Error('data size'); }
      chunks.push(value);
    }
    if (path.includes('/messages?')) {
      messageBytes += size;
      if (messageBytes > 8 * 1024 * 1024) throw new Error('message page size');
    }
    const bytes = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
    return JSON.parse(new TextDecoder('utf-8', {fatal: true}).decode(bytes));
  };
  try {
    const enc = encodeURIComponent;
    const [list, page, summary] = await Promise.all([
      get('/data/scenarios', 8 * 1024 * 1024),
      get('/data/tasks/' + enc(taskId) + '/messages?limit=100', 8 * 1024 * 1024),
      get('/data/tasks/' + enc(taskId) + '/messages/summaries', 8 * 1024 * 1024),
    ]);
    if (!Array.isArray(list.scenarios) || list.scenarios.length > 20000 || list.scenarios.some((scenario) => !Array.isArray(scenario.tasks))) return 'scenario_shape';
    if (!Array.isArray(page.messages) || page.messages.length > 100 || typeof page.has_more !== 'boolean') return 'page_shape';
    if (!Array.isArray(summary.messages) || summary.messages.length > 20000) return 'summary_shape';
    const tasks = list.scenarios.flatMap((scenario) => Array.isArray(scenario.tasks) ? scenario.tasks : []);
    if (tasks.length > 20000) return 'scenario_shape';
    const task = tasks.filter((task) => task.task_id === taskId);
    if (task.length !== 1 || task[0].session_id !== sessionId) return 'task_identity';
    const validPage = (value) => {
      if (!Array.isArray(value.messages) || value.messages.length > 100 || typeof value.has_more !== 'boolean') return false;
      let previous = 0;
      for (const message of value.messages) {
        if (!Number.isSafeInteger(message.id) || message.id <= previous || message.task_id !== taskId || !['user', 'system'].includes(message.role) || typeof message.content !== 'string' || typeof message.created_at !== 'string' || (message.thinking_steps != null && !Array.isArray(message.thinking_steps)) || !Array.isArray(message.files)) return false;
        previous = message.id;
      }
      return true;
    };
    if (!validPage(page)) return 'page_shape';
    let previousId = 0;
    for (const message of summary.messages) {
      if (!Number.isSafeInteger(message.id) || message.id <= previousId || !['user', 'system'].includes(message.role) || typeof message.content !== 'string') return 'summary_shape';
      previousId = message.id;
    }
    // Read through the loaded range to verify native rows using full records,
    // including leading user rows that initially have no dbId. Summary content
    // is truncated and must never be used to identify an unsaved native row.
    const anchorIndex = props ? messages.value.findIndex((message) => Number.isSafeInteger(message?.dbId) && message.dbId > 0) : -1;
    const anchor = anchorIndex >= 0 ? messages.value[anchorIndex].dbId : null;
    for (let count = 1; anchor != null && page.has_more && (page.messages[0]?.id > anchor || (anchorIndex > 0 && page.messages[0]?.id === anchor)); count++) {
      if (count >= 200) return 'limited';
      const before = page.messages[0].id;
      const previous = await get('/data/tasks/' + enc(taskId) + '/messages?limit=100&before=' + before, 8 * 1024 * 1024);
      if (!validPage(previous) || !previous.messages.length || previous.messages.at(-1).id >= before || page.messages.length + previous.messages.length > 20000) return 'page_shape';
      page.messages = [...previous.messages, ...page.messages];
      page.has_more = previous.has_more;
    }
    // An opening/closing panel or a new native execution during the read wins.
    const unchanged = () => {
      if (instances.value !== instanceArray || scenarios.value !== scenarioArray) return false;
      const nowBound = instanceArray.filter((instance) => instance.taskId === taskId);
      return nowBound.length === bound.length && !nowBound.some((instance, index) => instance !== bound[index] || instance.sessionId !== sessionId) && idle() && JSON.stringify(scenarios.value) === scenarioSnapshot && (!props || (messages.value === messageArray && hasPanelProps(props) && props.taskId === taskId && props.conversationSessionId === sessionId && JSON.stringify(messages.value) === snapshot && cacheFields.every((key, index) => bound[0][key] === cacheSnapshot[index])));
    };
    if (!unchanged()) return 'changed';
    if (!refsWritable()) return props ? 'panel_fields' : 'app_fields';
    if (!cacheWritable()) return 'app_fields';
    const nextScenarios = list.scenarios.map((scenario) => ({...scenario, tasks: [...scenario.tasks].sort((left, right) => Number(left.sort_order ?? 0) - Number(right.sort_order ?? 0) || String(right.updated_at || '').localeCompare(String(left.updated_at || '')) || String(right.created_at || '').localeCompare(String(left.created_at || '')) || right.task_id.localeCompare(left.task_id))}));
    if (!props) {
      if (bound.length) return 'changed';
      if (!unchanged()) return 'changed';
      if (!refsWritable()) return 'app_fields';
      scenarios.value = nextScenarios;
      return 'closed';
    }
    if (bound.length !== 1) return 'changed';
    const latest = page.messages;
    const firstId = latest[0]?.id ?? Number.MAX_SAFE_INTEGER;
    const lastId = latest.at(-1)?.id ?? 0;
    const old = messages.value;
    const stored = new Map(latest.map((message) => [message.id, message]));
    const identities = new Map();
    let lower = 0;
    for (let index = 0; index < old.length; index++) {
      const message = old[index];
      if (!message || !['user', 'system'].includes(message.type) || typeof message.content !== 'string') return 'unpersisted';
      if (Number.isSafeInteger(message.dbId) && message.dbId > 0) {
        if (message.dbId <= lower || message.dbId > lastId || (message.dbId >= firstId && !stored.has(message.dbId))) return 'unpersisted';
        const row = stored.get(message.dbId);
        // Native final writes may still be queued after streaming becomes false.
        // Only reply rows explicitly written by Hub may replace a cached value.
        if (row && !(row.role === 'system' && (ownedRows.has(row.id) || message._tiancaispaceHub === true)) && (row.role !== message.type || row.content !== message.content || JSON.stringify(row.files || []) !== JSON.stringify(message.files || []) || JSON.stringify(row.thinking_steps || []) !== JSON.stringify(message.thinkingSteps || []))) return 'unpersisted';
        identities.set(message.dbId, message);
        lower = message.dbId;
        continue;
      }
      // Native user messages initially lack dbId. Match only one persisted row
      // between the neighbouring known IDs, never duplicate a local draft row.
      const upper = old.slice(index + 1).find((item) => Number.isSafeInteger(item.dbId) && item.dbId > 0)?.dbId ?? Number.MAX_SAFE_INTEGER;
      const candidates = latest.filter((row) => row.id > lower && row.id < upper && row.role === message.type && row.content === message.content && JSON.stringify(row.files || []) === JSON.stringify(message.files || []));
      if (candidates.length !== 1) return 'ambiguous';
      lower = candidates[0].id;
      identities.set(lower, message);
    }
    const getTime = (value) => {
      const normalized = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d+)?$/.test(value) ? value.replace(' ', 'T') + 'Z' : value;
      const date = new Date(normalized);
      if (Number.isNaN(date.getTime())) return value;
      const now = new Date();
      const time = String(date.getHours()).padStart(2, '0') + ':' + String(date.getMinutes()).padStart(2, '0');
      if (date.toDateString() === now.toDateString()) return '今天 ' + time;
      const day = String(date.getMonth() + 1).padStart(2, '0') + '-' + String(date.getDate()).padStart(2, '0');
      return date.getFullYear() === now.getFullYear() ? day + ' ' + time : date.getFullYear() + '-' + day + ' ' + time;
    };
    const retained = [];
    const knownIds = new Map([...identities].map(([id, message]) => [message, id]));
    for (let index = 0; index < old.length; index++) {
      const message = old[index];
      if (Number.isSafeInteger(message.dbId) && message.dbId < firstId) retained.push(message);
      else if (!Number.isSafeInteger(message.dbId)) {
        const upper = old.slice(index + 1).find((item) => Number.isSafeInteger(item.dbId) && item.dbId > 0)?.dbId;
        if (upper != null && upper <= firstId) retained.push({...message, dbId: knownIds.get(message)});
      }
    }
    const refreshed = latest.map((row) => ({
      ...identities.get(row.id),
      id: identities.get(row.id)?.id ?? row.id, dbId: row.id, type: row.role,
      content: row.content, timestamp: getTime(row.created_at),
      files: row.files.length ? row.files : undefined,
      thinking: !!row.thinking_steps?.length,
      thinkingSteps: row.thinking_steps?.length ? row.thinking_steps : undefined,
      _tiancaispaceHub: row.role === 'system' && (ownedRows.has(row.id) || identities.get(row.id)?._tiancaispaceHub === true),
    }));
    const hosts = props.isActive ? [...document.querySelectorAll('.chat-panel-host')] : [];
    const host = hosts.find((host) => host.querySelector('.chat-input-textarea') && host.style.display !== 'none');
    const scroller = host?.querySelector('.chat-messages');
    const follow = scroller && scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 80;
    const previousTimes = new Map(bound[0].seedMessages?.map((message) => [message.dbId, message.createdAt]) || []);
    const nextMessages = [...retained, ...refreshed];
    const loadedIds = new Set(nextMessages.map((message) => message.dbId));
    const completeLoaded = !page.has_more || summary.messages.every((message) => loadedIds.has(message.id));
    const nextSeedMessages = nextMessages.map((message) => ({
      type: message.type, content: message.content, dbId: message.dbId,
      createdAt: stored.get(message.dbId)?.created_at ?? previousTimes.get(message.dbId), thinkingSteps: message.thinkingSteps, files: message.files,
    }));
    // Compute every replacement and recheck writable targets after asynchronous
    // reads, before the first assignment to the view or its cached instance.
    if (!unchanged()) return 'changed';
    if (!refsWritable()) return 'panel_fields';
    if (!cacheWritable()) return 'app_fields';
    // Preserve all loaded older rows and the original form/model/attachments.
    messages.value = nextMessages;
    welcome.value = nextMessages.length === 0;
    older.value = !completeLoaded;
    const instance = bound[0];
    instance.seedMessages = nextSeedMessages;
    instance.messageSummaries = summary.messages;
    instance.hasOlderMessages = !completeLoaded;
    scenarios.value = nextScenarios;
    // Auto-scroll only the active panel already at its bottom; no page change.
    if (follow) queueMicrotask(() => scroll());
    return 'updated';
  } catch (error) {
    return error?.message === 'message page size' || error?.message === 'data size' ? 'limited' : 'fetch_failed';
  } finally {
    clearTimeout(timer);
  }
}
