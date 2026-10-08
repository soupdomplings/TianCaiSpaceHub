#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { createServer } from 'node:http';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { StringDecoder } from 'node:string_decoder';
import { pathToFileURL } from 'node:url';

const DEFAULT_AUTH_HEADER = 'authorization-ticket-token';
const DEFAULT_TIMEOUT_MS = 300_000;
const LOOPBACK_HOSTS = new Set(['127.0.0.1', '::1', '[::1]', 'localhost']);
const FORWARDED_REQUEST_HEADERS = new Set([
  'accept',
  'content-type',
  'last-event-id',
  'mcp-protocol-version',
  'mcp-session-id',
]);
const FORWARDED_RESPONSE_HEADERS = new Set([
  'allow',
  'cache-control',
  'content-type',
  'location',
  'mcp-protocol-version',
  'mcp-session-id',
  'retry-after',
]);

export class ConnectorConfigurationError extends Error {}

export class RemoteRequestError extends Error {
  constructor(message, status = null) {
    super(message);
    this.status = status;
  }
}

function requireEnvironment(environment, name, { trim = true } = {}) {
  const raw = environment[name];
  if (typeof raw !== 'string' || raw.trim().length === 0) {
    throw new ConnectorConfigurationError(
      `Missing required environment variable: ${name}`,
    );
  }
  return trim ? raw.trim() : raw;
}

function parsePositiveInteger(value, name, defaultValue) {
  if (value === undefined || value === null || String(value).trim() === '') {
    return defaultValue;
  }
  const text = String(value).trim();
  if (!/^\d+$/.test(text)) {
    throw new ConnectorConfigurationError(`${name} must be a positive integer`);
  }
  const parsed = Number(text);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) {
    throw new ConnectorConfigurationError(`${name} must be a positive integer`);
  }
  return parsed;
}

function parseHttpUrl(value, name) {
  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    throw new ConnectorConfigurationError(`${name} must be an absolute HTTP URL`);
  }
  if (!['http:', 'https:'].includes(parsed.protocol)) {
    throw new ConnectorConfigurationError(`${name} must use HTTP or HTTPS`);
  }
  if (parsed.username || parsed.password || parsed.hash) {
    throw new ConnectorConfigurationError(
      `${name} must not include user info or a URL fragment`,
    );
  }
  return parsed;
}

function parseHeaderName(value) {
  const name = value.trim().toLowerCase();
  if (!/^[!#$%&'*+.^_`|~0-9a-z-]+$/.test(name)) {
    throw new ConnectorConfigurationError(
      'NVWA_MCP_TARGET_AUTH_HEADER is not a valid HTTP header name',
    );
  }
  if (['host', 'content-length', 'connection', 'transfer-encoding'].includes(name)) {
    throw new ConnectorConfigurationError(
      'NVWA_MCP_TARGET_AUTH_HEADER uses a prohibited HTTP header name',
    );
  }
  return name;
}

export function loadConfiguration(environment = process.env) {
  const certificationBaseUrl = parseHttpUrl(
    requireEnvironment(environment, 'NVWA_CERTIFICATION_BASE_URL'),
    'NVWA_CERTIFICATION_BASE_URL',
  );
  const targetUrl = parseHttpUrl(
    requireEnvironment(environment, 'NVWA_MCP_TARGET_URL'),
    'NVWA_MCP_TARGET_URL',
  );
  return Object.freeze({
    certificationBaseUrl,
    clientId: requireEnvironment(environment, 'NVWA_CERTIFICATION_CLIENT_ID'),
    clientSecret: requireEnvironment(
      environment,
      'NVWA_CERTIFICATION_CLIENT_SECRET',
      { trim: false },
    ),
    username: requireEnvironment(environment, 'NVWA_CERTIFICATION_USERNAME'),
    targetUrl,
    targetAuthHeader: parseHeaderName(
      environment.NVWA_MCP_TARGET_AUTH_HEADER ?? DEFAULT_AUTH_HEADER,
    ),
    timeoutMs: parsePositiveInteger(
      environment.NVWA_MCP_CONNECTOR_TIMEOUT_MS,
      'NVWA_MCP_CONNECTOR_TIMEOUT_MS',
      DEFAULT_TIMEOUT_MS,
    ),
  });
}

function base64Utf8(value) {
  return Buffer.from(value, 'utf8').toString('base64');
}

function joinBaseUrl(baseUrl, path) {
  return `${baseUrl.href.replace(/\/+$/, '')}${path}`;
}

function parseJsonResponse(text, context) {
  try {
    return JSON.parse(text);
  } catch {
    throw new RemoteRequestError(`${context} returned invalid JSON`);
  }
}

function findObjectWithProperty(root, property) {
  const queue = [root];
  const wrappers = ['data', 'result', 'content', 'payload', 'body', 'value'];
  let inspected = 0;
  while (queue.length > 0 && inspected < 32) {
    const current = queue.shift();
    inspected += 1;
    if (current === null || current === undefined) {
      continue;
    }
    if (typeof current === 'object' && Object.hasOwn(current, property)) {
      return current;
    }
    if (typeof current === 'string') {
      try {
        const nested = JSON.parse(current);
        if (nested !== current) {
          queue.push(nested);
        }
      } catch {
        // A plain string is not a JSON wrapper.
      }
      continue;
    }
    if (Array.isArray(current)) {
      queue.push(...current);
      continue;
    }
    if (typeof current === 'object') {
      for (const wrapper of wrappers) {
        if (Object.hasOwn(current, wrapper)) {
          queue.push(current[wrapper]);
        }
      }
    }
  }
  throw new RemoteRequestError(`${property} was not present in the response`);
}

async function fetchWithTimeout(fetchImplementation, url, options, timeoutMs) {
  const controller = new AbortController();
  const abortFromCaller = () => controller.abort(options.signal?.reason);
  if (options.signal?.aborted) {
    abortFromCaller();
  } else {
    options.signal?.addEventListener('abort', abortFromCaller, { once: true });
  }
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  timer.unref?.();
  try {
    return await fetchImplementation(url, {
      ...options,
      signal: controller.signal,
    });
  } catch (error) {
    if (controller.signal.aborted && !options.signal?.aborted) {
      throw new RemoteRequestError('Remote request timed out');
    }
    throw error;
  } finally {
    clearTimeout(timer);
    options.signal?.removeEventListener('abort', abortFromCaller);
  }
}

export class NvwaTokenManager {
  constructor(configuration, { fetchImplementation = fetch } = {}) {
    this.configuration = configuration;
    this.fetchImplementation = fetchImplementation;
    this.cachedToken = null;
    this.refreshPromise = null;
  }

  async getToken() {
    if (this.cachedToken !== null) {
      return this.cachedToken;
    }
    if (this.refreshPromise === null) {
      this.refreshPromise = this.#requestToken().finally(() => {
        this.refreshPromise = null;
      });
    }
    return await this.refreshPromise;
  }

  invalidate(token) {
    if (this.cachedToken === token) {
      this.cachedToken = null;
    }
  }

  async #requestToken() {
    const {
      certificationBaseUrl,
      clientId,
      clientSecret,
      username,
      timeoutMs,
    } = this.configuration;
    const timestamp = Date.now();
    const identity = `${clientId}:${timestamp}:${username}`;
    const digest = createHash('md5')
      .update(`${identity}:${clientSecret}`, 'utf8')
      .digest('hex')
      .toUpperCase();
    const ticketAuthorization = base64Utf8(`${identity}:${digest}`);
    const ticketResponse = await fetchWithTimeout(
      this.fetchImplementation,
      joinBaseUrl(
        certificationBaseUrl,
        '/nvwa-certification/v1/ticket/apply',
      ),
      {
        method: 'GET',
        headers: { 'authorization-cer-client': ticketAuthorization },
        redirect: 'manual',
      },
      timeoutMs,
    );
    if (!ticketResponse.ok) {
      await ticketResponse.body?.cancel();
      throw new RemoteRequestError(
        `Nvwa ticket apply returned HTTP ${ticketResponse.status}`,
        ticketResponse.status,
      );
    }
    const ticketPayload = parseJsonResponse(
      await ticketResponse.text(),
      'Nvwa ticket apply',
    );
    const ticketContainer = findObjectWithProperty(ticketPayload, 'data');
    const ticketId = ticketContainer.data?.id;
    if (typeof ticketId !== 'string' || ticketId.trim().length === 0) {
      throw new RemoteRequestError('Nvwa ticket apply returned no data.id');
    }

    const tokenResponse = await fetchWithTimeout(
      this.fetchImplementation,
      joinBaseUrl(
        certificationBaseUrl,
        `/nvwa-ticket/v1/ticket/${encodeURIComponent(ticketId)}`,
      ),
      {
        method: 'POST',
        headers: {
          'authorization-client-basic': base64Utf8(
            `${clientId}:${clientSecret}`,
          ),
        },
        redirect: 'manual',
      },
      timeoutMs,
    );
    if (!tokenResponse.ok) {
      await tokenResponse.body?.cancel();
      throw new RemoteRequestError(
        `Nvwa token exchange returned HTTP ${tokenResponse.status}`,
        tokenResponse.status,
      );
    }
    const tokenPayload = parseJsonResponse(
      await tokenResponse.text(),
      'Nvwa token exchange',
    );
    const tokenContainer = findObjectWithProperty(tokenPayload, 'id');
    const token = tokenContainer.id;
    if (typeof token !== 'string' || token.trim().length === 0) {
      throw new RemoteRequestError('Nvwa token exchange returned no id');
    }
    this.cachedToken = token;
    return token;
  }
}

function normalizeInputHeaders(inputHeaders) {
  const normalized = new Headers();
  if (inputHeaders instanceof Headers) {
    for (const [name, value] of inputHeaders.entries()) {
      normalized.set(name, value);
    }
    return normalized;
  }
  for (const [name, value] of Object.entries(inputHeaders ?? {})) {
    if (value === undefined || value === null) {
      continue;
    }
    normalized.set(name, Array.isArray(value) ? value.join(', ') : String(value));
  }
  return normalized;
}

export class AuthenticatedMcpRemote {
  constructor(
    configuration,
    { fetchImplementation = fetch, tokenManager = null } = {},
  ) {
    this.configuration = configuration;
    this.fetchImplementation = fetchImplementation;
    this.tokenManager =
      tokenManager ?? new NvwaTokenManager(configuration, { fetchImplementation });
  }

  async request(method, inputHeaders = {}, body = null, signal = undefined) {
    const originalHeaders = normalizeInputHeaders(inputHeaders);
    let token = await this.tokenManager.getToken();
    let response = await this.#send(
      method,
      originalHeaders,
      body,
      token,
      signal,
    );
    if (response.status === 401) {
      await response.body?.cancel();
      this.tokenManager.invalidate(token);
      token = await this.tokenManager.getToken();
      response = await this.#send(
        method,
        originalHeaders,
        body,
        token,
        signal,
      );
    }
    return response;
  }

  async #send(method, originalHeaders, body, token, signal) {
    const headers = new Headers();
    for (const [name, value] of originalHeaders.entries()) {
      if (FORWARDED_REQUEST_HEADERS.has(name.toLowerCase())) {
        headers.set(name, value);
      }
    }
    headers.set(this.configuration.targetAuthHeader, token);
    // A native connector has no browser origin; the server validates Origin when present.
    if (!headers.has('accept')) {
      headers.set('accept', 'application/json, text/event-stream');
    }
    return await fetchWithTimeout(
      this.fetchImplementation,
      this.configuration.targetUrl,
      {
        method,
        headers,
        body,
        signal,
        redirect: 'manual',
      },
      this.configuration.timeoutMs,
    );
  }
}

function responseHeaders(response) {
  const result = {};
  for (const [name, value] of response.headers.entries()) {
    if (FORWARDED_RESPONSE_HEADERS.has(name.toLowerCase())) {
      result[name] = value;
    }
  }
  return result;
}

async function readNodeRequest(request) {
  const chunks = [];
  for await (const chunk of request) {
    chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
  }
  return chunks.length === 0 ? null : Buffer.concat(chunks);
}

function parseListenAddress(value) {
  let parsed;
  try {
    parsed = new URL(`http://${value}`);
  } catch {
    throw new ConnectorConfigurationError(
      '--listen must use the form 127.0.0.1:<port>',
    );
  }
  if (!LOOPBACK_HOSTS.has(parsed.hostname)) {
    throw new ConnectorConfigurationError(
      '--listen only permits 127.0.0.1, ::1, or localhost',
    );
  }
  const port = Number.parseInt(parsed.port, 10);
  if (!Number.isInteger(port) || port < 0 || port > 65_535) {
    throw new ConnectorConfigurationError('--listen contains an invalid port');
  }
  return {
    host: parsed.hostname.replace(/^\[(.*)]$/, '$1'),
    port,
  };
}

export async function startHttpConnector({
  configuration,
  listen,
  fetchImplementation = fetch,
  logger = console,
}) {
  const remote = new AuthenticatedMcpRemote(configuration, {
    fetchImplementation,
  });
  const server = createServer(async (request, response) => {
    try {
      const requestUrl = new URL(request.url ?? '/', 'http://127.0.0.1');
      if (requestUrl.pathname === '/healthz') {
        response.writeHead(200, { 'content-type': 'application/json; charset=utf-8' });
        response.end('{"status":"UP"}');
        return;
      }
      if (requestUrl.pathname !== '/mcp') {
        response.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' });
        response.end('Not found');
        return;
      }
      if (!['GET', 'POST', 'DELETE'].includes(request.method ?? '')) {
        response.writeHead(405, {
          allow: 'GET, POST, DELETE',
          'content-type': 'text/plain; charset=utf-8',
        });
        response.end('Method not allowed');
        return;
      }
      const body = await readNodeRequest(request);
      const remoteResponse = await remote.request(
        request.method,
        request.headers,
        body,
      );
      response.writeHead(remoteResponse.status, responseHeaders(remoteResponse));
      if (remoteResponse.body === null) {
        response.end();
        return;
      }
      await pipeline(Readable.fromWeb(remoteResponse.body), response);
    } catch (error) {
      logger.error(`Connector request failed: ${safeErrorMessage(error)}`);
      if (response.headersSent) {
        response.destroy();
        return;
      }
      response.writeHead(502, {
        'content-type': 'application/json; charset=utf-8',
      });
      response.end('{"error":"NVWA MCP connector request failed"}');
    }
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(listen.port, listen.host, () => {
      server.off('error', reject);
      resolve();
    });
  });
  return server;
}

function parseSseBlock(block) {
  const dataLines = [];
  for (const line of block.split(/\r\n|\r|\n/)) {
    if (line.startsWith('data:')) {
      dataLines.push(line.slice(5).replace(/^ /, ''));
    }
  }
  if (dataLines.length === 0) {
    return null;
  }
  try {
    return JSON.parse(dataLines.join('\n'));
  } catch {
    throw new RemoteRequestError('Remote MCP returned invalid SSE JSON');
  }
}

export async function* responseMessages(response) {
  if (response.status === 202 || response.status === 204) {
    await response.body?.cancel();
    return;
  }
  const contentType = response.headers.get('content-type') ?? '';
  if (contentType.toLowerCase().startsWith('text/event-stream')) {
    if (response.body === null) {
      return;
    }
    const decoder = new TextDecoder('utf-8', { fatal: true });
    let buffered = '';
    for await (const chunk of response.body) {
      buffered += decoder.decode(chunk, { stream: true });
      let separator = /\r\n\r\n|\n\n|\r\r/.exec(buffered);
      while (separator !== null) {
        const block = buffered.slice(0, separator.index);
        buffered = buffered.slice(separator.index + separator[0].length);
        const message = parseSseBlock(block);
        if (message !== null) {
          yield message;
        }
        separator = /\r\n\r\n|\n\n|\r\r/.exec(buffered);
      }
    }
    buffered += decoder.decode();
    if (buffered.trim().length > 0) {
      const message = parseSseBlock(buffered);
      if (message !== null) {
        yield message;
      }
    }
    return;
  }
  const body = await response.text();
  if (body.trim().length === 0) {
    return;
  }
  try {
    const parsed = JSON.parse(body);
    if (Array.isArray(parsed)) {
      yield* parsed;
    } else {
      yield parsed;
    }
  } catch {
    throw new RemoteRequestError('Remote MCP returned invalid JSON');
  }
}

function jsonRpcError(id, message) {
  return {
    jsonrpc: '2.0',
    id: id ?? null,
    error: { code: -32098, message },
  };
}

function safeErrorMessage(error) {
  if (error instanceof ConnectorConfigurationError || error instanceof RemoteRequestError) {
    return error.message;
  }
  if (error?.name === 'AbortError') {
    return 'Remote request was aborted';
  }
  return 'Unexpected connector failure';
}

class LineWriter {
  constructor(output) {
    this.output = output;
    this.pending = Promise.resolve();
  }

  write(message) {
    const line = `${JSON.stringify(message)}\n`;
    this.pending = this.pending.then(
      () =>
        new Promise((resolve, reject) => {
          this.output.write(line, (error) => {
            if (error) {
              reject(error);
            } else {
              resolve();
            }
          });
        }),
    );
    return this.pending;
  }
}

export async function runStdioConnector({
  configuration,
  input = process.stdin,
  output = process.stdout,
  errorOutput = process.stderr,
  fetchImplementation = fetch,
}) {
  const remote = new AuthenticatedMcpRemote(configuration, {
    fetchImplementation,
  });
  const writer = new LineWriter(output);
  const pending = new Set();
  let sessionId = null;
  let protocolVersion = null;
  let ready = false;
  let startupChain = Promise.resolve();
  let buffered = '';
  const inputDecoder = new StringDecoder('utf8');

  const processMessage = async (message) => {
    const headers = {
      accept: 'application/json, text/event-stream',
      'content-type': 'application/json',
    };
    if (sessionId !== null) {
      headers['mcp-session-id'] = sessionId;
    }
    if (protocolVersion !== null) {
      headers['mcp-protocol-version'] = protocolVersion;
    }
    const response = await remote.request(
      'POST',
      headers,
      Buffer.from(JSON.stringify(message), 'utf8'),
    );
    if (response.status < 200 || response.status >= 300) {
      await response.body?.cancel();
      throw new RemoteRequestError(
        `Remote MCP returned HTTP ${response.status}`,
        response.status,
      );
    }
    const responseSessionId = response.headers.get('mcp-session-id');
    if (responseSessionId) {
      sessionId = responseSessionId;
    }
    for await (const responseMessage of responseMessages(response)) {
      const negotiated = responseMessage?.result?.protocolVersion;
      if (typeof negotiated === 'string' && negotiated.length > 0) {
        protocolVersion = negotiated;
      }
      await writer.write(responseMessage);
    }
  };

  const track = (task, message) => {
    const handled = task.catch(async (error) => {
      const safeMessage = safeErrorMessage(error);
      errorOutput.write(`Connector request failed: ${safeMessage}\n`);
      if (Object.hasOwn(message, 'id')) {
        await writer.write(jsonRpcError(message.id, 'NVWA MCP connector request failed'));
      }
    });
    pending.add(handled);
    handled.finally(() => pending.delete(handled));
    return handled;
  };

  const schedule = (message) => {
    let task;
    if (ready) {
      task = processMessage(message);
    } else {
      task = startupChain.then(() => processMessage(message));
      startupChain = task.catch(() => undefined);
      if (message.method === 'notifications/initialized') {
        task.then(() => {
          ready = true;
        });
      }
    }
    track(task, message);
  };

  const consumeLine = (line) => {
    const trimmed = line.trim().replace(/^\uFEFF/, '');
    if (trimmed.length === 0) {
      return;
    }
    let message;
    try {
      message = JSON.parse(trimmed);
    } catch {
      void writer.write(jsonRpcError(null, 'Invalid JSON-RPC message'));
      return;
    }
    if (message === null || typeof message !== 'object' || Array.isArray(message)) {
      void writer.write(jsonRpcError(null, 'Invalid JSON-RPC message'));
      return;
    }
    schedule(message);
  };

  for await (const chunk of input) {
    buffered += inputDecoder.write(
      Buffer.isBuffer(chunk) ? chunk : Buffer.from(String(chunk), 'utf8'),
    );
    let newlineIndex;
    while ((newlineIndex = buffered.indexOf('\n')) >= 0) {
      const line = buffered.slice(0, newlineIndex);
      buffered = buffered.slice(newlineIndex + 1);
      consumeLine(line);
    }
  }
  buffered += inputDecoder.end();
  if (buffered.trim().length > 0) {
    consumeLine(buffered);
  }
  await Promise.allSettled([...pending]);
  await startupChain;
  await writer.pending;
}

function usage() {
  return `NVWA MCP authentication connector

Default stdio mode:
  node tools/nvwa-mcp-auth-connector.mjs

Compatibility HTTP mode:
  node tools/nvwa-mcp-auth-connector.mjs --listen 127.0.0.1:<port>

Required environment variables:
  NVWA_CERTIFICATION_BASE_URL
  NVWA_CERTIFICATION_CLIENT_ID
  NVWA_CERTIFICATION_CLIENT_SECRET
  NVWA_CERTIFICATION_USERNAME
  NVWA_MCP_TARGET_URL

Optional environment variables:
  NVWA_MCP_TARGET_AUTH_HEADER       default: authorization-ticket-token
  NVWA_MCP_CONNECTOR_TIMEOUT_MS     default: 300000
`;
}

export function parseArguments(argumentsList) {
  if (argumentsList.includes('--help') || argumentsList.includes('-h')) {
    return { help: true, mode: 'stdio', listen: null };
  }
  if (argumentsList.length === 0) {
    return { help: false, mode: 'stdio', listen: null };
  }
  let listenValue = null;
  for (let index = 0; index < argumentsList.length; index += 1) {
    const argument = argumentsList[index];
    if (argument === '--listen') {
      if (listenValue !== null) {
        throw new ConnectorConfigurationError('--listen may only be specified once');
      }
      listenValue = argumentsList[index + 1];
      index += 1;
      continue;
    }
    if (argument.startsWith('--listen=')) {
      if (listenValue !== null) {
        throw new ConnectorConfigurationError('--listen may only be specified once');
      }
      listenValue = argument.slice('--listen='.length);
      continue;
    }
    throw new ConnectorConfigurationError(`Unknown argument: ${argument}`);
  }
  if (!listenValue) {
    throw new ConnectorConfigurationError('--listen requires an address and port');
  }
  return {
    help: false,
    mode: 'http',
    listen: parseListenAddress(listenValue),
  };
}

async function main() {
  const argumentsConfiguration = parseArguments(process.argv.slice(2));
  if (argumentsConfiguration.help) {
    process.stdout.write(usage());
    return;
  }
  const configuration = loadConfiguration();
  if (argumentsConfiguration.mode === 'stdio') {
    await runStdioConnector({ configuration });
    return;
  }
  const server = await startHttpConnector({
    configuration,
    listen: argumentsConfiguration.listen,
  });
  const address = server.address();
  const host = typeof address === 'object' && address ? address.address : '127.0.0.1';
  const port = typeof address === 'object' && address ? address.port : '';
  process.stderr.write(
    `NVWA MCP authentication connector listening on http://${host}:${port}/mcp\n`,
  );
  const stop = () => server.close(() => process.exit(0));
  process.once('SIGINT', stop);
  process.once('SIGTERM', stop);
}

const invokedPath = process.argv[1]
  ? pathToFileURL(process.argv[1]).href
  : null;
if (invokedPath === import.meta.url) {
  main().catch((error) => {
    process.stderr.write(`NVWA MCP connector failed: ${safeErrorMessage(error)}\n`);
    process.exitCode = 1;
  });
}
