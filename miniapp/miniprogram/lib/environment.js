module.exports = {
  preview: false,
  storageNamespace: 'hub.v1.',
  paymentAvailable: () => wx.canIUse('requestVirtualPayment'),
  loginCode: () => null
};
