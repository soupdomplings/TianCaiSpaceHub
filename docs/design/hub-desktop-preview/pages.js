/* Offline interaction preview. All fixtures are invented and live only in this tab. */
(() => {
  'use strict';
  const icon = (name, size = 18) => window.hubIcon(name, size);
  const el = (id) => document.getElementById(id);
  const number = (n) => n.toLocaleString('en-US');
  const pairs = [
    { name: 'OpenAI', tone: 'graphite', icon: 'spark', models: ['示例 GPT · 标准', '示例 GPT · 快速', '示例 GPT · 深度'] },
    { name: 'DeepSeek', tone: 'blue', icon: 'pulse', models: ['示例 DeepSeek · 对话', '示例 DeepSeek · 推理'] },
    { name: 'MiniMax', tone: 'coral', icon: 'bolt', models: ['示例 MiniMax · 通用', '示例 MiniMax · 编程'] },
  ];
  const clients = [
    { name: 'ChatGPT / Codex', icon: 'code', tone: 'graphite', active: true, selected: [0, 1] },
    { name: 'WorkBuddy', icon: 'bolt', tone: 'blue', active: true, selected: [0, 3] },
    { name: '天工 Work', icon: 'spark', tone: 'violet', active: true, selected: [2, 5] },
    { name: 'DeepSeek', icon: 'link', tone: 'blue', active: false, selected: [3] },
    { name: 'ZCode', icon: 'command', tone: 'coral', active: false, selected: [5] },
  ];
  const catalog = pairs.flatMap((pair) => pair.models);
  const available = pairs.map((pair) => new Set(pair.models));
  const dirty = pairs.map((pair) => new Set(pair.models));
  const samples = [
    { values: [12, 9, 16, 30, 42, 95, 83, 155, 130, 210, 172, 330], axes: ['00:00', '06:00', '12:00', '18:00', '24:00'], desc: '每两小时的请求量', failure: 3, latency: '1.2 秒' },
    { values: [1120, 936, 1262, 1100, 1548, 1692, 1284], axes: ['六天前', '四天前', '两天前', '今天'], desc: '每天的请求量', failure: 22, latency: '1.4 秒' },
  ];
  const requests = [
    ['16:42:09', 0, 0, '成功', '1.2 秒'], ['16:39:27', 1, 3, '成功', '0.9 秒'],
    ['16:37:04', 2, 5, '成功', '1.5 秒'], ['16:31:18', 0, 1, '失败', '4.0 秒'],
    ['16:29:40', 1, 0, '成功', '1.3 秒'], ['16:22:16', 2, 2, '成功', '2.1 秒'],
  ];
  const projects = [
    ['城市漫步手册', '整理步行路线'], ['城市漫步手册', '核对地图标记'], ['春季书目', '编排读书卡片'],
  ];
  const restored = new Set();
  let period = 0, provider = 0, logFilter = 0;
  const badge = (mark, tone = '') => `<span class="appicon ${tone}" aria-hidden="true">${icon(mark)}</span>`;
  const status = (ok, yes = '已连接', no = '未运行') => `<span class="pill ${ok ? 'ok' : no === '失败' ? 'error' : 'idle'}"><i class="dot"></i>${ok ? yes : no}</span>`;
  const pageTitle = (title) => `<h1 class="sr-only">${title}</h1>`;
  const seg = (name, options, selected, action) => `<div class="seg" role="group" data-track="segment" aria-label="${name}">${options.map((text, i) =>
    `<button type="button" data-${action}="${i}" class="${i === selected ? 'active' : ''}" aria-pressed="${i === selected}">${text}</button>`).join('')}</div>`;
  const info = (mark, title, sub, value, tone = '') => `<div class="healthrow">${badge(mark, tone)}<div><b>${title}</b>${sub ? `<small class="subtle">${sub}</small>` : ''}</div><strong>${value}</strong></div>`;
  const button = (text, action, cls = 'quiet') => `<button type="button" class="${cls}" data-act="${action}">${text}</button>`;

  function chart(sample) {
    const width = 480, top = 8, bottom = 95, max = Math.max(...sample.values) * 1.09;
    const pts = sample.values.map((v, i) => [4 + (width - 8) * i / (sample.values.length - 1), bottom - v / max * (bottom - top)]);
    let d = `M ${pts[0][0]} ${pts[0][1]}`;
    for (let i = 1; i < pts.length; i++) {
      const [x, y] = pts[i], [px, py] = pts[i - 1], mid = (x + px) / 2;
      d += ` C ${mid} ${py}, ${mid} ${y}, ${x} ${y}`;
    }
    const [x, y] = pts[pts.length - 1];
    return `<svg class="chart" viewBox="0 0 ${width} 100" preserveAspectRatio="none" role="img" aria-label="${sample.desc}：${sample.values.join('、')}">
      <defs><linearGradient id="trendWash" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stop-color="currentColor" stop-opacity=".23"/><stop offset="100%" stop-color="currentColor" stop-opacity="0"/></linearGradient></defs>
      ${[12, 40, 68, 96].map((n) => `<path d="M 0 ${n} H ${width}" stroke="currentColor" stroke-opacity=".13" stroke-dasharray="3 5" vector-effect="non-scaling-stroke"/>`).join('')}
      <path class="trendfill" d="${d} L ${x} 100 L 4 100 Z" fill="url(#trendWash)"/>
      <path class="trendline" d="${d}" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" fill="none" vector-effect="non-scaling-stroke"/>
      <circle cx="${x}" cy="${y}" r="3.3" fill="currentColor"/></svg><div class="axis">${sample.axes.map((label) => `<span>${label}</span>`).join('')}</div>`;
  }
  function overviewStats() {
    const sample = samples[period], total = sample.values.reduce((a, b) => a + b, 0);
    return {
      trend: `<div class="metric">${number(total)} <small>次请求</small></div>${chart(sample)}`,
      health: `<div class="cardhead"><h2>${period ? '近 7 天状态' : '今日状态'}</h2></div>${info('link', '本机网关', '', '运行中', 'mint')}${info('check', '请求成功率', '', ((total - sample.failure) * 100 / total).toFixed(1) + '%', 'blue')}${info('clock', '首次响应（平均）', '', sample.latency, 'violet')}`
    };
  }
  function overview() {
    const stats = overviewStats();
    return pageTitle('概览')
      + `<div class="chartgrid"><section class="card trendcard"><div class="cardhead"><h2>请求趋势</h2>${seg('统计范围', ['今天', '近 7 天'], period, 'period')}</div><div data-period-stats>${stats.trend}</div></section>
        <section class="card" data-period-health>${stats.health}</section></div>
        <div class="sectionhead"><h2>客户端</h2><span class="subtle">3 个已连接 · 共 5 个</span></div>
        <div class="card tablecard"><div class="tablehead" aria-hidden="true"><span>名称</span><span>首选模型</span><span>状态</span><span></span></div>
        ${clients.map((c, i) => `<button type="button" class="clientrow" data-client="${i}"><span class="clientname">${badge(c.icon, c.tone)}<b>${c.name}</b></span><span class="subtle">${catalog[c.selected[0]] || '未选择模型'}</span>${status(c.active)}${icon('chevron', 15)}</button>`).join('')}</div>`;
  }
  const same = (a, b) => a.size === b.size && [...a].every((v) => b.has(v));
  const choice = (label, attr, id, checked, detail = '') => `<label class="choice-row"><span><b>${label}</b>${detail ? `<small class="subtle">${detail}</small>` : ''}</span><input type="checkbox" data-${attr}="${id}" ${checked ? 'checked' : ''}></label>`;
  const choiceGroup = (name, rows) => rows.length ? `<div role="group" aria-label="${name}">${rows.join('')}</div>` : '';
  const picked = () => `已选 ${dirty[provider].size} / ${pairs[provider].models.length}`;
  function providerPanel() {
    const p = pairs[provider];
    return `<div class="cardhead"><h2>${p.name}</h2><span class="subtle" data-model-count role="status">${picked()}</span></div>
      ${choiceGroup(p.name + ' 可用模型', p.models.map((model, i) => choice(model, 'model', i, dirty[provider].has(model))))}
      <div class="sheetactions"><button type="button" class="primary" data-act="save-provider" ${same(available[provider], dirty[provider]) ? 'disabled' : ''}>保存更改</button></div>`;
  }
  function models() {
    return pageTitle('模型与渠道')
      + `<div class="chartgrid modelgrid"><section class="card"><div class="cardhead"><h2>厂商渠道</h2><span class="subtle">3 个</span></div>
      <div class="providertrack" data-track="providers" role="group" aria-label="厂商渠道">${pairs.map((item, i) => `<button type="button" data-provider="${i}" class="providerrow healthrow ${i === provider ? 'active' : ''}" aria-pressed="${i === provider}">${badge(item.icon, item.tone)}<span><b>${item.name}</b><small class="subtle" data-available-count>${available[i].size} 个可用模型</small></span>${icon('chevron', 15)}</button>`).join('')}</div></section>
      <section class="card" data-provider-panel>${providerPanel()}</section></div>`;
  }
  const indexOfClient = () => Math.max(0, clients.findIndex((c) => window.demoClient === c.name));
  function client() {
    const id = indexOfClient(), c = clients[id];
    return pageTitle(c.name)
      + `<section class="card"><div class="cardhead"><h2>客户端模型</h2><div class="inline-tools">${status(c.active)}${button('选择模型', 'client-models')}</div></div>
        ${c.selected.length ? c.selected.map((mid, i) => info('models', catalog[mid], pairs.find((p) => p.models.includes(catalog[mid])).name, i === 0 ? '首选' : '', 'blue')).join('') : '<p class="subtle">还没有选择模型。</p>'}</section>
        ${id === 0 ? `<section class="card maintenance"><div class="cardhead"><div><h2>检查与修复</h2><p class="subtle">找回侧栏缺少的项目和会话。</p></div>${button('检查项目 ' + icon('arrow', 15), 'repair')}</div></section>` : ''}`;
  }
  function clientModels() {
    const c = clients[indexOfClient()], selected = new Set(c.selected);
    const visible = catalog.map((m, i) => ({ m, i })).filter(({m, i}) => available.some((a) => a.has(m)) || selected.has(i));
    const d = window.hubSheet('选择客户端模型', `<p class="subtle">保存后，${c.name} 的演示模型列表会随之更新。</p>
      ${choiceGroup('客户端模型', visible.map(({m, i}) => choice(m, 'client-model', i, selected.has(i), pairs.find((p) => p.models.includes(m)).name)))}
      <div class="sheetactions">${button('取消', 'cancel-models')}${button('保存选择', 'apply-models', 'primary')}</div>`);
    d.querySelector('[data-act="cancel-models"]').onclick = window.closeHubSheet;
    d.querySelector('[data-act="apply-models"]').onclick = () => {
      const checked = [...d.querySelectorAll('[data-client-model]:checked')].map((e) => Number(e.dataset.clientModel));
      c.selected = [...c.selected.filter((id) => checked.includes(id)), ...checked.filter((id) => !selected.has(id))];
      d.addEventListener('close', () => {
        render('client'); el('view').querySelector('[data-act="client-models"]').focus({preventScroll: true});
      }, {once: true});
      window.closeHubSheet();
      window.hubToast(`已保存 ${c.selected.length} 个演示模型`);
    };
  }
  function logRows() {
    const chosen = requests.map((r, i) => ({ r, i })).filter(({r}) => !logFilter || r[3] === '失败');
    return chosen.map(({r, i}) => `<tr><td>${r[0]}</td><td><b>${clients[r[1]].name}</b><small class="subtle">${catalog[r[2]]}</small></td><td>${status(r[3] === '成功', '成功', '失败')}</td><td>${r[4]}</td><td><button type="button" class="tool" data-log="${i}" aria-label="查看 ${r[0]} 的请求详情">${icon('chevron', 15)}</button></td></tr>`).join('');
  }
  function logs() {
    return pageTitle('请求日志')
      + `<section class="card tablecard"><div class="cardhead logtools"><h2>请求记录</h2>${seg('请求状态', ['全部', '仅失败'], logFilter, 'logs')}</div>
      <div class="tablewrap"><table class="logtable"><thead><tr><th>时间</th><th>客户端 / 模型</th><th>状态</th><th>耗时</th><th><span class="subtle">详情</span></th></tr></thead>
      <tbody data-log-rows>${logRows()}</tbody></table></div></section>`;
  }
  function openLog(id) {
    const [time, cid, mid, result, delay] = requests[id];
    const d = window.hubSheet('请求详情', `<div class="sectionhead"><b>${clients[cid].name}</b>${status(result === '成功', '成功', '失败')}</div>
      ${info('models', '模型', pairs.find((p) => p.models.includes(catalog[mid])).name, catalog[mid])}
      ${info('clock', '请求时间', '今天', time)}${info('pulse', '耗时', '收到响应', delay)}
      ${result === '失败' ? '<p>上游未在限定时间内响应。</p>' : ''}<div class="sheetactions">${button('完成', 'done-log')}</div>`);
    d.querySelector('[data-act="done-log"]').onclick = window.closeHubSheet;
  }
  function openRepair() {
    const pending = projects.map(([project, name], i) => ({ name, project, i })).filter(({i}) => !restored.has(i));
    const d = window.hubSheet('检查与修复', `<div class="eyebrow">CHATGPT / CODEX</div><h3>找回项目里的会话</h3><p class="subtle">${pending.length ? '勾选你要放回项目的会话，已有的项目和会话保持原样。' : '演示中的会话均已归入项目。'}</p>
      ${choiceGroup('要归回项目的会话', pending.map(({name, project, i}) => choice(name, 'repair', i, false, '归入「' + project + '」')))}
      <p class="subtle">这里只预览操作，不会修改本机。</p>
      <div class="sheetactions"><span data-scope class="subtle" aria-live="polite"></span>
      ${button(pending.length ? '取消' : '完成', 'cancel-repair')}${pending.length ? '<button type="button" class="primary" data-act="apply-repair" disabled>恢复所选会话</button>' : ''}</div>`);
    d.querySelector('[data-act="cancel-repair"]').onclick = window.closeHubSheet;
    if (!pending.length) return;
    const selected = () => [...d.querySelectorAll('[data-repair]:checked')].map((e) => Number(e.dataset.repair));
    const apply = d.querySelector('[data-act="apply-repair"]');
    const update = () => {
      const ids = selected(), count = new Set(ids.map((id) => projects[id][0])).size;
      d.querySelector('[data-scope]').textContent = `已选 ${ids.length} 个会话 · ${count} 个项目`;
      apply.disabled = !ids.length;
    };
    d.querySelectorAll('[data-repair]').forEach((e) => { e.onchange = update; });
    apply.onclick = () => {
      const ids = selected();
      ids.forEach((id) => restored.add(id));
      window.closeHubSheet(); window.hubToast(`已将 ${ids.length} 个演示会话归回项目`);
    };
    update();
  }
  function render(page) {
    const host = el('view'), templates = { overview, models, client, logs };
    if (!Object.hasOwn(templates, page)) return;
    host.innerHTML = templates[page]();
    host.querySelectorAll('[data-track]').forEach((group) => {
      window.hubTrack(group, group.querySelector('button[aria-pressed="true"]'));
    });
    const moveSelection = (b) => {
      const group = b.closest('[data-track]');
      group.querySelectorAll('button[aria-pressed]').forEach((control) => {
        control.classList.toggle('active', control === b);
        control.setAttribute('aria-pressed', String(control === b));
      });
      window.hubTrack(group, b);
    };
    host.onclick = (event) => {
      const b = event.target.closest('button');
      if (!b || !host.contains(b)) return;
      const a = b.dataset;
      if ('period' in a && period !== Number(a.period)) {
        period = Number(a.period); moveSelection(b);
        const stats = overviewStats();
        host.querySelector('[data-period-stats]').innerHTML = stats.trend;
        host.querySelector('[data-period-health]').innerHTML = stats.health;
      }
      if ('client' in a) { window.demoClient = clients[Number(a.client)].name; window.showHub('client', window.demoClient); }
      if ('provider' in a && provider !== Number(a.provider)) {
        provider = Number(a.provider); moveSelection(b);
        host.querySelector('[data-provider-panel]').innerHTML = providerPanel();
      }
      if ('logs' in a && logFilter !== Number(a.logs)) {
        logFilter = Number(a.logs); moveSelection(b);
        host.querySelector('[data-log-rows]').innerHTML = logRows();
      }
      if ('log' in a) openLog(Number(a.log));
      if (a.act === 'repair') openRepair();
      if (a.act === 'client-models') clientModels();
      if (a.act === 'save-provider') {
        available[provider] = new Set(dirty[provider]); b.disabled = true;
        const source = host.querySelector(`[data-provider="${provider}"]`);
        source.querySelector('[data-available-count]').textContent = available[provider].size + ' 个可用模型';
        source.focus({preventScroll: true});
        window.hubToast(`已保存 ${pairs[provider].name} 的模型选择`);
      }
    };
    host.onchange = ({ target: box }) => {
      if (!box.matches('[data-model]')) return;
      const value = pairs[provider].models[Number(box.dataset.model)];
      if (box.checked) dirty[provider].add(value); else dirty[provider].delete(value);
      host.querySelector('[data-model-count]').textContent = picked();
      host.querySelector('[data-act="save-provider"]').disabled = same(available[provider], dirty[provider]);
    };
  }
  window.HubDemo = { render, openRepair };
})();
