import { RemoteApi } from './api';
import type { RemoteFile } from './types';

export interface SessionFiles {
  clientId: string;
  sessionId: string;
  handles: ReadonlyMap<string, RemoteFile>;
  download: (file: RemoteFile) => void;
}

type LinkResult = { kind: 'file'; file: RemoteFile } | { kind: 'external'; url: string } | { kind: 'unavailable' };

export function resolveMessageLink(raw: string | undefined, session?: SessionFiles): LinkResult {
  const unavailable = { kind: 'unavailable' } as const;
  if (!raw || raw.includes('\\')) return unavailable;
  const absolute = /^https?:\/\//i.test(raw);
  if (!absolute && !raw.startsWith('/api/remote/v1/')) {
    return /^mailto:[^\s@]+@[^\s@]+$/i.test(raw) ? { kind:'external', url:raw } : unavailable;
  }
  try {
    const url = new URL(raw, location.href);
    if (!url.pathname.startsWith('/api/remote/v1/')) {
      return absolute && url.origin !== location.origin ? { kind: 'external', url:url.href } : unavailable;
    }
    if (!session || url.origin !== location.origin) return unavailable;
    const match = /^\/api\/remote\/v1\/clients\/([^/]+)\/sessions\/([^/]+)\/files\/([^/]+)$/.exec(url.pathname);
    if (!match || decodeURIComponent(match[1]) !== session.clientId || decodeURIComponent(match[2]) !== session.sessionId) return unavailable;
    const file = session.handles.get(decodeURIComponent(match[3]));
    return file ? { kind:'file', file } : unavailable;
  } catch { return unavailable; }
}

export async function saveRemoteFile(api: RemoteApi, clientId: string, sessionId: string, file: RemoteFile, signal?: AbortSignal) {
  const blob = await api.readFile(clientId, sessionId, file.id, true, signal);
  if (signal?.aborted) return;
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  try {
    anchor.href = url;
    anchor.download = file.name;
    document.body.append(anchor);
    anchor.click();
  } finally {
    anchor.remove();
    window.setTimeout(() => URL.revokeObjectURL(url), 30000);
  }
}
