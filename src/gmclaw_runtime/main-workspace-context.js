(function installTiangongMainContext() {
  globalThis.__tiancaiHubWorkspaceMain = Object.freeze({
    taskScope(db, task) {
      const session = task.session_id
        ? db.prepare('SELECT user_id,agent_id,project_id FROM sessions WHERE session_id = ?').get(task.session_id)
        : undefined;
      return {
        user_id: session?.user_id || task.owner_id || 'ai-gateway',
        agent_id: session?.agent_id || 'desktop-orchestration',
        project_id: session?.project_id || 'default',
      };
    },
    projectId(db, task) {
      const id = this.taskScope(db, task).project_id;
      return id === 'default' ? '0' : id;
    },
  });
  return true;
})()
