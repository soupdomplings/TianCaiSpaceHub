const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { resolve } = require('node:path');
const test = require('node:test');
const vm = require('node:vm');
const adapt = vm.runInNewContext(readFileSync(resolve(__dirname,
  '../src/desktop/launcher/zcode-workspace-adapter.js'), 'utf8'));
const providerId = 'tiancai-hub-11111111-2222-3333-4444-555555555555';
const options = { models: [{ providerId, modelId: 'fixture-model' }] };
const native = `
function summary({modelSelectionView:e}) {
  const rows=e?.providers??[];
  return {hasUsableProvider:rows.some(e=>e.models.length>0),providerCount:rows.length};
}
async function startup(n,i,r,t,s) {
  let a=summary({modelSelectionView:n.forceRefresh?await s():i}),{hasUsableProvider:l,providerCount:u}=a,d=!r||!t&&!l;
  return {hasUsableProvider:l,providerCount:u,shouldOpenLoginEntry:d};
}`;
const view = (provider = providerId, model = 'fixture-model') => ({
  providers: [{ providerId: provider, models: [{ modelId: model }], apiKey: 'must-not-be-retained' }]
});
function loaded(source = native, selection = options) {
  const result = adapt(source, selection);
  assert.equal(result.adapted, true);
  const context = vm.createContext({});
  vm.runInContext(result.source, context);
  return { result, context };
}
test('only an actual matching Hub registry model bypasses the startup login gate', async () => {
  const { context } = loaded();
  const result = await context.startup({}, view(), '', null);
  assert.equal(result.shouldOpenLoginEntry, false);
  assert.equal(context.__tiancaiZCodeWorkspace.available, true);
  assert.equal(JSON.stringify(context.__tiancaiZCodeWorkspace).includes('must-not-be-retained'), false);
});
test('other providers, wrong models and an unhydrated registry keep native login', async () => {
  for (const value of [view('external'), view(providerId, 'other-model'), null, { providers: [] }]) {
    const { context } = loaded();
    const result = await context.startup({}, value, '', null);
    assert.equal(result.shouldOpenLoginEntry, true);
    assert.equal(context.__tiancaiZCodeWorkspace.available, false);
  }
});
test('official logged-in behavior and native account data stay unchanged', async () => {
  const { context } = loaded();
  const account = { id: 'native-account' };
  assert.equal((await context.startup({}, null, 'official', account)).shouldOpenLoginEntry, false);
  assert.deepEqual(account, { id: 'native-account' });
  assert.equal(context.providerFamilyDomain, undefined);
  assert.equal(context.user, undefined);
});
test('forceRefresh reads once and uses the refreshed registry', async () => {
  const { context } = loaded();
  let reads = 0;
  const result = await context.startup({ forceRefresh: true }, null, '', null, async () => {
    reads += 1;
    return view();
  });
  assert.equal(reads, 1);
  assert.equal(result.shouldOpenLoginEntry, false);
});
test('options update reuses the current renderer and actual identities', async () => {
  const { context } = loaded();
  await context.startup({}, view(), '', null);
  const state = context.__tiancaiZCodeWorkspace;
  assert.equal(state.updateModels(options), true);
  assert.equal(state.updateModels({ models: [{ providerId, modelId: 'missing' }] }), false);
  assert.equal(state.available, false);
  assert.equal(state.updateModels(options), true);
});
test('a missing, repeated or already patched structure preserves the original source', () => {
  for (const source of ['function unknown(){}', native + native]) {
    const result = adapt(source, options);
    assert.equal(result.adapted, false);
    assert.equal(result.source, source);
  }
  const { result } = loaded();
  assert.equal(adapt(result.source, options).adapted, false);
  assert.equal(adapt(native, { models: [{ providerId: 'official', modelId: 'fixture-model' }] }).adapted, false);
});
test('minifier identifier changes preserve the native gate contract', async () => {
  const { context } = loaded(native.replaceAll('summary', 'renamedSummary').replaceAll('let a=', 'let $result=').replaceAll('}=a,', '}=$result,'));
  assert.equal((await context.startup({}, view(), '', null)).shouldOpenLoginEntry, false);
});
