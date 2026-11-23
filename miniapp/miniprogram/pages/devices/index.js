const env = require('../../lib/environment');
const { cloud, pair } = require('../../lib/api');
const {
  invoke,
  get,
  last,
  scale,
  openPage,
  failure
} = require('../../lib/platform');
Page({
  data: {
    preview: env.preview,
    devices: [],
    font: 17,
    loading: false,
    error: ''
  },
  onShow() {
    this.visible = true;
    this.getTabBar()?.setData({ selected: 0 });
    this.setData({ font: scale() });
    this.load();
  },
  onHide() {
    this.visible = false;
    clearTimeout(this.timer);
  },
  onUnload() {
    this.onHide();
  },
  async load() {
    if (this.loading) return;
    this.loading = true;
    clearTimeout(this.timer);
    this.setData({ loading: true, error: '' });
    try {
      const { devices } = await cloud('/devices');
      const rows = devices.map((d) => ({
        ...d,
        keyPresent: !!get('key.' + d.bindingId, true),
        last: last(d.bindingId)
      }));
      if (!this.visible) return;
      this.setData({ devices: rows });
      const recent = get('recentBinding'),
        current = rows.find(
          (d) =>
            d.bindingId === recent &&
            d.online &&
            d.state === 'active' &&
            d.keyPresent &&
            d.last
        );
      const app = getApp();
      if (current && !app.restored) {
        app.restored = true;
        await openPage('chat', { ...current.last, binding: current.bindingId });
      }
      if (this.visible && rows.some((d) => d.state === 'pending'))
        this.timer = setTimeout(() => this.load(), 2500);
    } catch (e) {
      if (this.visible) this.setData({ error: failure(e) });
    } finally {
      this.loading = false;
      if (this.visible) this.setData({ loading: false });
    }
  },
  async scan() {
    if (this.pairing) return;
    this.pairing = true;
    try {
      const { result } = await invoke('scanCode', {
        onlyFromCamera: true,
        scanType: ['qrCode']
      });
      await pair(result);
      await this.load();
    } catch (e) {
      if (!failure(e).includes('cancel')) this.setData({ error: failure(e) });
    } finally {
      this.pairing = false;
    }
  },
  open(e) {
    const row = this.data.devices[e.currentTarget.dataset.index];
    if (!row || !row.online || !row.keyPresent || row.state !== 'active')
      return;
    openPage('sessions', { binding: row.bindingId, name: row.name });
  },
  onPullDownRefresh() {
    return this.load().finally(() => wx.stopPullDownRefresh());
  }
});
