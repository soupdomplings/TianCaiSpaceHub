const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { resolve } = require('node:path');
const test = require('node:test');
const vm = require('node:vm');
const adapt = vm.runInNewContext(readFileSync(resolve(__dirname,
  '../src/desktop/launcher/zcode-workspace-adapter.js'), 'utf8'));
const probe = readFileSync(resolve(__dirname,
  '../src/desktop/launcher/zcode_workspace.rs'), 'utf8')
  .match(/const PROBE: &str = r#"([\s\S]*?)"#;/)[1];
const page = 'file:///D:/Program%20Files/ZCode/resources/app.asar/out/renderer/index.html';
function inspectPage({ links = [], resources = [], selectors = [], inputs = [] } = {}) {
  return vm.runInNewContext(probe, {
    window: {}, location: { href: page },
    performance: { getEntriesByType: () => resources.map(name => ({ name })) },
    document: {
      querySelector: query => query.split(',').some(value => selectors.includes(value)) ? {} : null,
      querySelectorAll: query => query.includes('modulepreload') ? links : inputs
    }
  });
}

test('startup finds the native preload beyond a full resource timing buffer', () => {
  // ZCode 3.17.2 index.html preloads 316 modules; styles is the last one.
  const links = Array.from({ length: 315 }, (_, i) => ({ href: new URL(`assets/chunk-${i}.js`, page).href }));
  const module = new URL('assets/styles-fixture.js', page).href;
  links.push({ href: module });
  const result = inspectPage({ links, resources: links.slice(0, 250).map(link => link.href),
    selectors: ['[data-testid="login-use-api-key-button"]'] });
  assert.equal(result.module, module);
  assert.equal(result.loginOpen, true);
  assert.equal(result.workspaceOpen, false);
  assert.equal(result.page, page);
});

test('startup recognizes a cached preload without resource timing and keeps page protections', () => {
  const module = new URL('assets/styles-cached.js', page).href;
  const result = inspectPage({ links: [{ href: module }],
    selectors: ['[data-testid="sidebar-primary-navigation"]', '[data-testid="oauth-cancel"]'],
    inputs: [{ value: 'unsent draft' }] });
  assert.equal(result.module, module);
  assert.equal(result.workspaceOpen, true);
  assert.equal(result.loginOpen, false);
  assert.equal(result.oauthPending, true);
  assert.equal(result.hasInput, true);
});

test('startup retains resource timing fallback and does not confuse stylesheets with scripts', () => {
  const module = new URL('assets/styles-dynamic.js', page).href;
  const resources = [new URL('assets/styles-fixture.css', page).href, module];
  assert.equal(inspectPage({ resources }).module, module);
  assert.equal(inspectPage({ links: [{ href: resources[0] }] }).module, null);
});
test('native login header is ready while provider discovery has not rendered its buttons', () => {
  const module = new URL('assets/styles-provider-loading.js', page).href;
  const result = inspectPage({ links: [{ href: module }],
    selectors: ['main > section header [role="img"][aria-label="ZCode"]'] });
  assert.equal(result.loginOpen, true);
  assert.equal(result.workspaceOpen, false);
  assert.equal(result.oauthPending, false);
  assert.equal(result.hasInput, false);
  const active = inspectPage({ links: [{ href: module }],
    selectors: ['main > section header [role="img"][aria-label="ZCode"]', '[data-testid="oauth-cancel"]'],
    inputs: [{ value: 'unfinished login input' }] });
  assert.equal(active.oauthPending, true);
  assert.equal(active.hasInput, true);
});
test('native database and root loading screens remain separate from a login ready to adapt', () => {
  const result = inspectPage({selectors: ['[data-testid="root-startup-loading"]','[data-testid="database-startup-silent"]']});
  assert.equal(result.startupLoading, true);
  assert.equal(result.databaseStarting, true);
  assert.equal(result.loginOpen, false);
  assert.equal(result.workspaceOpen, false);
});
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
