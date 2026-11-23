const origin = require('../config');
const { invoke, get, set } = require('./platform');
const crypt = require('./crypto');
let token, loginFlight;
function check(response) {
  if (response.statusCode >= 200 && response.statusCode < 300)
    return response.data;
  const data = response.data || {};
  const err = new Error(
    data.error?.message ||
      data.message ||
      (typeof data.error === 'string' ? data.error : '') ||
      '请求失败（' + response.statusCode + '）'
  );
  err.status = response.statusCode;
  throw err;
}
async function call(path, method, data, session) {
  if (!origin || !origin.startsWith('https://'))
    throw Error('微信连接服务尚未配置');
  const result = await invoke('request', {
    url: origin + '/v1' + path,
    method,
    data,
    timeout: 42000,
    header: session ? { Authorization: 'Bearer ' + session } : {}
  });
  return check(result);
}
async function login() {
  if (token) return token;
  if (!loginFlight)
    loginFlight = (async () => {
      const { code } = await invoke('login');
      const session = await call('/login', 'POST', { code });
      if (!session.token) throw Error('微信登录失败');
      return (token = session.token);
    })().finally(() => {
      loginFlight = null;
    });
  return loginFlight;
}
async function cloud(path, method = 'GET', data, readOnly = method === 'GET') {
  const old = await login();
  try {
    return await call(path, method, data, old);
  } catch (e) {
    if (e.status !== 401) throw e;
    if (token === old) token = null;
    // Never replay a user write after an ambiguous response.
    if (!readOnly) throw e;
    return call(path, method, data, await login());
  }
}
async function pair(raw) {
  const qr = crypt.qr(raw);
  const result = await cloud('/pair', 'POST', {
    deviceId: qr.deviceId,
    inviteId: qr.inviteId,
    name: '我的微信'
  });
  if (!result.bindingId) throw Error('配对服务未返回设备标识');
  set('key.' + result.bindingId, qr.key, true);
  return result;
}
async function rpc(binding, method, path, body, extra = {}) {
  const key = get('key.' + binding, true);
  if (!key) throw Error('当前手机没有这台电脑的配对密钥，请重新扫码');
  const id = await crypt.uuid();
  const req = await crypt.seal(key, binding, id, {
    method,
    path,
    issuedAt: Date.now(),
    body,
    ...extra
  });
  const res = crypt.unpack(key, binding, id, await cloud('/rpc', 'POST', req, method === 'GET'));
  return check({ statusCode: res.status, data: res.body });
}
module.exports = { cloud, rpc, pair };
