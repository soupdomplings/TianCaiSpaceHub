const { rpc } = require('../../lib/api');
const { decoded, scale, openPage, failure } = require('../../lib/platform');
Page({
  data: {
    font: 17,
    clients: [],
    sessions: [],
    projects: [],
    error: '',
    loading: false,
    createOpen: false
  },
  onLoad(query) {
    this.options = decoded(query);
    this.setData({ font: scale(), name: this.options.name });
  },
  onShow() {
    this.active = true;
    this.refresh();
  },
  onHide() {
    this.active = false;
    this.seq = (this.seq || 0) + 1;
  },
  onUnload() {
    this.onHide();
  },
  api(method, path, body) {
    return rpc(this.options.binding, method, path, body);
  },
  async refresh() {
    const seq = (this.seq = (this.seq || 0) + 1);
    this.setData({ error: '', loading: true });
    try {
      const { clients } = await this.api('GET', '/clients?view=summary');
      if (seq !== this.seq || !this.active) return;
      const client =
        clients.find((c) => c.id === this.data.client?.id) ||
        clients.find((c) => c.ready) ||
        clients[0];
      this.setData({
        clients,
        client: client || null,
        sessions: [],
        nextCursor: null,
        createOpen: false,
        loading: false
      });
      if (client?.ready) await this.list(false);
    } catch (e) {
      if (this.seq === seq) this.setData({ error: failure(e), loading: false });
    }
  },
  change(e) {
    const client = this.data.clients[e.currentTarget.dataset.index];
    if (!client || client.id === this.data.client?.id) return;
    this.seq++;
    this.setData({
      client,
      sessions: [],
      nextCursor: null,
      createOpen: false,
      error: '',
      loading: false
    });
    if (client.ready) this.list(false);
  },
  async list(more) {
    if (this.data.loading || !this.data.client) return;
    const seq = this.seq,
      client = this.data.client,
      append = more === true;
    this.setData({ loading: true, error: '' });
    try {
      const q = append
        ? '&cursor=' + encodeURIComponent(this.data.nextCursor)
        : '';
      const res = await this.api(
        'GET',
        '/clients/' + encodeURIComponent(client.id) + '/sessions?limit=16' + q
      );
      if (seq !== this.seq) return;
      this.setData({
        sessions: append
          ? [
              ...new Map(
                this.data.sessions.concat(res.sessions).map((s) => [s.id, s])
              ).values()
            ]
          : res.sessions,
        nextCursor: res.nextCursor || null
      });
    } catch (e) {
      if (seq === this.seq) this.setData({ error: failure(e) });
    } finally {
      if (seq === this.seq) this.setData({ loading: false });
    }
  },
  more() {
    return this.list(true);
  },
  open(e) {
    return this.chat(this.data.sessions[e.currentTarget.dataset.index]);
  },
  chat(s) {
    const client = this.data.client;
    if (s)
      openPage('chat', {
        binding: this.options.binding,
        client: client.id,
        session: s.id,
        title: s.title || '会话'
      });
  },
  async projects(append) {
    const seq = this.seq;
    if (this.loadingProject) return;
    this.loadingProject = true;
    this.setData({ error: '' });
    try {
      const q =
        append === true
          ? '&cursor=' + encodeURIComponent(this.data.projectCursor)
          : '';
      const r = await this.api(
        'GET',
        '/clients/' +
          encodeURIComponent(this.data.client.id) +
          '/projects?limit=12' +
          q
      );
      if (seq !== this.seq) return;
      this.setData({
        createOpen: true,
        projects:
          append === true ? this.data.projects.concat(r.projects) : r.projects,
        projectCursor: r.nextCursor || null
      });
    } catch (e) {
      if (seq === this.seq) this.setData({ error: failure(e) });
    } finally {
      this.loadingProject = false;
    }
  },
  moreProjects() {
    return this.projects(true);
  },
  cancel() {
    this.setData({ createOpen: false });
  },
  async create(e) {
    const project = this.data.projects[e.currentTarget.dataset.index],
      client = this.data.client;
    if (this.creating || !project) return;
    this.creating = true;
    this.setData({ error: '' });
    try {
      const { session } = await this.api(
        'POST',
        '/clients/' + encodeURIComponent(client.id) + '/sessions',
        { projectId: project.id, cwd: project.path }
      );
      this.setData({ createOpen: false });
      this.chat(session);
    } catch (err) {
      this.setData({ error: failure(err) });
    } finally {
      this.creating = false;
    }
  }
});
