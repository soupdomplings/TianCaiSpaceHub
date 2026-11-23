const gcm = require('./aes');
const { invoke } = require('./platform');
function b64(bytes) {
  const buffer = bytes.buffer.slice(
    bytes.byteOffset,
    bytes.byteOffset + bytes.byteLength
  );
  return wx
    .arrayBufferToBase64(buffer)
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '');
}
function bytes(value) {
  if (
    typeof value !== 'string' ||
    !/^[A-Za-z0-9_-]*$/.test(value) ||
    value.length % 4 === 1
  )
    throw Error('无效的加密数据');
  const padded =
    value.replace(/-/g, '+').replace(/_/g, '/') +
    '='.repeat((4 - (value.length % 4)) % 4);
  return new Uint8Array(wx.base64ToArrayBuffer(padded));
}
// The native WeChat JS runtime does not consistently provide TextEncoder/TextDecoder.
function utf8(value) {
  const raw = unescape(encodeURIComponent(value));
  const out = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
  return out;
}
function decode(value) {
  let raw = '';
  for (let i = 0; i < value.length; i += 8192)
    raw += String.fromCharCode.apply(null, value.subarray(i, i + 8192));
  return decodeURIComponent(escape(raw));
}
async function random(n) {
  return new Uint8Array(
    (await invoke('getRandomValues', { length: n })).randomValues
  );
}
async function uuid() {
  const a = await random(16);
  a[6] = (a[6] & 15) | 64;
  a[8] = (a[8] & 63) | 128;
  const h = Array.from(a, (n) => n.toString(16).padStart(2, '0')).join('');
  return [
    h.slice(0, 8),
    h.slice(8, 12),
    h.slice(12, 16),
    h.slice(16, 20),
    h.slice(20)
  ].join('-');
}
function cipher(key, binding, id, direction, nonce) {
  const keyBytes = bytes(key),
    iv = bytes(nonce);
  if (keyBytes.length !== 32 || iv.length !== 12)
    throw Error('配对密钥或加密消息无效');
  return gcm(
    keyBytes,
    iv,
    utf8(['tc-hub-v1', binding, id, direction].join('|'))
  );
}
async function seal(key, binding, id, value) {
  const nonce = b64(await random(12));
  const ciphertext = b64(
    cipher(key, binding, id, 'req', nonce).encrypt(utf8(JSON.stringify(value)))
  );
  return { bindingId: binding, requestId: id, nonce, ciphertext };
}
function unpack(key, binding, id, envelope) {
  if (envelope.bindingId !== binding || envelope.requestId !== id)
    throw Error('收到的答复与本次请求不匹配');
  return JSON.parse(
    decode(
      cipher(key, binding, id, 'res', envelope.nonce).decrypt(
        bytes(envelope.ciphertext)
      )
    )
  );
}
function qr(raw) {
  let data;
  try {
    data = JSON.parse(raw);
  } catch {
    throw Error('请扫描目标 Hub「配对手机」显示的二维码');
  }
  const ident = (v) => typeof v === 'string' && /^[\w-]{1,160}$/.test(v);
  if (
    data.v !== 1 ||
    !ident(data.deviceId) ||
    !ident(data.inviteId) ||
    !data.key ||
    bytes(data.key).length !== 32
  )
    throw Error('二维码无效，请在目标 Hub 重新生成');
  return data;
}
module.exports = { bytes, b64, utf8, decode, random, uuid, seal, unpack, qr };
