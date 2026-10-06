function (messages, streaming, confirmation, welcome, older, scroll, loadingOlder, modelsLoaded, localTaskIdRef, sessionIdRef, nativeCleanup, instances, scenarios, openingTasks, deletingTaskIds, props) {
  const isRef = (value) => value && value.__v_isRef === true;
  const isSet = (value) => (isRef(value) ? value.value : value) instanceof Set;
  if (!isRef(instances) || !Array.isArray(instances.value) || !isRef(scenarios) || !Array.isArray(scenarios.value) || !isSet(openingTasks) || !isSet(deletingTaskIds)) return 'app_fields';
  if (props && (![messages, streaming, confirmation, welcome, older, loadingOlder, modelsLoaded, localTaskIdRef, sessionIdRef].every(isRef) || !Array.isArray(messages.value) || typeof scroll !== 'function' || (nativeCleanup !== null && typeof nativeCleanup !== 'function'))) return 'panel_fields';
  return 'ready';
}
