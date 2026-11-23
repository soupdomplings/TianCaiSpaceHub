// Local experience fixture. Copied only into preview/, never into production miniprogram/.
const { set, get } = require('./platform');
const binding = 'demo-only';
// A simulated target Hub. Change this fixture and regenerate preview/ to
// inspect each category without reintroducing phone-owned settings.
const displayContent = { reasoning: false, tool: false, fileChange: false, artifact: false };
const capabilities = {
  history: true,
  create: true,
  send: true,
  stop: true,
  approvals: true,
  questions: true,
  artifacts: true,
  liveActivity: true
};
const approval = {
  id: 'demo-approval',
  type: 'approval',
  title: '允许保存报告吗？',
  message: '写入项目中的演示报告。',
  choices: [
    { value: 'accept', label: '允许保存' },
    { value: 'decline', label: '拒绝' }
  ]
};
const question = {
  id: 'demo-question',
  type: 'question',
  title: '选择报告格式',
  questions: [
    {
      id: 'format',
      prompt: '你希望收到哪种格式？',
      options: [
        { value: 'pdf', label: 'PDF' },
        { value: 'md', label: 'Markdown' }
      ],
      allowOther: true,
      required: true
    }
  ]
};
function initial() {
  return {
    sessions: [{ id: 'demo-chat', title: '周报整理' }],
    threads: {
      'demo-chat': {
        items: [
          {
            id: 'welcome',
            role: 'assistant',
            type: 'message',
            text: '演示报告已准备好，请确认格式和保存权限。'
          }
        ],
        approvals: [question, approval],
        events: [],
        status: 'waiting_for_user_input'
      }
    },
    orders: {},
    balance: 36
  };
}
let db;
function state() {
  if (!db) db = get('fixtures') || initial();
  return db;
}
function save() {
  set('fixtures', db);
}
function emit(thread, session, type, data, itemId) {
  thread.events.push({
    id: thread.events.length + 1,
    clientId: 'codex',
    sessionId: session,
    type,
    data,
    itemId
  });
}
async function cloud(path, method, data) {
  const s = state();
  if (path === '/devices') {
    set('key.' + binding, 'preview-only', true);
    return {
      devices: [
        {
          bindingId: binding,
          deviceId: 'demo-machine',
          name: '演示工作电脑',
          state: 'active',
          online: true
        }
      ]
    };
  }
  if (path === '/squad')
    return {
      bound: true,
      balance: s.balance,
      announcements: [
        {
          id: 'news',
          title: '欢迎体验天才小分队',
          content: '这是本地样例，可查看个人用量、充值预览及到账结果。'
        }
      ],
      usage: [{ id: 'today', label: '今天 · 演示用量', amount: 2.4 }],
      payment: {
        enabled: true,
        platforms: ['ios', 'android', 'devtools', 'windows', 'mac'],
        amounts: [10, 30, 100]
      }
    };
  if (path === '/squad/quote')
    return {
      quoteId: 'sample-' + data.amount,
      amount: data.amount,
      payable: data.amount,
      credit: data.amount
    };
  if (path === '/squad/orders') {
    if (!s.orders[data.requestId]) {
      const amount = Number(data.quoteId.split('-')[1]);
      s.orders[data.requestId] = {
        id: data.requestId,
        amount,
        credit: amount,
        state: 'paid'
      };
      s.balance += amount;
      save();
    }
    return s.orders[data.requestId];
  }
  if (path.startsWith('/squad/orders/'))
    return s.orders[path.slice('/squad/orders/'.length)];
  throw Error('本地演示未包含此操作');
}
async function pair() {
  throw Error('本地演示使用已配对的样例电脑');
}
async function rpc(b, method, path, body) {
  const s = state();
  if (b !== binding) throw Error('本地演示设备无效');
  const p = path.split('?')[0];
  if (p === '/clients')
    return {
      clients: [{ id: 'codex', name: 'Codex', ready: true, capabilities }]
    };
  if (p.endsWith('/projects'))
    return {
      projects: [{ id: 'demo-project', name: '演示项目', path: 'demo' }]
    };
  if (p.endsWith('/sessions')) {
    if (method === 'GET') return { sessions: s.sessions };
    const session = { id: 'example-' + Date.now(), title: '新的演示会话' };
    s.sessions.unshift(session);
    s.threads[session.id] = {
      items: [],
      approvals: [],
      events: [],
      status: 'idle'
    };
    save();
    return { session };
  }
  const parts = p.split('/'),
    sid = parts[4],
    th = s.threads[sid];
  if (!th) throw Error('未找到演示会话');
  if (parts[5] === 'history')
    return {
      items: th.items,
      approvals: th.approvals,
      status: th.status,
      displayContent,
      cursor: th.events.length
    };
  if (parts[5] === 'events') {
    await new Promise((resolve) => setTimeout(resolve, 700));
    const after = Number(/after=(\d+)/.exec(path)?.[1] || 0);
    return { events: th.events.slice(after).map(event => ({...event, data:{...event.data, displayContent}})) };
  }
  if (parts[5] === 'approvals') {
    th.approvals = th.approvals.filter((a) => a.id !== parts[6]);
    emit(th, sid, 'approval.resolved', { id: parts[6] });
    th.status = th.approvals.length ? 'waiting_for_user_input' : 'idle';
    emit(th, sid, 'turn.updated', { status: th.status });
    save();
    return { status: th.status };
  }
  if (parts[5] === 'turns') {
    const id = body.requestId,
      user = { id, role: 'user', type: 'message', text: body.text };
    const reply = {
      id: id + '-reply',
      role: 'assistant',
      type: 'message',
        text: '演示报告已经生成。',
        files: [{id:id+'-artifact',name:'演示报告.txt',kind:'artifact',mimeType:'text/plain',size:9}]
    };
    for (const [category, text] of [
      ['reasoning', '已读取演示请求'],
      ['tool', '检索演示项目'],
      ['fileChange', '更新演示记录']
    ])
      emit(
        th,
        sid,
        'cursor.advanced',
        { liveActivity: { category, mode: 'entry', text } },
        id + category
      );
    for (const item of [user, reply]) {
      th.items.push(item);
      emit(th, sid, 'item.updated', { item });
    }
    th.status = 'idle';
    emit(th, sid, 'turn.updated', { status: 'idle' });
    save();
    return { status: 'running', turnId: id };
  }
  if (parts[5] === 'files' && method === 'GET' && th.items.some(item=>item.files?.some(file=>file.id===parts[6]))) {
    return {chunk:'SHViIGRlbW8K',nextOffset:null};
  }
  if (parts[5] === 'stop') {
    th.status = 'idle';
    emit(th, sid, 'turn.updated', { status: 'idle' });
    save();
    return { status: 'idle' };
  }
  throw Error('本地演示未包含此操作');
}
module.exports = { cloud, rpc, pair };
