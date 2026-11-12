'use strict';

// WorkBuddy 5.9.3: the task selector receives AgentManager's main-agent
// catalog, not every entry in the confirmed product configuration. Keep this
// projection separate from the host lifecycle and never return provider auth.
function isCustomModel(model) {
  return model.configurable === true || model.tags?.includes('custom') === true
    || model.id.startsWith('custom:') || model.id.startsWith('custom-local:');
}

function projectBilling(model) {
  const billing = { unit: 'credits' };
  const display = typeof model.credits === 'string' ? model.credits.trim() : '';
  if (!display) return billing;
  billing.display = display;
  // Only an explicit full multiplier is numeric. Other public pricing text
  // stays a display value; missing prices must not imply 1x or free usage.
  const multiplier = /^x\s*(\d+(?:\.\d+)?)\s*(?:credits)?$/i.exec(display);
  if (multiplier && Number.isFinite(Number(multiplier[1]))) billing.multiplier = Number(multiplier[1]);
  return billing;
}

function projectModel(model) {
  if (!model || typeof model.id !== 'string' || !model.id.trim() || model.disabled === true) return null;
  const row = { id: model.id, name: typeof model.name === 'string' && model.name.trim() ? model.name.trim() : model.id };
  const positive = value => Number.isSafeInteger(value) && value > 0 ? value : undefined;
  row.contextWindow = positive(model.contextWindow);
  row.maxOutputTokens = positive(model.maxOutputTokens);
  row.billing = projectBilling(model);
  if (typeof model.supportsToolCall === 'boolean') row.supportsTools = model.supportsToolCall;
  if (typeof model.supportsImages === 'boolean') row.supportsImages = model.supportsImages;
  if (typeof model.supportsReasoning === 'boolean') row.supportsThinking = model.supportsReasoning;
  return row;
}

function projectCatalog(snapshot) {
  if (snapshot?.freshness !== 'fresh' || !Array.isArray(snapshot.configuration?.models)) return [];
  const configuration = snapshot.configuration;
  const agents = Array.isArray(configuration.agents)
    ? configuration.agents.filter(agent => agent?.tags?.includes('cli')) : [];
  const main = agents.find(agent => agent.tags?.includes('default'))
    ?? agents.find(agent => !agent.tags?.includes('plugin'));
  if (!main) return [];

  const models = configuration.models.filter(model => model && typeof model.id === 'string');
  // Match AgentManager.computeModelsFromRaw / findModelByIdOrName: an exact
  // ID wins over a display name, which wins over aliases. Preserve that ID.
  const find = value => models.find(model => model.id === value)
    ?? models.find(model => model.name === value)
    ?? models.find(model => model.aliases?.includes(value));
  const selected = Array.isArray(main.models) ? main.models : [];
  const declared = typeof main.declaredModel === 'string' ? main.declaredModel.trim() : '';
  const declaredIsConcrete = declared && !['inherit', 'default'].includes(declared.toLowerCase());
  const requested = declaredIsConcrete ? [declared, ...selected.filter(value => value !== declared)] : selected;
  const seen = new Set();
  const rows = [];
  const append = model => {
    if (!model || seen.has(model.id) || isCustomModel(model)) return;
    const row = projectModel(model);
    if (!row) return;
    seen.add(row.id);
    rows.push(row);
  };
  for (const value of requested) if (typeof value === 'string') append(find(value));
  // AgentManager also appends custom/chat models. Only chat entries belong in
  // this built-in sharing feature; custom API models keep their own channels.
  for (const model of models) if (model.tags?.includes('chat')) append(model);

  const names = new Map();
  for (const row of rows) names.set(row.name, (names.get(row.name) ?? 0) + 1);
  for (const row of rows) if (names.get(row.name) > 1) row.name += ' · ' + row.id;
  return rows;
}

module.exports = { projectCatalog, projectModel };
