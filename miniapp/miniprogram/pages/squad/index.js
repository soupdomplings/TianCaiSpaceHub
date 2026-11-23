const env = require('../../lib/environment');
const { cloud } = require('../../lib/api');
const { scale, platform, openPage, failure } = require('../../lib/platform');
Page({
  data: {
    preview: env.preview,
    font: 17,
    error: '',
    binding: false,
    code: '',
    account: null
  },
  onShow() {
    this.visible = true;
    this.getTabBar()?.setData({ selected: 1 });
    this.setData({ font: scale() });
    this.load();
  },
  onHide() {
    this.visible = false;
  },
  onUnload() {
    this.onHide();
  },
  async load() {
    const seq = (this.seq = (this.seq || 0) + 1);
    this.setData({ loading: true, error: '' });
    try {
      const a = await cloud('/squad');
      if (this.visible && this.seq === seq)
        this.setData({
          account: a,
          canPay:
            !!a.payment?.enabled &&
            a.payment.platforms?.includes(platform()) &&
            env.paymentAvailable()
        });
    } catch (e) {
      if (this.visible && seq === this.seq) this.setData({ error: failure(e) });
    } finally {
      if (this.visible && seq === this.seq) this.setData({ loading: false });
    }
  },
  input(e) {
    this.setData({ code: e.detail.value });
  },
  async bind() {
    if (this.data.binding || !this.data.code.trim()) return;
    this.setData({ binding: true, error: '' });
    try {
      await cloud('/squad/bind', 'POST', { code: this.data.code.trim() });
      this.setData({ code: '' });
      await this.load();
    } catch (e) {
      this.setData({ error: failure(e) });
    } finally {
      this.setData({ binding: false });
    }
  },
  expand(e) {
    const id = e.currentTarget.dataset.id;
    this.setData({ expanded: this.data.expanded === id ? null : id });
  },
  recharge() {
    openPage('recharge', {});
  },
  onPullDownRefresh() {
    return this.load().finally(() => wx.stopPullDownRefresh());
  }
});
