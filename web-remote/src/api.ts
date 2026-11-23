import type { ApprovalDecision, DevicesResponse, HistoryResponse, NewSessionInput, ProjectsResponse, QuestionAnswers, RemoteClient, RemoteEvent, RemoteFile, RemoteSession, SessionsResponse, TurnInput } from './types';

const ROOT = '/api/remote/v1';
const segment = encodeURIComponent;

export class ApiError extends Error {
  constructor(public status: number, public code: string, message: string) {
    super(message);
    this.name = 'ApiError';
  }
}
export function accessEnded(failure: unknown): boolean {
  return failure instanceof ApiError && [401, 403, 410].includes(failure.status);
}

async function checked(response: Response): Promise<Response> {
  if (response.ok) return response;
  const text = await response.text();
  let message = text || `请求失败（HTTP ${response.status}）`;
  let code = 'request_failed';
  try {
    const value = JSON.parse(text);
    message = typeof value.error === 'string' ? value.error : value.error?.message || value.message || message;
    code = value.error?.code || code;
  } catch { /* Non-JSON failures still retain their readable response body. */ }
  throw new ApiError(response.status, code, message);
}

export async function getDevices(signal?: AbortSignal): Promise<DevicesResponse> {
  const response = await checked(await fetch(`${ROOT}/devices`, { signal, credentials: 'same-origin' }));
  return response.json();
}

/** Incremental SSE parser; preserves split UTF-8 and multi-line data fields. */
export async function readEventStream(
  body: ReadableStream<Uint8Array>,
  signal: AbortSignal,
  onEvent: (event: RemoteEvent) => void | Promise<void>,
): Promise<void> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  let data: string[] = [];
  const line = async (value: string) => {
    if (!value) {
      if (data.length) {
        const payload = data.join('\n');
        data = [];
        await onEvent(JSON.parse(payload) as RemoteEvent);
      }
      return;
    }
    if (value.startsWith('data:')) data.push(value.slice(5).replace(/^ /, ''));
  };
  const abort = () => { void reader.cancel().catch(() => {}); };
  signal.addEventListener('abort', abort, { once: true });
  try {
    while (!signal.aborted) {
      const { value, done } = await reader.read();
      buffer += decoder.decode(value, { stream: !done });
      let newline: number;
      while ((newline = buffer.indexOf('\n')) >= 0) {
        const raw = buffer.slice(0, newline).replace(/\r$/, '');
        buffer = buffer.slice(newline + 1);
        if (!signal.aborted) await line(raw);
      }
      if (done) {
        if (buffer && !signal.aborted) await line(buffer.replace(/\r$/, ''));
        if (!signal.aborted) await line('');
        return;
      }
    }
  } finally {
    signal.removeEventListener('abort', abort);
    await reader.cancel().catch(() => {});
    reader.releaseLock();
  }
}

export class RemoteApi {
  constructor(readonly deviceId: string) {}

  private async request(path: string, init: RequestInit = {}): Promise<Response> {
    const headers = new Headers(init.headers);
    headers.set('X-Hub-Target', this.deviceId);
    return checked(await fetch(`${ROOT}${path}`, { ...init, headers, credentials: 'same-origin' }));
  }

  private async json<T>(path: string, signal?: AbortSignal): Promise<T> {
    return (await this.request(path, { signal })).json();
  }

  private async post<T>(path: string, value: unknown, signal?: AbortSignal): Promise<T> {
    return (await this.request(path, {
      method: 'POST', signal, headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(value),
    })).json();
  }

  sessionPath(clientId: string, sessionId: string): string {
    return `/clients/${segment(clientId)}/sessions/${segment(sessionId)}`;
  }

  clients(signal?: AbortSignal): Promise<{ clients: RemoteClient[] }> {
    return this.json('/clients?view=summary', signal);
  }

  models(clientId: string, signal?: AbortSignal): Promise<Pick<RemoteClient, 'models'> & {modelSelection:boolean; effortSelection:boolean}> {
    return this.json(`/clients/${segment(clientId)}/models`, signal);
  }

  projects(clientId: string, cursor?: string | null, signal?: AbortSignal): Promise<ProjectsResponse> {
    const query = new URLSearchParams({ limit: '12' });
    if (cursor) query.set('cursor', cursor);
    return this.json(`/clients/${segment(clientId)}/projects?${query}`, signal);
  }

  sessions(clientId: string, cursor?: string | null, signal?: AbortSignal): Promise<SessionsResponse> {
    const query = new URLSearchParams({ limit: '16' });
    if (cursor) query.set('cursor', cursor);
    return this.json(`/clients/${segment(clientId)}/sessions?${query}`, signal);
  }

  create(clientId: string, value: NewSessionInput, signal?: AbortSignal): Promise<{ session: RemoteSession }> {
    return this.post(`/clients/${segment(clientId)}/sessions`, value, signal);
  }

  history(clientId: string, sessionId: string, cursor?: string | null, signal?: AbortSignal): Promise<HistoryResponse> {
    const query = new URLSearchParams({ limit: '3' });
    if (cursor) query.set('cursor', cursor);
    return this.json(`${this.sessionPath(clientId, sessionId)}/history?${query}`, signal);
  }

  send(clientId: string, sessionId: string, value: TurnInput): Promise<{ turnId: string; status: string }> {
    // A transport failure must not replay a turn: the target may already have accepted it.
    return this.post(`${this.sessionPath(clientId, sessionId)}/turns`, value);
  }

  stop(clientId: string, sessionId: string, turnId?: string | null): Promise<{ status: string }> {
    return this.post(`${this.sessionPath(clientId, sessionId)}/stop`, { turnId });
  }

  respond(clientId: string, sessionId: string, approvalId: string, decision: ApprovalDecision, answers?: QuestionAnswers): Promise<{ status: string }> {
    return this.post(`${this.sessionPath(clientId, sessionId)}/approvals/${segment(approvalId)}`, { decision, answers });
  }

  async upload(clientId: string, sessionId: string, file: File, signal?: AbortSignal): Promise<RemoteFile> {
    const response = await this.request(`${this.sessionPath(clientId, sessionId)}/files`, {
      method: 'POST', body: file, signal,
      headers: { 'Content-Type': file.type || 'application/octet-stream', 'X-File-Name': encodeURIComponent(file.name) },
    });
    return (await response.json()).file;
  }

  async readFile(clientId: string, sessionId: string, fileId: string, download = false, signal?: AbortSignal): Promise<Blob> {
    const response = await this.request(`${this.sessionPath(clientId, sessionId)}/files/${segment(fileId)}${download ? '?download=1' : ''}`, { signal });
    if (download && !/^attachment(?:\s*;|$)/i.test(response.headers.get('content-disposition') || '')) {
      throw new ApiError(502, 'file_response_expected', '目标电脑没有返回下载文件，请重新打开会话核对。');
    }
    return response.blob();
  }

  async removeFile(clientId: string, sessionId: string, fileId: string): Promise<void> {
    await this.request(`${this.sessionPath(clientId, sessionId)}/files/${segment(fileId)}`, { method: 'DELETE' });
  }

  async events(clientId: string, sessionId: string, after: number, signal: AbortSignal, onEvent: (event: RemoteEvent) => void | Promise<void>, onOpen?: () => void, liveActivity = false): Promise<void> {
    const response = await this.request(`${this.sessionPath(clientId, sessionId)}/events?after=${after}${liveActivity ? '&liveActivity=1' : ''}`, {
      signal, headers: { Accept: 'text/event-stream' }, cache: 'no-store',
    });
    if (!response.body) throw new ApiError(502, 'empty_stream', '事件连接没有返回内容');
    onOpen?.();
    await readEventStream(response.body, signal, onEvent);
  }
}
