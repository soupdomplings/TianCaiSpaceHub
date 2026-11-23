const env = require('../../lib/environment');
const { rpc } = require('../../lib/api');
const { set, decoded, scale, failure } = require('../../lib/platform');
const { uuid } = require('../../lib/crypto');
const {
  snapshot,
  reduce,
  merge,
  form,
  busyStates
} = require('../../lib/conversation');
const download = require('../../lib/download');
const { defaults: defaultFlags, fromHub } = require('../../lib/display');
Page({
  data: {
    preview: env.preview,
    font: 17,
    items: [],
    approvals: [],
    sheet: '',
    caps: {},
    activeForm: null,
    displayManaged: false,
    text: '',
    connection: '连接中',
    error: '',
    keyHeight: 0
  },
  onLoad(q) {
    this.loc = decoded(q);
    const { client, session, binding, title } = this.loc;
    this.base =
      '/clients/' +
      encodeURIComponent(client) +
      '/sessions/' +
      encodeURIComponent(session);
    set('recentBinding', binding);
    set('recent.' + binding, { client, session, title });
    this.seq = 0;
    this.flags = { ...defaultFlags };
    this.answers = {};
    this.submitted = new Set();
    this.setData({ font: scale(), title: title || '会话' });
    wx.setNavigationBarTitle({
      title: (env.preview ? '本地演示 · ' : '') + (title || '会话')
    });
  },
  onShow() {
    this.start();
  },
  onHide() {
    this.seq++;
    clearTimeout(this.timer);
    this.setData({ keyHeight: 0 });
  },
  onUnload() {
    this.onHide();
  },
  api(method, path, body) {
    return rpc(this.loc.binding, method, path, body);
  },
  async start() {
    const seq = ++this.seq;
    clearTimeout(this.timer);
    this.setData({ connection: '连接中', error: '' });
    try {
      const [page, clients] = await Promise.all([
        this.api('GET', this.base + '/history?limit=3'),
        this.api('GET', '/clients?view=summary')
      ]);
      if (seq !== this.seq) return;
      const client = clients.clients.find((c) => c.id === this.loc.client);
      if (!client?.ready)
        throw Error(client?.reason || '请在目标电脑启动客户端');
      this.receiveDisplay(page.displayContent);
      this.state = snapshot(page, this.state?.registry);
      this.baseline = new Set(this.state.items.map((i) => i.id));
      this.setData({ caps: client.capabilities || {}, connection: '已连接' });
      this.draw();
      this.poll(seq);
    } catch (e) {
      if (seq === this.seq) this.recover(e, seq, () => this.start());
    }
  },
  recover(e, seq, next) {
    const denied = [401, 403, 410].includes(e.status);
    this.setData({
      connection: denied ? '访问已结束' : '连接已中断',
      error: failure(e)
    });
    if (!denied)
      this.timer = setTimeout(() => {
        if (seq === this.seq) next();
      }, 3000);
  },
  async poll(seq) {
    if (seq !== this.seq) return;
    try {
      const page = await this.api(
        'GET',
        this.base +
          '/events?after=' +
          this.state.cursor +
          (this.data.caps.liveActivity ? '&liveActivity=1' : '')
      );
      if (seq !== this.seq) return;
      let changed = false;
      const ambiguous = new Set();
      for (const event of page.events || []) {
        if (
          event.sessionId !== this.loc.session ||
          event.clientId !== this.loc.client
        )
          continue;
        if (event.id <= this.state.cursor) continue;
        this.receiveDisplay(event.data?.displayContent);
        if (event.type === 'reset') {
          this.start();
          return;
        }
        if (event.type === 'error')
          this.setData({ error: event.data?.message || '目标客户端错误' });
        if (
          event.type === 'message.delta' &&
          this.baseline.has(event.itemId) &&
          !event.data.liveActivity
        ) {
          // A history item can already include this delta. The native completed item replaces it.
          ambiguous.add(event.itemId);
          this.state.cursor = Math.max(this.state.cursor, event.id);
          continue;
        }
        const prev = this.state;
        const fileCount = prev.registry.files.size;
        this.state = reduce(this.state, event, this.flags);
        changed =
          changed ||
          prev.items !== this.state.items ||
          fileCount !== this.state.registry.files.size ||
          prev.approvals !== this.state.approvals ||
          prev.status !== this.state.status;
      }
      if (ambiguous.size) {
        const absolute = snapshot(
          await this.api('GET', this.base + '/history?limit=3'),
          this.state.registry,
          !!this.flags.artifacts
        );
        if (seq !== this.seq) return;
        const replacements = absolute.items.filter((item) =>
          ambiguous.has(item.id)
        );
        if (replacements.length) {
          this.state.items = merge(this.state.items, replacements);
          changed = true;
        }
      }
      if (this.data.connection !== '已连接')
        this.setData({ connection: '已连接', error: '' });
      if (changed) this.draw();
      this.timer = setTimeout(() => this.poll(seq), 50);
    } catch (e) {
      if (seq === this.seq) this.recover(e, seq, () => this.poll(seq));
    }
  },
  draw() {
    const st = this.state;
    const requests = st.approvals || [];
    const selected = requests.find((a) => a.id === this.formId);
    if (!selected && this.data.sheet === 'approval')
      this.setData({ sheet: '' });
    this.setData({
      items: st.items,
      approvals: requests,
      artifacts: [...st.registry.files.values()],
      nextCursor: st.nextCursor,
      busy: busyStates.has(st.status),
      waiting: /waiting|awaiting/.test(st.status || '') && !requests.length,
      activeForm: selected ? this.viewForm(selected) : null,
      anchor: this.data.readingOlder ? this.data.anchor : 'end'
    });
  },
  scroll(e) {
    if (e.detail.deltaY > 0 && !this.data.readingOlder)
      this.setData({ readingOlder: true, anchor: '' });
  },
  bottom() {
    this.setData({ readingOlder: false });
  },
  async older() {
    if (!this.state?.nextCursor || this.paging) return;
    const seq = this.seq,
      cursor = this.state.nextCursor;
    this.paging = true;
    const first = this.state.items[0]?.id;
    try {
      const p = snapshot(
        await this.api(
          'GET',
          this.base + '/history?limit=3&cursor=' + encodeURIComponent(cursor)
        ), this.state.registry
      );
      if (seq !== this.seq) return;
      this.state = {
        ...this.state,
        items: merge(p.items, this.state.items),
        nextCursor: p.nextCursor
      };
      this.setData({ readingOlder: true, anchor: '' });
      this.draw();
      if (first) {
        const index = this.state.items.findIndex((i) => i.id === first);
        this.setData({ anchor: 'r' + index });
      }
    } catch (e) {
      if (seq === this.seq) this.setData({ error: failure(e) });
    } finally {
      this.paging = false;
    }
  },
  receiveDisplay(value) {
    const next = fromHub(value);
    if (next.artifacts && !this.flags.artifacts && this.state)
      this.state.registry.atEnable = new Set(this.state.registry.knownItems);
    this.flags = next;
    const managed = !!value && typeof value === 'object';
    if (managed !== this.data.displayManaged) this.setData({ displayManaged: managed });
  },
  showArtifacts() {
    this.setData({ sheet: 'artifacts' });
  },
  dismiss() {
    this.setData({ sheet: '' });
  },
  noop() {},
  type(e) {
    this.setData({ text: e.detail.value });
  },
  keyboard(e) {
    this.setData({ keyHeight: Math.max(0, e.detail.height || 0) });
  },
  async send() {
    const text = this.data.text.trim();
    if (
      !text ||
      this.data.writing ||
      !this.data.caps.send ||
      this.data.busy ||
      this.data.connection !== '已连接'
    )
      return;
    this.setData({ writing: true, error: '', readingOlder: false });
    try {
      const r = await this.api('POST', this.base + '/turns', {
        text,
        attachmentIds: [],
        requestId: await uuid()
      });
      this.state.turnId = r.turnId;
      this.state.status = r.status;
      this.setData({ text: '' });
      this.draw();
    } catch (e) {
      this.setData({ error: failure(e) });
    } finally {
      this.setData({ writing: false });
    }
  },
  async stop() {
    if (this.data.writing || !this.data.caps.stop || !this.state) return;
    this.setData({ writing: true, error: '' });
    try {
      const r = await this.api('POST', this.base + '/stop', {
        turnId: this.state.turnId || null
      });
      this.state.status = r.status;
      this.draw();
    } catch (e) {
      this.setData({ error: failure(e) });
    } finally {
      this.setData({ writing: false });
    }
  },
  viewForm(a) {
    return {
      ...form(
        a,
        this.answers[a.id] || {},
        this.data.caps[a.type === 'question' ? 'questions' : 'approvals']
      ),
      submitted: this.submitted.has(a.id)
    };
  },
  openForm(e) {
    const a = this.state.approvals[e.currentTarget.dataset.index];
    if (!a) return;
    this.formId = a.id;
    this.setData({
      sheet: 'approval',
      activeForm: this.viewForm(a),
      formError: ''
    });
  },
  answer(q, v) {
    this.answers[this.formId] = {
      ...(this.answers[this.formId] || {}),
      [q.id]: v
    };
    const a = this.state.approvals.find((a) => a.id === this.formId);
    if (a) this.setData({ activeForm: this.viewForm(a) });
  },
  choose(e) {
    const { q, o } = e.currentTarget.dataset;
    const spec = this.data.activeForm.questions[q],
      value = spec.options[o].value;
    const old = this.answers[this.formId]?.[spec.id];
    this.answer(
      spec,
      spec.multiple
        ? Array.isArray(old)
          ? old.includes(value)
            ? old.filter((x) => x !== value)
            : old.concat(value)
          : [value]
        : value
    );
  },
  custom(e) {
    this.answer(
      this.data.activeForm.questions[e.currentTarget.dataset.q],
      e.detail.value
    );
  },
  async respond(e) {
    const a = this.state.approvals.find((x) => x.id === this.formId);
    if (!a || this.data.answering || this.submitted.has(a.id)) return;
    const f = this.viewForm(a),
      action = f.actions[e.currentTarget.dataset.index];
    if (f.desktopOnly || !action || action.disabled) return;
    this.setData({ answering: true, formError: '' });
    try {
      await this.api(
        'POST',
        this.base + '/approvals/' + encodeURIComponent(a.id),
        {
          decision: action.value,
          answers: action.value === 'accept' ? this.answers[a.id] : undefined
        }
      );
      this.submitted.add(a.id);
      this.setData({ sheet: '' });
    } catch (err) {
      this.setData({ formError: failure(err) });
    } finally {
      this.setData({ answering: false });
    }
  },
  async file(e) {
    if (this.downloading) return;
    this.downloading = true;
    const { row, idx, fileId } = e.currentTarget.dataset;
    try {
      await download(
        this.loc.binding,
        this.base,
        fileId ? this.state.registry.files.get(fileId) : this.state.items[row].files[idx]
      );
    } catch (err) {
      if (!failure(err).includes('cancel'))
        this.setData({ error: failure(err) });
    } finally {
      this.downloading = false;
    }
  }
});
