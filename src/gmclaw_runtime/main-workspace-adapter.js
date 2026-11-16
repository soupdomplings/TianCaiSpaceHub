// Pure LiveEdit transform. Keep existing closure bindings so active native
// callbacks are updated without restarting the DataServer or editing the app.
(function adaptTiangongMain(source) {
  const marker = 'TIANCAISPACEHUB_MAIN_WORKSPACE_V1';
  if (typeof source !== 'string') return {adapted: false, reason: 'invalid_source'};
  if (source.includes(marker)) return {adapted: false, reason: 'already_adapted'};
  let changed = source.replace(/\r\n/g, '\n');
  const replace = (before, after) => {
    if (changed.split(before).length !== 2) throw new Error('unsupported_main');
    changed = changed.replace(before, after);
  };
  try {
    replace('conversationAccount: () => db ? readClawSession(db)?.userId || "" : "",',
      'conversationAccount: () => db ? (readClawSession(db), "ai-gateway") : "", // ' + marker);
    // Never claim unrelated legacy conversations just because a user opens
    // the external-model workspace. Explicit history migration handles those.
    replace('      conversationAccounts.claimLegacy(owner);',
      '      if (owner !== "ai-gateway") conversationAccounts.claimLegacy(owner);');
    replace('add("GET", "/data/claw/session", ({ db: db2 }) => {\n' +
      '  const session = readClawSession(db2);\n' +
      '  if (!session) return { loggedIn: false };\n' +
      '  return {\n    loggedIn: true,',
      'add("GET", "/data/claw/session", ({ db: db2 }) => {\n' +
      '  const session = readClawSession(db2);\n' +
      '  if (!session) return { loggedIn: false, hubConversationOwner: "ai-gateway" };\n' +
      '  return {\n    hubConversationOwner: "ai-gateway",\n    loggedIn: true,');
    replace('user_id: "local", agent_id: "desktop-orchestration", project_id: "default", project,',
      '...globalThis.__tiancaiHubWorkspaceMain.taskScope(db2, task), project: { ...project, id: globalThis.__tiancaiHubWorkspaceMain.projectId(db2, task) },');
    replace('      session_id: task.session_id,\n      user_id: "local",',
      '      session_id: task.session_id,\n      ...globalThis.__tiancaiHubWorkspaceMain.taskScope(db2, task),');
    replace('        user_id: "local",\n        agent_id: "desktop-orchestration",',
      '        ...globalThis.__tiancaiHubWorkspaceMain.taskScope(db2, task),');
    replace('project: { id: task.scenario_id === "default" ? "0" : task.scenario_id, path: workspace,',
      'project: { id: globalThis.__tiancaiHubWorkspaceMain.projectId(db2, task), path: workspace,');
    replace('project: { path: taskWorkspacePath(task, task.work_dir, defaultWorkspace()) },',
      'project: { id: globalThis.__tiancaiHubWorkspaceMain.projectId(db2, task), path: taskWorkspacePath(task, task.work_dir, defaultWorkspace()) },');
    // Scheduled runs already carry their owner's task/project assignment.
    replace('      user_id: "local",\n      agent_id: "desktop-orchestration",',
      '      user_id: user,\n      agent_id: "desktop-orchestration",');
    return {source: changed, adapted: true};
  } catch (_) {
    return {adapted: false, reason: 'unsupported_main'};
  }
})
