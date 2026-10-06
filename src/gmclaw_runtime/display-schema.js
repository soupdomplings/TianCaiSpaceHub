function (messages, streaming, confirmation, welcome, older, scroll, loadingOlder, modelsLoaded, localTaskIdRef, sessionIdRef, nativeCleanup, instances, scenarios, openingTasks, deletingTaskIds, props) {
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
  const isSet = (value) => (isRef(value) ? value.value : value) instanceof Set;
  const isBooleanRef = (value) => isRef(value) && typeof value.value === 'boolean';
  const isNullableStringRef = (value) => isRef(value) && (value.value === null || typeof value.value === 'string');
  const isConfirmationRef = (value) => isRef(value) && (value.value === null || (typeof value.value === 'object' && !Array.isArray(value.value)));
  const hasPanelProps = (value) => value && typeof value.taskId === 'string' && typeof value.conversationSessionId === 'string' && typeof value.isActive === 'boolean';
  if (!isRef(instances) || !Array.isArray(instances.value) || !isWritableRef(scenarios) || !Array.isArray(scenarios.value) || !isSet(openingTasks) || !isSet(deletingTaskIds)) return 'app_fields';
  if (props && (!hasPanelProps(props) || !isRef(messages) || !Array.isArray(messages.value) || ![messages, welcome, older].every(isWritableRef) || ![streaming, welcome, older, loadingOlder, modelsLoaded].every(isBooleanRef) || !isConfirmationRef(confirmation) || ![localTaskIdRef, sessionIdRef].every(isNullableStringRef) || typeof scroll !== 'function' || (nativeCleanup !== null && typeof nativeCleanup !== 'function'))) return 'panel_fields';
  if (props) {
    const bound = instances.value.filter((instance) => instance?.taskId === props.taskId);
    if (bound.length !== 1 || bound[0].sessionId !== props.conversationSessionId) return 'instance_identity';
    if (!isMutableObject(bound[0]) || !['seedMessages', 'messageSummaries', 'hasOlderMessages'].every((key) => canAssignCache(bound[0], key))) return 'app_fields';
  }
  return 'ready';
}
