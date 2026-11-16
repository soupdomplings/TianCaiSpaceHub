import { describe, expect, it, vi, afterEach } from 'vitest';
import { RemoteApi, readEventStream } from '../src/api';
import { applyEvent, emptyChat, fromHistory, prependHistory, shouldSendKey } from '../src/session-state';
import { mergeLiveSnapshot, mergeSnapshot } from '../src/use-session';
import type { HistoryResponse, RemoteEvent, RemoteItem } from '../src/types';

afterEach(() => vi.unstubAllGlobals());
const item = (id: string, text = id): RemoteItem => ({ id, role: 'assistant', type: 'message', text });
const history = (items: RemoteItem[], cursor = 5): HistoryResponse => ({ items, cursor, status: 'idle', approvals: [], nextCursor: 'older' });
const delta: RemoteEvent = { id: 6, type: 'message.delta', clientId: 'codex:a', sessionId: 'thread-a', itemId: 'answer', data: { text: '你好' } };

describe('native event isolation and replay', () => {
  it('ignores another source and duplicate events without advancing the session cursor', () => {
    const state = applyEvent(emptyChat, { ...delta, id: 5, data: { text: '已收到：' } }, 'codex:a', 'thread-a');
    expect(applyEvent(state, { ...delta, clientId: 'codex:b' }, 'codex:a', 'thread-a')).toBe(state);
    expect(applyEvent(state, { ...delta, sessionId: 'thread-b' }, 'codex:a', 'thread-a')).toBe(state);
    const updated = applyEvent(state, delta, 'codex:a', 'thread-a');
    expect(updated.items[0].text).toBe('已收到：你好');
    expect(applyEvent(updated, delta, 'codex:a', 'thread-a')).toBe(updated);
  });

  it('replaces incremental content with the final native item and ends its busy state', () => {
    let state = applyEvent(emptyChat, delta, 'codex:a', 'thread-a');
    state = applyEvent(state, { ...delta, id: 7, type: 'item.updated', data: { item: item('answer', '你好，已完成。') } }, 'codex:a', 'thread-a');
    state = applyEvent(state, { ...delta, id: 8, type: 'turn.updated', data: { status: 'completed' } }, 'codex:a', 'thread-a');
    expect(state.items).toHaveLength(1);
    expect(state.items[0].text).toBe('你好，已完成。');
    expect(state.status).toBe('completed');
  });

  it('keeps previously paged messages when a reconnect fetches a newer native snapshot', () => {
    const state = fromHistory(history([item('older'), item('anchor'), item('last', '未完成')]));
    const merged = mergeSnapshot(state, history([item('anchor'), item('last', '完成'), item('new')]));
    expect(merged.items.map(value => value.id)).toEqual(['older', 'anchor', 'last', 'new']);
    expect(merged.items[2].text).toBe('完成');
    expect(merged.nextCursor).toBe('older');
  });

  it('prepends older history once without replaying duplicate message ids or resetting live state', () => {
    const state = { ...fromHistory(history([item('current')], 30)), status: 'running', turnId: 'real-turn' };
    const merged = prependHistory(state, { ...history([item('old'), item('current')], 2), nextCursor: null });
    expect(merged.items.map(value => value.id)).toEqual(['old', 'current']);
    expect(merged.cursor).toBe(30);
    expect(merged.turnId).toBe('real-turn');
    expect(merged.status).toBe('running');
  });

  it('does not append an already included delta after a native snapshot', () => {
    const state = fromHistory(history([item('answer', 'A')], 5));
    const replayed = applyEvent(state, { ...delta, data: { text: 'A' } }, 'codex:a', 'thread-a');
    expect(replayed.items[0].text).toBe('A');
    expect(replayed.cursor).toBe(6);
  });

  it('recovers a delta missing from the native snapshot using an absolute refresh', () => {
    const state = fromHistory(history([item('answer', 'A')], 5));
    const pending = applyEvent(state, { ...delta, data: { text: 'B' } }, 'codex:a', 'thread-a');
    expect(pending.items[0].text).toBe('A');
    const refreshed = mergeLiveSnapshot(pending, history([item('answer', 'AB')], 6), pending.cursor);
    expect(refreshed.items[0].text).toBe('AB');
    // A background native read must not skip any still incoming SSE events.
    expect(refreshed.cursor).toBe(6);
    expect(refreshed.snapshotItemIds.has('answer')).toBe(true);
  });

  it('ignores replayed item.started and keeps a newer completed item across an in-flight snapshot', () => {
    const initial = fromHistory(history([item('answer', 'A')], 5));
    const started = applyEvent(initial, { ...delta, id: 6, type: 'item.updated', data: { item: item('answer', ''), completed: false } }, 'codex:a', 'thread-a');
    expect(started.items[0].text).toBe('A');
    const completed = applyEvent(started, { ...delta, id: 7, type: 'item.updated', data: { item: { ...item('answer', 'AB'), status: 'completed' }, completed: true } }, 'codex:a', 'thread-a');
    const refreshed = mergeLiveSnapshot(completed, history([item('answer', 'A')], 5), 6);
    expect(refreshed.items[0].text).toBe('AB');
    expect(refreshed.snapshotItemIds.has('answer')).toBe(false);
    expect(refreshed.cursor).toBe(7);
  });

  it('preserves new live items and metadata received while an absolute snapshot is loading', () => {
    let state = fromHistory(history([item('old')], 5));
    state = applyEvent(state, { ...delta, itemId: 'new', data: { text: 'A' } }, 'codex:a', 'thread-a');
    state = applyEvent(state, { ...delta, id: 7, itemId: 'new', data: { text: 'B' } }, 'codex:a', 'thread-a');
    state = applyEvent(state, { ...delta, id: 8, type: 'turn.updated', data: { status: 'running', turnId: 'new-turn' } }, 'codex:a', 'thread-a');
    const refreshed = mergeLiveSnapshot(state, history([item('old'), item('new', 'A')], 999), 6);
    expect(refreshed.items[1].text).toBe('AB');
    expect(refreshed.status).toBe('running');
    expect(refreshed.turnId).toBe('new-turn');
    expect(refreshed.cursor).toBe(8);
    expect(refreshed.snapshotItemIds.has('new')).toBe(false);
    const absentFromNative = mergeLiveSnapshot(state, history([item('old')], 999), 6);
    expect(absentFromNative.items.map(value => value.id)).toEqual(['old', 'new']);
    expect(absentFromNative.items[1].text).toBe('AB');
    const toolDelta = applyEvent(emptyChat, { ...delta, data: { text: 'stdout', role: 'tool', type: 'tool' } }, 'codex:a', 'thread-a');
    expect(toolDelta.items[0].role).toBe('tool');
    expect(toolDelta.items[0].type).toBe('tool');
  });
});

describe('SSE transport', () => {
  it('decodes UTF-8 split inside Chinese text and CRLF boundaries', async () => {
    const second = { ...delta, id: 7, data: { text: '完成' } };
    const bytes = new TextEncoder().encode(`: heartbeat\r\nid: 6\r\ndata: ${JSON.stringify(delta)}\r\n\r\nid: 7\r\ndata: ${JSON.stringify(second)}\r\n\r\n`);
    const body = new ReadableStream<Uint8Array>({ start(controller) {
      for (let offset = 0; offset < bytes.length; offset += 2) controller.enqueue(bytes.slice(offset, offset + 2));
      controller.close();
    } });
    const received: RemoteEvent[] = [];
    await readEventStream(body, new AbortController().signal, value => { received.push(value); });
    expect(received).toEqual([delta, second]);
  });

  it('supports SSE multi-line JSON data and cancels the reader when the view is closed', async () => {
    const abort = new AbortController();
    const cancel = vi.fn();
    const body = new ReadableStream<Uint8Array>({ start(controller) {
      controller.enqueue(new TextEncoder().encode(`data: {"id":6,"type":"message.delta",\ndata: "clientId":"codex:a","sessionId":"thread-a","data":{"text":"hello"}}\n\n`));
    }, cancel });
    const received: RemoteEvent[] = [];
    await readEventStream(body, abort.signal, value => { received.push(value); abort.abort(); });
    expect(received).toHaveLength(1);
    expect(received[0].data.text).toBe('hello');
    expect(cancel).toHaveBeenCalledTimes(1);
  });
});

describe('gateway requests', () => {
  it('does not replay a POST after a network failure; sends only the selected device reference', async () => {
    const fetch = vi.fn().mockRejectedValue(new TypeError('connection lost'));
    vi.stubGlobal('fetch', fetch);
    const api = new RemoteApi('paired-device');
    await expect(api.send('codex:a', 'native/thread', { text: '继续', attachmentIds: [], requestId: 'request-1' })).rejects.toThrow('connection lost');
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(fetch.mock.calls[0][0]).toBe('/api/remote/v1/clients/codex%3Aa/sessions/native%2Fthread/turns');
    const request = fetch.mock.calls[0][1] as RequestInit;
    expect(new Headers(request.headers).get('X-Hub-Target')).toBe('paired-device');
    expect(new Headers(request.headers).has('Authorization')).toBe(false);
  });

  it('shows the server error message rather than only an HTTP status', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: { code: 'client_not_ready', message: '请先启动目标天工客户端' } }), { status: 409 })));
    await expect(new RemoteApi('device').clients()).rejects.toThrow('请先启动目标天工客户端');
  });
});

describe('Chinese IME and send shortcut', () => {
  const key = { key: 'Enter', ctrlKey: true, metaKey: false, shiftKey: false, isComposing: false, keyCode: 13 };
  it('submits Ctrl/Command+Enter, keeping plain Enter and IME confirmation as editing', () => {
    expect(shouldSendKey(key)).toBe(true);
    expect(shouldSendKey({ ...key, ctrlKey: false, metaKey: true })).toBe(true);
    expect(shouldSendKey({ ...key, ctrlKey: false })).toBe(false);
    expect(shouldSendKey({ ...key, isComposing: true })).toBe(false);
    expect(shouldSendKey({ ...key, keyCode: 229 })).toBe(false);
    expect(shouldSendKey({ ...key, shiftKey: true })).toBe(false);
  });
});
