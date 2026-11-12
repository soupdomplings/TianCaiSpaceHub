'use strict';

// Loaded only by the user-requested WorkBuddy launch. Explicit standalone
// authorization exports a minimal current-session projection to Hub's leased
// loopback callback; ordinary launch/legacy sharing never exports credentials.
const Module = require('node:module');
const path = require('node:path');
const http = require('node:http');
const crypto = require('node:crypto');
const { projectCatalog, projectModel } = require('./model-catalog.cjs');
const ATTACH = Symbol.for('tiancaispace.workbuddy.builtin-share');
const START = Symbol.for('tiancaispace.workbuddy.builtin-start');
const ANCHOR = 'let ye=createSkillVlmService({authenticationManager:e.authenticationManager,productManager:e.productManager,appVersion:r.appVersion,fetch:s,logger:O});';
const BOOTSTRAP_ANCHOR = 'new DaemonAppServerProcessManager({env:u,requestHandler:';
const ENV = 'TIANCAI_WORKBUDDY_';
const HEARTBEAT_MS = 1000;

function optionsFromEnv(env) {
  if (!env[ENV + 'CALLBACK'] || !env[ENV + 'LEASE'] || !env[ENV + 'SERVER']) return null;
  const callback = new URL(env[ENV + 'CALLBACK']);
  if (callback.protocol !== 'http:' || callback.hostname !== '127.0.0.1' || callback.pathname !== '/') return null;
  return { callback, lease: env[ENV + 'LEASE'], server: env[ENV + 'SERVER'],
    bridgeId: env[ENV + 'BRIDGE_ID'], role: 'daemon', bootstrap: __filename,
    captureAuthorization: env[ENV + 'CAPTURE_AUTHORIZATION'] === '1' };
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

function identity(session) {
  if (!session?.auth?.accessToken || !session.account?.uid) return null;
  return crypto.createHash('sha256').update(JSON.stringify([
    session.account.uid, session.account.enterpriseId ?? '', session.auth.domain ?? ''
  ])).digest('hex');
}

function projectAuthorization(session, host, nativeService) {
  if (!identity(session)) return null;
  const text = value => typeof value === 'string' && value.trim() ? value : undefined;
  const timestamp = value => Number.isSafeInteger(value) && value >= 0 ? value : undefined;
  const accountValue = value => text(value) ?? (Number.isSafeInteger(value) ? value : undefined);
  try {
    const uid = accountValue(session.account.uid);
    const enterpriseId = accountValue(session.account.enterpriseId);
    if (uid === undefined || (session.account.enterpriseId != null && enterpriseId === undefined)) return null;
    const configuration = host.productManager?.getCurrentConfiguration?.();
    const authentication = configuration?.authentication;
    const gatewayEndpoint = text(host.productManager?.getEndpoint?.());
    const authEndpoint = text(authentication?.endpoint) || gatewayEndpoint;
    const authPrefixPath = authentication?.attributes?.prefixPath ?? '';
    const authType = text(authentication?.type);
    const appVersion = text(nativeService.deps?.getRequestHeaders?.()?.['X-IDE-Version']);
    if (!gatewayEndpoint || !authEndpoint || typeof authPrefixPath !== 'string' || !authType || !appVersion) return null;
    return {
      account: { uid, enterpriseId, type: text(session.account.type) },
      auth: { accessToken: session.auth.accessToken, refreshToken: text(session.auth.refreshToken),
        domain: text(session.auth.domain), tokenType: text(session.auth.tokenType),
        expiresAt: timestamp(session.auth.expiresAt), refreshExpiresAt: timestamp(session.auth.refreshExpiresAt) },
      gatewayEndpoint, authEndpoint, authPrefixPath, authType, appVersion,
      cliVersion: text(configuration.platformVersion), deploymentType: text(configuration.deploymentType)
    };
  } catch { return null; }
}

function attach(nativeService, host, options) {
  // A separate instance owns its ledger and listener. Closing Hub must not
  // interrupt WorkBuddy's own skills or computer-use sessions.
  const owned = new nativeService.constructor(nativeService.deps);
  const sessionId = 'tiancai-hub-' + options.bridgeId;
  const captureAuthorization = options.captureAuthorization === true;
  let stopped = false, publishing = false, pending = false, timer, subscription;
  let currentIdentity, projectedAuthorization, authorizationAuthority;
  let authorizationEpoch = 0, authorizationQueue = Promise.resolve();
  const close = async () => {
    if (stopped) return;
    stopped = true;
    clearInterval(timer);
    subscription?.unsubscribe?.();
    await owned.close();
  };
  const publicSnapshot = session => {
    const accountId = identity(session);
    const snapshot = accountId ? host.coordinator?.getConfirmedRemoteModelSnapshot?.() : null;
    const display = session?.account?.displayName || session?.account?.nickname || session?.account?.name;
    return { loggedIn: !!accountId, accountId,
      accountName: typeof display === 'string' ? display.slice(0, 128) : undefined,
      models: projectCatalog(snapshot) };
  };
  const queueAuthorization = session => {
    if (!captureAuthorization) return authorizationQueue;
    const authorization = projectAuthorization(session, host, nativeService);
    const serialized = JSON.stringify(authorization);
    if (serialized === projectedAuthorization) return authorizationQueue;
    const authority = authorization ? JSON.stringify([identity(session), authorization.account.type,
      authorization.gatewayEndpoint, authorization.authEndpoint, authorization.authPrefixPath,
      authorization.authType]) : null;
    const enqueue = value => {
      const epoch = ++authorizationEpoch;
      const metadata = value ? publicSnapshot(session) : { loggedIn: false, accountId: null, models: [] };
      const report = { ...metadata, authOnly: true, authorizationEpoch: epoch, authorization: value };
      authorizationQueue = authorizationQueue.then(async () => {
        if (stopped || (value && epoch !== authorizationEpoch)) return;
        await notify(options, report);
      }).catch(close);
    };
    // Revoke the former authority before granting another account/organization
    // or endpoint. Token rotation within the same authority replaces it once.
    if (authorizationAuthority && authority !== authorizationAuthority && authorization) enqueue(null);
    projectedAuthorization = serialized;
    authorizationAuthority = authority;
    enqueue(authorization);
    return authorizationQueue;
  };
  const publish = async () => {
    if (stopped) return;
    if (publishing) { pending = true; return; }
    publishing = true;
    try {
      owned.syncIdentity();
      const session = host.authenticationManager.currentSessionSubject.getValue();
      const accountId = identity(session);
      await queueAuthorization(session);
      if (stopped) return;
      const epoch = authorizationEpoch;
      if (currentIdentity !== accountId) {
        owned.releaseSession(sessionId);
        currentIdentity = accountId;
      }
      const metadata = publicSnapshot(session);
      const models = metadata.models;
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
      if (identity(host.authenticationManager.currentSessionSubject.getValue()) !== accountId
          || (captureAuthorization && epoch !== authorizationEpoch)) {
        pending = true;
        return;
      }
      const unavailable = captureAuthorization && accountId && projectedAuthorization === 'null';
      await notify(options, { ...metadata, reference,
        ...(captureAuthorization ? { authorizationEpoch: epoch } : {}),
        ...(unavailable ? { authorizationError: '无法取得当前账户的独立分享授权，请重新授权；原 WorkBuddy 服务保持可用' } : {}),
        state: !accountId ? 'login-required' : unavailable ? 'authorization-unavailable'
          : !models.length ? 'catalog-pending' : !reference ? 'reference-unavailable' : 'ready' });
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
    // Publish revocation independently of a pending native ledger acquisition.
    // Epochs let Hub discard a late heartbeat from the former authority.
    void queueAuthorization(host.authenticationManager.currentSessionSubject.getValue());
    void publish();
  });
  timer = setInterval(publish, HEARTBEAT_MS);
  timer.unref?.();
  void publish();
  return { close, publish };
}

function install(options) {
  if (!options) return { installed: false };
  const stage = state => void notify(options, { stage: true, loggedIn: false, models: [], state }).catch(() => {});
  const samePath = (left, right) => !!right && path.resolve(left).toLowerCase() === path.resolve(right).toLowerCase();
  const main = options.role === 'main';
  if (!main) {
    // --require is supplied as a real RUN_AS_NODE CLI argument, never as a
    // packaged Electron main-process NODE_OPTIONS option. Don't inherit it.
    for (const key of Object.keys(process.env)) if (key.startsWith(ENV)) delete process.env[key];
    for (let index = process.execArgv.length - 2; index >= 0; index--) {
      if ((process.execArgv[index] === '--require' || process.execArgv[index] === '-r')
          && samePath(process.execArgv[index + 1], options.bootstrap)) process.execArgv.splice(index, 2);
    }
  } else {
    globalThis[START] = {
      argv: () => ['--require', options.bootstrap],
      env: native => ({ ...native,
        [ENV + 'CALLBACK']: String(options.callback), [ENV + 'LEASE']: options.lease,
        [ENV + 'BRIDGE_ID']: options.bridgeId, [ENV + 'SERVER']: options.server,
        [ENV + 'CAPTURE_AUTHORIZATION']: options.captureAuthorization === true ? '1' : '0' })
    };
  }
  let attached = false;
  globalThis[ATTACH] = (service, host) => {
    if (attached) return;
    attached = true;
    try { attach(service, host, options); }
    catch { stage('host-attach-failed'); }
  };
  const previous = Module.prototype._compile;
  Module.prototype._compile = function(source, filename) {
    if (main && samePath(filename, options.mainBootstrap)) {
      Module.prototype._compile = previous;
      const offset = source.indexOf(BOOTSTRAP_ANCHOR);
      if (offset < 0 || source.indexOf(BOOTSTRAP_ANCHOR, offset + BOOTSTRAP_ANCHOR.length) >= 0) {
        stage('unsupported-bootstrap');
      } else {
        source = source.slice(0, offset)
          + 'new DaemonAppServerProcessManager({processExecArgv:globalThis[Symbol.for("tiancaispace.workbuddy.builtin-start")].argv(),env:globalThis[Symbol.for("tiancaispace.workbuddy.builtin-start")].env(u),requestHandler:'
          + source.slice(offset + BOOTSTRAP_ANCHOR.length);
        stage('main-bootstrap-ready');
      }
    } else if (!main && samePath(filename, options.server)) {
      Module.prototype._compile = previous;
      const offset = source.indexOf(ANCHOR);
      if (offset < 0 || source.indexOf(ANCHOR, offset + ANCHOR.length) >= 0) {
        stage('unsupported-host');
      } else {
        source = source.slice(0, offset + ANCHOR.length)
          + 'globalThis[Symbol.for("tiancaispace.workbuddy.builtin-share")](ye,e);'
          + source.slice(offset + ANCHOR.length);
        stage('server-module-ready');
      }
    }
    return previous.call(this, source, filename);
  };
  stage(main ? 'main-loader-ready' : 'daemon-preload-ready');
  return { installed: true, role: main ? 'main' : 'daemon' };
}

module.exports = { install, attach, projectModel, projectAuthorization, identity, ANCHOR, BOOTSTRAP_ANCHOR };
try { install(optionsFromEnv(process.env)); } catch { /* Ordinary launch remains available. */ }
