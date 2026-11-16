import { useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import { ApiError, RemoteApi } from './api';
import { applyEvent, emptyChat, fromHistory, prependHistory } from './session-state';
import type { ChatState } from './session-state';
import { errorMessage, isBusy } from './types';
import type { ConnectionState, HistoryResponse, RemoteClient } from './types';

function wait(milliseconds: number, signal: AbortSignal): Promise<void> {
  return new Promise(resolve => {
    if (signal.aborted) { resolve(); return; }
    const finish = () => { clearTimeout(timer); signal.removeEventListener('abort', finish); resolve(); };
    const timer = window.setTimeout(finish, milliseconds);
    signal.addEventListener('abort', finish, { once: true });
  });
}

/** Keep already paged history when reconnecting to the newest native snapshot. */
export function mergeSnapshot(previous: ChatState, history: HistoryResponse): ChatState {
  const next = fromHistory(history);
  if (!history.items.length) return next;
  const anchor = previous.items.findIndex(item => item.id === history.items[0].id);
  if (anchor <= 0) return next;
  const prefix = previous.items.slice(0, anchor);
  return { ...next, items: [...prefix, ...next.items], nextCursor: previous.nextCursor,
    snapshotItemIds: new Set([...prefix.map(item => item.id), ...next.snapshotItemIds]) };
}

/** Reconcile an in-flight snapshot without rolling back newer SSE content. */
export function mergeLiveSnapshot(previous: ChatState, history: HistoryResponse, requestCursor: number): ChatState {
  const next = mergeSnapshot(previous, history);
  const liveItems = new Map(previous.items.filter(item => !previous.snapshotItemIds.has(item.id)).map(item => [item.id, item]));
  const ids = new Set(next.items.map(item => item.id));
  let lastNativeIndex = -1;
  previous.items.forEach((item, index) => { if (ids.has(item.id)) lastNativeIndex = index; });
  const items = next.items.map(item => liveItems.get(item.id) || item);
  // In-progress native histories can omit the newest live item until it is saved.
  previous.items.forEach((item, index) => {
    if (index > lastNativeIndex && liveItems.has(item.id) && !ids.has(item.id)) items.push(item);
  });
  const snapshotItemIds = new Set(next.snapshotItemIds);
  for (const id of liveItems.keys()) snapshotItemIds.delete(id);
  const receivedDuringRead = previous.cursor !== requestCursor;
  return { ...next, items, snapshotItemIds, cursor: previous.cursor,
    ...(receivedDuringRead ? { status: previous.status, turnId: previous.turnId, approvals: previous.approvals } : {}) };
}

class RefreshSnapshot extends Error {}

export function useSession(api: RemoteApi | null, client: RemoteClient | null, sessionId: string | null) {
  const [chat, setRenderedChat] = useState<ChatState>(emptyChat);
  const chatRef = useRef(chat);
  const setChat = useCallback<Dispatch<SetStateAction<ChatState>>>(update => {
    const next = typeof update === 'function' ? update(chatRef.current) : update;
    chatRef.current = next;
    setRenderedChat(next);
  }, []);
  const [connection, setConnection] = useState<ConnectionState>('connecting');
  const [loading, setLoading] = useState(false);
  const [loadingEarlier, setLoadingEarlier] = useState(false);
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  const identityRef = useRef('');
  const generation = useRef(0);
  const clientId = client?.id;
  const canRead = !!client?.ready && !!client?.capabilities.history;
  const refresh = useCallback(() => setRevision(value => value + 1), []);

  useEffect(() => {
    const identity = `${api?.deviceId}\0${clientId}\0${sessionId}`;
    const changed = identityRef.current !== identity;
    identityRef.current = identity;
    const currentGeneration = ++generation.current;
    const abort = new AbortController();
    let snapshotTimer: number | null = null;
    let snapshotInFlight = false;
    let snapshotDirty = false;
    let streamEpoch = 0;
    if (changed) setChat(emptyChat);
    setError('');
    setLoadingEarlier(false);
    if (!api || !clientId || !sessionId || !canRead) {
      setLoading(false);
      setConnection('offline');
      return () => abort.abort();
    }
    setLoading(changed);
    setConnection(changed ? 'connecting' : 'reconnecting');
    // Coalesce ambiguous deltas into absolute native refreshes, without
    // interrupting the established SSE subscription or replaying any turn.
    const scheduleSnapshot = () => {
      snapshotDirty = true;
      if (snapshotTimer !== null || snapshotInFlight || abort.signal.aborted) return;
      const scheduledEpoch = streamEpoch;
      snapshotTimer = window.setTimeout(() => {
        snapshotTimer = null;
        if (abort.signal.aborted || scheduledEpoch !== streamEpoch) return;
        snapshotInFlight = true;
        snapshotDirty = false;
        const requestCursor = chatRef.current.cursor;
        void api.history(clientId, sessionId, null, abort.signal).then(snapshot => {
          if (abort.signal.aborted || generation.current !== currentGeneration || scheduledEpoch !== streamEpoch) return;
          setChat(previous => mergeLiveSnapshot(previous, snapshot, requestCursor));
        }).catch(failure => {
          if (!abort.signal.aborted && scheduledEpoch === streamEpoch) setError(errorMessage(failure));
        }).finally(() => {
          snapshotInFlight = false;
          if (snapshotDirty && !abort.signal.aborted) scheduleSnapshot();
        });
      }, 250);
    };
    void (async () => {
      let failures = 0;
      while (!abort.signal.aborted) {
        streamEpoch++;
        if (snapshotTimer !== null) { clearTimeout(snapshotTimer); snapshotTimer = null; }
        snapshotDirty = false;
        try {
          const snapshot = await api.history(clientId, sessionId, null, abort.signal);
          if (abort.signal.aborted) return;
          let cursor = snapshot.cursor || 0;
          setChat(previous => mergeSnapshot(previous, snapshot));
          setLoading(false);
          setError('');
          // This is set to live only when the SSE response has actually arrived.
          await api.events(clientId, sessionId, cursor, abort.signal, event => {
            if (abort.signal.aborted || generation.current !== currentGeneration) return;
            if (event.clientId !== clientId || event.sessionId !== sessionId) return;
            if (event.type === 'reset') throw new RefreshSnapshot();
            if (event.id <= cursor) return;
            cursor = event.id;
            if (event.type === 'message.delta' && event.itemId && chatRef.current.snapshotItemIds.has(event.itemId)) scheduleSnapshot();
            if (event.type === 'turn.updated') {
              const status = String(event.data.status || chatRef.current.status);
              if (!isBusy(status) && status !== chatRef.current.status) scheduleSnapshot();
            }
            if (event.type === 'error') setError(String(event.data.message || '目标运行端返回错误'));
            setChat(previous => applyEvent(previous, event, clientId, sessionId));
          }, () => {
            if (!abort.signal.aborted) { failures = 0; setConnection('live'); }
          });
          if (abort.signal.aborted) return;
          setConnection('reconnecting');
          setError('连接已断开，正在重新连接。目标端任务会继续运行。');
        } catch (failure) {
          if (abort.signal.aborted) return;
          setLoading(false);
          if (failure instanceof RefreshSnapshot) continue;
          setError(errorMessage(failure));
          if (failure instanceof ApiError && [401, 403, 404, 409, 422, 501].includes(failure.status)) {
            setConnection('offline');
            return;
          }
          setConnection('reconnecting');
        }
        failures++;
        await wait(Math.min(1000 * 2 ** Math.min(failures - 1, 4), 10000), abort.signal);
      }
    })();
    return () => { abort.abort(); if (snapshotTimer !== null) clearTimeout(snapshotTimer); };
  }, [api, clientId, sessionId, canRead, revision, setChat]);

  const earlier = useCallback(async () => {
    if (!api || !clientId || !sessionId || !chat.nextCursor || loadingEarlier) return false;
    const current = generation.current;
    setLoadingEarlier(true);
    try {
      const page = await api.history(clientId, sessionId, chat.nextCursor);
      if (current === generation.current) { setChat(previous => prependHistory(previous, page)); return true; }
    } catch (failure) {
      if (current === generation.current) setError(errorMessage(failure));
    } finally {
      if (current === generation.current) setLoadingEarlier(false);
    }
    return false;
  }, [api, clientId, sessionId, chat.nextCursor, loadingEarlier, setChat]);

  return { chat, setChat, connection, loading, loadingEarlier, error, refresh, earlier };
}
