function (view, ...values) {
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
  const isSet = (value) => (isRef(value) ? value.value : value) instanceof Set;
  if (view === 'app') {
    const [instances, scenarios, opening, deleting] = values;
    return values.length === 4 && isRef(instances) && Array.isArray(instances.value)
      && isWritableRef(scenarios) && Array.isArray(scenarios.value) && isSet(opening) && isSet(deleting);
  }
  if (view === 'panel') {
    const [messages, streaming, confirmation, welcome, older, scroll, loadingOlder, modelsLoaded, taskId, sessionId, cleanup] = values;
    return values.length === 11 && [messages, streaming, confirmation, welcome, older, loadingOlder, modelsLoaded, taskId, sessionId].every(isRef)
      && Array.isArray(messages.value)
      && [messages, welcome, older].every(isWritableRef)
      && [streaming, welcome, older, loadingOlder, modelsLoaded].every((value) => typeof value.value === 'boolean')
      && (confirmation.value === null || (typeof confirmation.value === 'object' && !Array.isArray(confirmation.value)))
      && (taskId.value === null || typeof taskId.value === 'string')
      && (sessionId.value === null || typeof sessionId.value === 'string')
      && typeof scroll === 'function' && (cleanup === null || typeof cleanup === 'function');
  }
  return false;
}
