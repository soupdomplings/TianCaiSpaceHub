const env = require('./environment');
function invoke(method, args = {}) {
  if (env.preview && method === 'login')
    return Promise.resolve({ code: env.loginCode() });
  if (
    env.preview &&
    ['requestVirtualPayment', 'request', 'scanCode'].includes(method)
  )
    return Promise.reject(Error('当前为本地演示'));
  return new Promise((resolve, reject) => {
    if (typeof wx[method] !== 'function')
      return reject(new Error('当前微信版本不支持此功能，请升级微信'));
    wx[method]({
      ...args,
      success: resolve,
      fail: (e) => reject(new Error(e.errMsg || '微信请求失败'))
    });
  });
}
function scale() {
  const { fontSizeSetting } = wx.getAppBaseInfo
    ? wx.getAppBaseInfo()
    : wx.getSystemInfoSync();
  return Math.max(16, Math.min(36, fontSizeSetting || 17));
}
function platform() {
  return (wx.getDeviceInfo ? wx.getDeviceInfo() : wx.getSystemInfoSync())
    .platform;
}
function get(key, encrypted = false) {
  return wx.getStorageSync(env.storageNamespace + key, encrypted);
}
function set(key, value, encrypted = false) {
  wx.setStorageSync(env.storageNamespace + key, value, encrypted);
}
function del(key) {
  wx.removeStorageSync(env.storageNamespace + key);
}
function last(binding) {
  return get('recent.' + binding);
}
function openPage(page, data) {
  const qs = Object.entries(data)
    .filter(([, v]) => v != null)
    .map(([k, v]) => encodeURIComponent(k) + '=' + encodeURIComponent(v))
    .join('&');
  return invoke('navigateTo', { url: '/pages/' + page + '/index?' + qs });
}
function decoded(options) {
  const result = {};
  for (const [k, v] of Object.entries(options || {}))
    result[k] = decodeURIComponent(v);
  return result;
}
function failure(e) {
  return (e && e.message) || String(e);
}
module.exports = {
  invoke,
  get,
  set,
  del,
  last,
  scale,
  platform,
  openPage,
  decoded,
  failure
};
