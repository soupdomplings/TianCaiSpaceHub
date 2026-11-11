'use strict';

// Loaded only by the user-requested WorkBuddy launch. Account credentials stay
// inside the installed host; this module publishes a short-lived local ledger.
const Module = require('node:module');
const path = require('node:path');
const http = require('node:http');
const crypto = require('node:crypto');
const ATTACH = Symbol.for('tiancaispace.workbuddy.builtin-share');
const ANCHOR = 'let ye=createSkillVlmService({authenticationManager:e.authenticationManager,productManager:e.productManager,appVersion:r.appVersion,fetch:s,logger:O});';
const ENV = 'TIANCAI_WORKBUDDY_';
const HEARTBEAT_MS = 1000;

function optionsFromEnv(env) {
  if (!env[ENV + 'CALLBACK'] || !env[ENV + 'LEASE'] || !env[ENV + 'SERVER']) return null;
  const callback = new URL(env[ENV + 'CALLBACK']);
  if (callback.protocol !== 'http:' || callback.hostname !== '127.0.0.1' || callback.pathname !== '/') return null;
  return { callback, lease: env[ENV + 'LEASE'], server: env[ENV + 'SERVER'],
    bridgeId: env[ENV + 'BRIDGE_ID'], originalNodeOptions: env[ENV + 'ORIGINAL_NODE_OPTIONS'] || '' };
}

function notify(options, report) {
  return new Promise((resolve, reject) => {
    const body = Buffer.from(JSON.stringify({ bridgeId: options.bridgeId, ...report }));
    const request = http.request(options.callback, {
      method: 'POST', headers: { authorization: 'Bearer ' + options.lease,
        'content-type': 'application/json', 'content-length': body.length }, timeout: 2000
    }, response => {
      response.resume();
      response.once('end', () => response.statusCode === 204 ? resolve() : reject(new Error('lease-ended')));
    });
    request.once('timeout', () => request.destroy(new Error('lease-timeout')));
    request.once('error', reject);
    request.end(body);
  });
}

function projectModel(model) {
  if (!model || typeof model.id !== 'string' || !model.id || model.disabled === true) return null;
  const row = { id: model.id, name: typeof model.name === 'string' ? model.name : model.id };
  const positive = value => Number.isSafeInteger(value) && value > 0 ? value : undefined;
  row.contextWindow = positive(model.contextWindow);
  row.maxOutputTokens = positive(model.maxOutputTokens);
  if (typeof model.supportsToolCall === 'boolean') row.supportsTools = model.supportsToolCall;
  if (typeof model.supportsImages === 'boolean') row.supportsImages = model.supportsImages;
  if (typeof model.supportsReasoning === 'boolean') row.supportsThinking = model.supportsReasoning;
  // Unknown vendor-specific reasoning objects are deliberately not guessed.
  return row;
}

function identity(session) {
  if (!session?.auth?.accessToken || !session.account?.uid) return null;
  return crypto.createHash('sha256').update(JSON.stringify([
    session.account.uid, session.account.enterpriseId ?? '', session.auth.domain ?? ''
  ])).digest('hex');
}

function attach(nativeService, host, options) {
  // A separate instance owns its ledger and listener. Closing Hub must not
  // interrupt WorkBuddy's own skills or computer-use sessions.
  const owned = new nativeService.constructor(nativeService.deps);
  const sessionId = 'tiancai-hub-' + options.bridgeId;
  let stopped = false, publishing = false, pending = false, timer, subscription;
  let currentIdentity;
  const close = async () => {
    if (stopped) return;
    stopped = true;
    clearInterval(timer);
    subscription?.unsubscribe?.();
    await owned.close();
  };
  const publish = async () => {
    if (stopped) return;
    if (publishing) { pending = true; return; }
    publishing = true;
    try {
      owned.syncIdentity();
      const session = host.authenticationManager.currentSessionSubject.getValue();
      const accountId = identity(session);
      if (currentIdentity !== accountId) {
        owned.releaseSession(sessionId);
        currentIdentity = accountId;
      }
      const snapshot = accountId ? host.coordinator?.getConfirmedRemoteModelSnapshot?.() : null;
      const models = snapshot?.freshness === 'fresh' && Array.isArray(snapshot.configuration?.models)
        ? snapshot.configuration.models.map(projectModel).filter(Boolean) : [];
      let reference;
      if (accountId && models.length) {
        const env = await owned.resolveEnv(sessionId);
        if (env.WORKBUDDY_VLM_CHAT_URL && env.WORKBUDDY_VLM_AUTH_TOKEN && env.WORKBUDDY_VLM_CONTEXT) {
          reference = { url: env.WORKBUDDY_VLM_CHAT_URL, token: env.WORKBUDDY_VLM_AUTH_TOKEN,
            context: env.WORKBUDDY_VLM_CONTEXT };
        }
      } else {
        owned.releaseSession(sessionId);
      }
      // Re-check after async acquisition: an account switch may revoke it.
      if (identity(host.authenticationManager.currentSessionSubject.getValue()) !== accountId) {
        pending = true;
        return;
      }
      const display = session?.account?.displayName || session?.account?.nickname || session?.account?.name;
      await notify(options, { loggedIn: !!accountId, accountId,
        accountName: typeof display === 'string' ? display.slice(0, 128) : undefined,
        models, reference,
        state: !accountId ? 'login-required' : !models.length ? 'catalog-pending' : !reference ? 'reference-unavailable' : 'ready' });
    } catch {
      // A stopped Hub/lease cannot be used to keep a native session alive.
      await close();
    } finally {
      publishing = false;
      if (pending && !stopped) { pending = false; queueMicrotask(publish); }
    }
  };
  subscription = host.authenticationManager.currentSessionSubject.subscribe(() => {
    owned.syncIdentity(); // Revoke before the next Hub heartbeat.
    void publish();
  });
  timer = setInterval(publish, HEARTBEAT_MS);
  timer.unref?.();
  void publish();
  return { close, publish };
}

function install(options) {
  if (!options) return;
  let attached = false;
  globalThis[ATTACH] = (service, host) => {
    if (attached) return;
    attached = true;
    // Do not propagate the Hub lease or preload into host-executed tools.
    for (const key of Object.keys(process.env)) if (key.startsWith(ENV)) delete process.env[key];
    if (options.originalNodeOptions) process.env.NODE_OPTIONS = options.originalNodeOptions;
    else delete process.env.NODE_OPTIONS;
    attach(service, host, options);
  };
  const previous = Module.prototype._compile;
  Module.prototype._compile = function(source, filename) {
    if (path.resolve(filename).toLowerCase() === path.resolve(options.server).toLowerCase()) {
      Module.prototype._compile = previous;
      const offset = source.indexOf(ANCHOR);
      if (offset < 0 || source.indexOf(ANCHOR, offset + ANCHOR.length) >= 0) {
        void notify(options, { loggedIn: false, models: [], state: 'unsupported-host' }).catch(() => {});
      } else {
        source = source.slice(0, offset + ANCHOR.length)
          + 'globalThis[Symbol.for("tiancaispace.workbuddy.builtin-share")](ye,e);'
          + source.slice(offset + ANCHOR.length);
      }
    }
    return previous.call(this, source, filename);
  };
}

module.exports = { install, attach, projectModel, identity, ANCHOR };
try { install(optionsFromEnv(process.env)); } catch { /* Ordinary launch remains available. */ }
