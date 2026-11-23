const env = require('../../lib/environment');
const { cloud } = require('../../lib/api');
const {
  invoke,
  get,
  set,
  del,
  scale,
  platform,
  failure
} = require('../../lib/platform');
const { uuid } = require('../../lib/crypto');
const orderKey = 'squadOrder';
const stateLabels = {
  pending: '待付款或到账核对中',
  paid: '已到账',
  closed: '订单已关闭',
  refunded: '已退款'
};
Page({
  data: {
    preview: env.preview,
    font: 17,
    amounts: [],
    quote: null,
    order: null,
    working: false,
    error: ''
  },
  onLoad() {
    this.saved = get(orderKey) || null;
    this.setData({ font: scale() });
  },
  onShow() {
    this.active = true;
    this.load();
  },
  onHide() {
    this.active = false;
  },
  onUnload() {
    this.onHide();
  },
  async load() {
    const rev = (this.rev = (this.rev || 0) + 1);
    this.setData({ error: '' });
    try {
      const account = await cloud('/squad');
      if (rev !== this.rev || !this.active) return;
      if (!account.bound) throw Error('请先绑定当前小分队账户');
      this.setData({
        amounts: account.payment?.amounts || [],
        canPay:
          account.payment?.enabled &&
          account.payment?.platforms?.includes(platform()) &&
          env.paymentAvailable()
      });
      if (this.saved?.id) await this.status();
      else if (this.saved) this.setData({ uncertain: true });
    } catch (e) {
      if (this.active) this.setData({ error: failure(e) });
    }
  },
  async select(e) {
    if (this.data.working || this.saved) return;
    const amount = this.data.amounts[e.currentTarget.dataset.index],
      selection = (this.selection = (this.selection || 0) + 1);
    this.setData({ amount, quote: null, error: '' });
    try {
      const quote = await cloud('/squad/quote', 'POST', {
        amount,
        platform: platform()
      });
      if (selection === this.selection && this.active) this.setData({ quote });
    } catch (e) {
      if (selection === this.selection) this.setData({ error: failure(e) });
    }
  },
  async pay() {
    if (
      this.data.working ||
      !this.data.canPay ||
      (!this.saved && !this.data.quote)
    )
      return;
    this.setData({ working: true, error: '' });
    try {
      if (!this.saved) {
        this.saved = {
          requestId: await uuid(),
          quoteId: this.data.quote.quoteId
        };
        set(orderKey, this.saved);
      }
      const { code } = await invoke('login');
      const order = await cloud('/squad/orders', 'POST', {
        requestId: this.saved.requestId,
        quoteId: this.saved.quoteId,
        code
      });
      if (!order.id || (this.saved.id && order.id !== this.saved.id))
        throw Error('订单身份不匹配，请核对原订单');
      this.saved = { ...this.saved, id: order.id };
      set(orderKey, this.saved);
      this.showOrder(order);
      if (order.state === 'pending' && order.payment) {
        const { mode, signData, paySig, signature } = order.payment;
        await invoke('requestVirtualPayment', {
          mode,
          signData,
          paySig,
          signature
        });
      }
    } catch (e) {
      if (!failure(e).includes('cancel')) this.setData({ error: failure(e) });
    } finally {
      if (this.saved?.id) {
        try {
          await this.status();
        } catch (e) {
          this.setData({ error: failure(e) });
        }
      } else this.setData({ uncertain: !!this.saved });
      this.setData({ working: false });
    }
  },
  showOrder(order) {
    this.setData({
      order: {
        id: order.id,
        state: order.state,
        label: stateLabels[order.state] || '待核对',
        credit: order.credit ?? null,
        amount: order.amount ?? null
      },
      uncertain: false
    });
  },
  async status() {
    if (!this.saved?.id) return;
    try {
      const order = await cloud(
        '/squad/orders/' + encodeURIComponent(this.saved.id)
      );
      this.showOrder(order);
    } catch (e) {
      this.setData({ error: failure(e) });
    }
  },
  done() {
    if (
      this.saved?.id &&
      !['paid', 'closed', 'refunded'].includes(this.data.order?.state)
    )
      return;
    del(orderKey);
    this.saved = null;
    this.setData({ quote: null, order: null, amount: null, uncertain: false });
  }
});
