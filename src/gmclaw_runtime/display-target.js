function (taskId, sessionId) {
  const root = document.getElementById('app')?._vnode?.component;
  if (!root || typeof root.render !== 'function') return null;
  const isId = (value) => typeof value === 'string' && value.length > 0 && value.length <= 128 && /^[A-Za-z0-9_-]+$/.test(value);
  const hasPanelProps = (props) => props && isId(props.taskId) && isId(props.conversationSessionId) && typeof props.isActive === 'boolean';
  const matches = [];
  const queue = [root.subTree];
  const seen = new Set();
  for (let count = 0; queue.length && count < 12000; count++) {
    const vnode = queue.pop();
    if (!vnode || typeof vnode !== 'object' || seen.has(vnode)) continue;
    seen.add(vnode);
    const component = vnode.component;
    const props = component?.props;
    const matchesTask = taskId == null ? props?.isActive === true && props?.taskId != null && props.taskId !== '' : props?.taskId === taskId;
    const hasPanelContract = props !== null && typeof props === 'object' && !Array.isArray(props) && ['taskId', 'conversationSessionId', 'isActive'].every((key) => key in props);
    // The legacy name only diagnoses an incomplete known panel; full semantic
    // props admit any component name without treating task-list rows as panels.
    if (typeof component?.render === 'function' && matchesTask && (hasPanelContract || component.type?.__name === 'ChatPanel')) {
      if (!hasPanelProps(props)) throw new Error('panel fields');
      if (taskId != null && props.conversationSessionId !== sessionId) throw new Error('session identity');
      matches.push(component);
    }
    if (component?.subTree) queue.push(component.subTree);
    if (Array.isArray(vnode.children)) queue.push(...vnode.children);
  }
  if (queue.length || matches.length > 1) throw new Error('view identity');
  const panel = matches[0];
  return {appRender: root.render, panelRender: panel?.render ?? null, panelProps: panel?.props ?? null};
}
