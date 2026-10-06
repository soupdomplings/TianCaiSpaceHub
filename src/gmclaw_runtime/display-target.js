function (taskId, sessionId) {
  const root = document.getElementById('app')?._vnode?.component;
  if (!root || root.type?.__name !== 'App' || typeof root.render !== 'function') return null;
  const matches = [];
  const queue = [root.subTree];
  const seen = new Set();
  for (let count = 0; queue.length && count < 12000; count++) {
    const vnode = queue.pop();
    if (!vnode || typeof vnode !== 'object' || seen.has(vnode)) continue;
    seen.add(vnode);
    const component = vnode.component;
    if (component?.type?.__name === 'ChatPanel' && (taskId == null ? component.props?.isActive === true && component.props?.taskId != null : component.props?.taskId === taskId)) {
      if (taskId != null && component.props.conversationSessionId !== sessionId) throw new Error('session identity');
      matches.push(component);
    }
    if (component?.subTree) queue.push(component.subTree);
    if (Array.isArray(vnode.children)) queue.push(...vnode.children);
  }
  if (queue.length || matches.length > 1) throw new Error('view identity');
  const panel = matches[0];
  return {appRender: root.render, panelRender: panel?.render ?? null, panelProps: panel?.props ?? null};
}
