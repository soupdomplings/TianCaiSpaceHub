const labels = {
  reasoning: '思考过程',
  tool: '工具执行',
  fileChange: '文件变更'
};
const busyStates = new Set([
  'running',
  'inProgress',
  'in_progress',
  'queued',
  'pending',
  'starting',
  'finalizing',
  'stopping',
  'waiting_for_approval',
  'waiting_for_user_input',
  'awaiting_confirmation',
  'waiting_approval',
  'waiting_confirmation',
  'unknown'
]);
function publicItem(item) {
  if (!item?.id) return null;
  if (
    item.type === 'message' &&
    ['assistant', 'user', 'system'].includes(item.role)
  )
    return { ...item, text: item.text || '' };
  if (item.files?.length)
    return {
      id: item.id,
      role: 'assistant',
      type: 'message',
      text: '',
      files: item.files
    };
  return null;
}
function merge(rows, items) {
  const ordered = new Map(rows.map((i) => [i.id, i]));
  for (const item of items) if (item) ordered.set(item.id, item);
  return [...ordered.values()];
}
// The first encounter decides inline visibility. The download registry is independent.
function projectItem(raw, registry, showNewArtifacts) {
  if (raw?.id) (registry.knownItems ||= new Set()).add(raw.id);
  const item = publicItem(raw);
  if (!item?.files?.length) return item;
  const files = item.files.filter((file) => {
    if (!file.id || item.role === 'user' || file.kind === 'attachment') return true;
    if (!registry.visible.has(file.id)) registry.visible.set(file.id, showNewArtifacts);
    registry.files.set(file.id, file);
    return registry.visible.get(file.id);
  });
  return files.length || item.text ? { ...item, files } : null;
}
function snapshot(page, registry = { files: new Map(), visible: new Map() }, showNewArtifacts = true) {
  return {
    registry,
    items: (page.items || []).map(item => projectItem(item, registry, showNewArtifacts)).filter(Boolean),
    status: page.status,
    turnId: page.turnId || '',
    approvals: page.approvals || [],
    cursor: page.cursor || 0,
    nextCursor: page.nextCursor || null
  };
}
function reduce(state, event, flags) {
  if (!Number.isSafeInteger(event.id) || event.id <= state.cursor) return state;
  const next = { ...state, cursor: event.id },
    d = event.data || {};
  const detail = d.liveActivity;
  if (
    detail &&
    labels[detail.category] &&
    flags[detail.category] &&
    typeof detail.text === 'string' &&
    detail.text
  ) {
    const id =
      'live.' +
      detail.category +
      '.' +
      (detail.itemId || event.itemId || d.item?.id || event.id);
    const old = state.items.find((i) => i.id === id);
    const text =
      detail.mode === 'append'
        ? (old?.text || '') + detail.text
        : [old?.text, detail.text].filter(Boolean).join('\n');
    next.items = merge(state.items, [
      {
        id,
        role: 'activity',
        type: detail.category,
        title: labels[detail.category],
        text
      }
    ]);
  }
  if (event.type === 'item.updated') {
    const completed = d.completed === true || ['completed', 'failed', 'error', 'interrupted', 'cancelled'].includes(d.item?.status);
    const fresh = !completed || !state.registry.atEnable?.has(d.item?.id);
    const item = projectItem(d.item, state.registry, !!flags.artifacts && fresh);
    if (item) next.items = merge(next.items, [item]);
  } else if (
    event.type === 'message.delta' &&
    !detail &&
    (!d.type || d.type === 'message') &&
    event.itemId &&
    typeof d.text === 'string'
  ) {
    const old = next.items.find((i) => i.id === event.itemId);
    next.items = merge(next.items, [
      {
        ...old,
        id: event.itemId,
        role: 'assistant',
        type: 'message',
        text: (old?.text || '') + d.text
      }
    ]);
  } else if (event.type === 'turn.updated') {
    next.status = d.status;
    next.turnId = d.turnId || event.turnId || state.turnId;
  } else if (event.type === 'approval.requested' && d.approval?.id) {
    next.approvals = merge(state.approvals, [d.approval]);
  } else if (event.type === 'approval.resolved') {
    next.approvals = state.approvals.filter((a) => a.id !== d.id);
  }
  return next;
}
function validate(q, value) {
  if (
    value === undefined ||
    value === null ||
    (typeof value === 'string' && !value.trim()) ||
    (Array.isArray(value) && !value.length)
  )
    return q.required === false;
  if (q.inputType === 'integer')
    return typeof value === 'string' && Number.isSafeInteger(Number(value));
  if (q.inputType === 'number')
    return typeof value === 'string' && Number.isFinite(Number(value));
  const values = Array.isArray(value) ? value : [value];
  return (
    !q.options?.length ||
    q.allowOther !== false ||
    values.every((v) => q.options.some((o) => o.value === v))
  );
}
function form(a, answers, canRespond) {
  const questions = (a.questions || []).map((q) => ({
    ...q,
    custom: !q.options?.length || q.allowOther !== false,
    typed:
      typeof answers[q.id] === 'string' &&
      !q.options?.some((o) => o.value === answers[q.id])
        ? answers[q.id]
        : '',
    options: (q.options || []).map((o) => ({
      ...o,
      selected: Array.isArray(answers[q.id])
        ? answers[q.id].includes(o.value)
        : o.value === answers[q.id]
    }))
  }));
  const actions =
    a.type !== 'question' && a.choices?.length
      ? a.choices
      : a.type === 'question'
        ? [
            { value: 'cancel', label: '取消问题' },
            ...(a.canDecline ? [{ value: 'decline', label: '拒绝' }] : []),
            { value: 'accept', label: '提交回答' }
          ]
        : [
            { value: 'decline', label: '拒绝' },
            { value: 'accept', label: '允许' }
          ];
  return {
    ...a,
    questions,
    actions: actions.map((x) => ({
      ...x,
      disabled:
        x.value === 'accept' &&
        questions.some((q) => !validate(q, answers[q.id]))
    })),
    desktopOnly: !!a.desktopOnly || !canRespond
  };
}
module.exports = {
  labels,
  busyStates,
  merge,
  snapshot,
  reduce,
  form,
  validate
};
