const test = require('node:test');
const assert = require('node:assert/strict');
const {
  randomBytes,
  createDecipheriv,
  createCipheriv
} = require('node:crypto');
const memory = new Map();
global.wx = {
  arrayBufferToBase64: (ab) => Buffer.from(ab).toString('base64'),
  base64ToArrayBuffer: (b64) =>
    Uint8Array.from(Buffer.from(b64, 'base64')).buffer,
  getRandomValues: ({ length, success }) =>
    success({ randomValues: Uint8Array.from(randomBytes(length)).buffer }),
  getStorageSync: (k) => memory.get(k),
  setStorageSync: (k, v) => memory.set(k, v)
};
const crypt = require('../miniprogram/lib/crypto');
const { snapshot, reduce, form } = require('../miniprogram/lib/conversation');
test('desktop AES-GCM interop binds device, request and message direction', async () => {
  const key = randomBytes(32),
    binding = 'binding-123',
    id = await crypt.uuid();
  const request = await crypt.seal(key.toString('base64url'), binding, id, {
    text: '在外面 查看 💻'
  });
  const blob = Buffer.from(request.ciphertext, 'base64url');
  const decrypt = createDecipheriv(
    'aes-256-gcm',
    key,
    Buffer.from(request.nonce, 'base64url')
  );
  decrypt.setAAD(Buffer.from('tc-hub-v1|' + binding + '|' + id + '|req'));
  decrypt.setAuthTag(blob.subarray(-16));
  assert.deepEqual(
    JSON.parse(
      Buffer.concat([decrypt.update(blob.subarray(0, -16)), decrypt.final()])
    ),
    { text: '在外面 查看 💻' }
  );
  const nonce = randomBytes(12),
    en = createCipheriv('aes-256-gcm', key, nonce);
  en.setAAD(Buffer.from('tc-hub-v1|' + binding + '|' + id + '|res'));
  const ct = Buffer.concat([
    en.update(JSON.stringify({ status: 200, body: '好' })),
    en.final(),
    en.getAuthTag()
  ]).toString('base64url');
  const response = {
    bindingId: binding,
    requestId: id,
    ciphertext: ct,
    nonce: nonce.toString('base64url')
  };
  assert.deepEqual(
    crypt.unpack(key.toString('base64url'), binding, id, response),
    { status: 200, body: '好' }
  );
  assert.throws(() =>
    crypt.unpack(key.toString('base64url'), binding, id, request)
  );
  assert.throws(() =>
    crypt.unpack(key.toString('base64url'), 'other', id, response)
  );
});
test('pairing QR validates format and rejects incomplete keys', () => {
  const q = {
    v: 1,
    deviceId: 'pc',
    inviteId: 'invite',
    key: randomBytes(32).toString('base64url')
  };
  assert.deepEqual(crypt.qr(JSON.stringify(q)), q);
  assert.throws(() => crypt.qr(JSON.stringify({ ...q, key: 'AA' })));
  assert.throws(() => crypt.qr('https://untrusted.invalid'));
});
test('live toggles capture subsequent public activity without replacing historical rows or approvals', () => {
  const history = snapshot({
    items: [
      { id: 'h', role: 'user', type: 'message', text: '工具执行' },
      {
        id: 'x',
        role: 'tool',
        type: 'tool',
        text: 'private',
        files: [{ id: 'file' }]
      }
    ],
    cursor: 4,
    status: 'running'
  });
  const event = (id, cat, text) => ({
    id,
    type: 'cursor.advanced',
    data: { liveActivity: { itemId: 'i', category: cat, mode: 'append', text } }
  });
  let result = reduce(history, event(5, 'tool', 'before'), {});
  result = reduce(result, event(6, 'tool', 'after'), { tool: true });
  result = reduce(result, event(7, 'tool', 'again'), {});
  assert.deepEqual(result.items.slice(0, 2), history.items);
  assert.equal(result.items[2].text, 'after');
  const pending = reduce(
    result,
    {
      id: 8,
      type: 'approval.requested',
      data: { approval: { id: 'q', type: 'question' } }
    },
    {}
  );
  assert.equal(pending.approvals[0].id, 'q');
  assert.deepEqual(
    reduce(pending, { id: 9, type: 'approval.resolved', data: { id: 'q' } }, {})
      .approvals,
    []
  );
  assert.equal(JSON.stringify(result).includes('before'), false);
  assert.equal(result.items[1].files[0].id, 'file');
});
test('question actions keep native values, required numbers and desktop-only status', () => {
  const q = {
    type: 'question',
    questions: [
      { id: 'size', prompt: '数量', inputType: 'integer' },
      {
        id: 'lang',
        prompt: '选择',
        options: [{ label: '中文', value: 'zh' }],
        allowOther: false
      }
    ]
  };
  assert.equal(
    form(q, { size: '2.5', lang: 'zh' }, true).actions.find(
      (x) => x.value === 'accept'
    ).disabled,
    true
  );
  assert.equal(
    form(q, { size: '2', lang: 'zh' }, true).actions.find(
      (x) => x.value === 'accept'
    ).disabled,
    false
  );
  assert.equal(
    form(q, { size: '2', lang: 'fake' }, true).actions.find(
      (x) => x.value === 'accept'
    ).disabled,
    true
  );
  assert.equal(form(q, {}, false).desktopOnly, true);
  assert.equal(
    form(
      {
        type: 'approval',
        choices: [{ label: '此会话允许', value: 'option:1' }]
      },
      {},
      true
    ).actions[0].value,
    'option:1'
  );
});
