'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { projectCatalog, projectModel } = require('./model-catalog.cjs');

const snapshot = (models, agents) => ({ freshness: 'fresh', configuration: { models, agents } });
const main = models => ({ name: 'cli', tags: ['cli', 'default'], models });

test('the default CLI agent catalog resolves IDs before names and aliases, preserving order and unique IDs', () => {
  const rows = projectCatalog(snapshot([
    { id: 'not-selected', name: 'second' },
    { id: 'first', name: 'First', aliases: ['first-alias'] },
    { id: 'second', name: 'Second' },
    { id: 'third', name: 'Third', aliases: ['third-alias'] },
    { id: 'image-only', name: 'Image', tags: ['text-to-image'] },
    { id: 'completion', name: 'Completion' }
  ], [main(['second', 'First', 'third-alias', 'first-alias', 'second', 'missing']),
    { name: 'title', tags: ['cli'], models: ['completion'] }]));
  assert.deepEqual(rows.map(row => row.id), ['second', 'first', 'third']);
});

test('main-agent selection follows CLI/default then non-plugin order and keeps concrete declared models first', () => {
  const models = ['plugin', 'one', 'two', 'three'].map(id => ({ id, name: id }));
  const agents = [{ name: 'plugin', tags: ['cli', 'plugin'], models: ['plugin'] },
    { name: 'one', tags: ['cli'], models: ['one'] },
    { ...main(['two']), declaredModel: 'three' }];
  assert.deepEqual(projectCatalog(snapshot(models, agents)).map(row => row.id), ['three', 'two']);
  assert.deepEqual(projectCatalog(snapshot(models, agents.slice(0, 2))).map(row => row.id), ['one']);
  assert.deepEqual(projectCatalog(snapshot(models, [{ ...main(['two']), declaredModel: ' InHerit ' }])).map(row => row.id), ['two']);
  assert.deepEqual(projectCatalog(snapshot(models, [{ ...main(['two']), declaredModel: 'default' }])).map(row => row.id), ['two']);
});

test('only enabled built-in task/chat models are shared, with no custom API credentials or private fields', () => {
  const rows = projectCatalog(snapshot([
    { id: 'task', name: 'Task', url: 'https://private.invalid', apiKey: 'fixture-private-secret',
      contextWindow: 128000, maxOutputTokens: 8000, supportsToolCall: true, supportsImages: false, supportsReasoning: true },
    { id: 'chat', name: 'Chat', tags: ['chat'] },
    { id: 'off', disabled: true, tags: ['chat'] },
    { id: 'custom:a', tags: ['chat'] },
    { id: 'custom-local:b', tags: ['chat'] },
    { id: 'tagged-custom', tags: ['custom', 'chat'] },
    { id: 'configurable', configurable: true, configured: true, tags: ['chat'] },
    { id: 'internal' }
  ], [main(['task', 'off', 'custom:a', 'custom-local:b', 'tagged-custom', 'configurable'])]));
  assert.deepEqual(rows.map(row => row.id), ['task', 'chat']);
  assert.deepEqual(rows[0], { id: 'task', name: 'Task', contextWindow: 128000, maxOutputTokens: 8000,
    billing: { unit: 'credits' }, supportsTools: true, supportsImages: false, supportsThinking: true });
  const body = JSON.stringify(rows);
  for (const privateValue of ['private.invalid', 'fixture-private-secret', 'apiKey', 'url', 'tags']) assert(!body.includes(privateValue));
});

test('same IDs are deduplicated while different IDs with the same display name remain distinguishable', () => {
  const rows = projectCatalog(snapshot([
    { id: 'hy3', name: 'Hy3', credits: 'x0.00 credits' }, { id: 'hy3-x', name: 'Hy3', credits: 'x0.05 credits' },
    { id: 'other', name: 'Other' }, { id: 'hy3', name: 'duplicate' }
  ], [main(['hy3', 'hy3-x', 'hy3', 'other'])]));
  assert.deepEqual(rows.map(row => [row.id, row.name]), [
    ['hy3', 'Hy3 · hy3'], ['hy3-x', 'Hy3 · hy3-x'], ['other', 'Other']
  ]);
  assert.deepEqual(rows.slice(0, 2).map(row => row.billing.multiplier), [0, 0.05]);
});

test('missing or stale selector configuration never falls back to the complete product model directory', () => {
  const base = snapshot([{ id: 'internal' }], [main(['internal'])]);
  assert.deepEqual(projectCatalog({ ...base, freshness: 'stale' }), []);
  assert.deepEqual(projectCatalog(snapshot(base.configuration.models, [])), []);
  assert.deepEqual(projectCatalog(snapshot(base.configuration.models, [{ name: 'cli', models: ['internal'] }])), []);
  assert.deepEqual(projectCatalog(snapshot(base.configuration.models, [{ name: 'plugin', tags: ['cli', 'plugin'], models: ['internal'] }])), []);
  assert.deepEqual(projectCatalog(undefined), []);
  assert.equal(projectModel({ id: ' ', name: 'Empty' }), null);
  assert.deepEqual(projectModel({ id: 'input-only', maxInputTokens: 100000 }),
    { id: 'input-only', name: 'input-only', contextWindow: undefined, maxOutputTokens: undefined, billing: { unit: 'credits' } });
});

test('public credits display retains exact catalog pricing and parses only explicit full multipliers', () => {
  for (const [credits, multiplier] of [
    ['x0.51 credits', 0.51], [' x2.00 credits ', 2], ['x0.21', 0.21], ['X 1.20', 1.2], ['x0.00 credits', 0]
  ]) {
    assert.deepEqual(projectModel({ id: 'priced', credits }).billing,
      { unit: 'credits', display: credits.trim(), multiplier });
  }
  for (const display of ['2 credits per request', 'x0.5 credits plus tools', 'x-1', '目录说明', '1.2']) {
    assert.deepEqual(projectModel({ id: 'described', credits: display }).billing, { unit: 'credits', display });
  }
});

test('unknown pricing is never inferred from balances, promotions, or provider secrets', () => {
  for (const credits of [undefined, null, '', '  ', 1]) {
    const model = projectModel({ id: 'unknown', credits, apiKey: 'fixture-secret', balance: 100,
      promotions: [{ kind: 'discount', discount: { factor: 0.5 } }], billing: { multiplier: 1 } });
    assert.deepEqual(model.billing, { unit: 'credits' });
    assert(!JSON.stringify(model).includes('fixture-secret'));
    assert.equal(model.promotions, undefined);
    assert.equal(model.balance, undefined);
  }
});
