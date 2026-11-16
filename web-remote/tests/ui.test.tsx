import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, renderHook, screen, waitFor } from '@testing-library/react';
import App from '../src/App';
import { ApprovalCard, Markdown, MessageItem } from '../src/components';
import { RemoteApi } from '../src/api';
import { useSession } from '../src/use-session';
import type { ClientKind, HistoryResponse, RemoteClient, RemoteEvent } from '../src/types';

const client: RemoteClient = { id: 'codex:a', kind: 'codex', name: 'Codex', ready: true,
  capabilities: { history: true, send: true, create: true, attachments: true, stop: true, approvals: true, questions: true, modelSelection: false, effortSelection: false },
  models: [{ id: 'real-model', name: 'Real Model', efforts: ['low', 'high'] }] };

beforeEach(() => {
  history.replaceState(null, '', '/remote/?device=device-a&client=codex%3Aa&session=thread-a');
  vi.stubGlobal('matchMedia', vi.fn(() => ({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() })));
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

describe('native content and approval cards', () => {
  it('renders GFM tables without executing raw HTML or auto-loading external images', () => {
    const { container } = render(<Markdown text={'| 模型 | 状态 |\n| --- | --- |\n| 原始模型 | 可用 |\n\n<script>window.compromised=true</script>\n\n![image](https://example.com/private.png)'} />);
    expect(screen.getByRole('table')).toBeTruthy();
    expect(screen.getByText('原始模型')).toBeTruthy();
    expect(container.querySelector('script')).toBeNull();
    expect(container.querySelector('img')).toBeNull();
  });

  it('forwards the exact native approval choice value rather than inventing a decision', async () => {
    const respond = vi.fn().mockResolvedValue(undefined);
    render(<ApprovalCard approval={{ id: 'native-42', type: 'approval', title: '执行命令', choices: [{ value: 'option:0', label: '允许本次' }, { value: 'option:1', label: '允许本会话' }, { value: 'option:2', label: '拒绝' }] }} canRespond onRespond={respond} />);
    fireEvent.click(screen.getByRole('button', { name: '允许本会话' }));
    await waitFor(() => expect(respond).toHaveBeenCalledWith('option:1', {}));
  });

  it('collects multi-choice and free-form native questions before submitting once', async () => {
    const respond = vi.fn().mockResolvedValue(undefined);
    render(<ApprovalCard approval={{ id: 'q', type: 'question', title: '补充信息', questions: [{ id: 'platforms', prompt: '选择平台', multiple: true, options: [{ value: 'windows', label: 'Windows' }, { value: 'macos', label: 'macOS' }] }, { id: 'details', prompt: '用途' }] }} canRespond onRespond={respond} />);
    const submit = screen.getByRole('button', { name: '提交回答' });
    expect((submit as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole('checkbox', { name: 'Windows' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'macOS' }));
    fireEvent.change(screen.getAllByRole('textbox')[1], { target: { value: '远程继续原会话' } });
    fireEvent.click(submit);
    await waitFor(() => expect(respond).toHaveBeenCalledWith('accept', { platforms: ['windows', 'macos'], details: '远程继续原会话' }));
  });

  it('preserves native file-change paths, diffs and actual tool names', () => {
    render(<><MessageItem clientName="Codex" onFile={() => {}} item={{ id: 'change', role: 'tool', type: 'fileChange', text: '', data: { changes: [{ path: 'src/remote.rs', kind: 'update', diff: '+真实会话\n-替代会话' }] } }} /><MessageItem clientName="Codex" onFile={() => {}} item={{ id: 'tool', role: 'tool', type: 'tool', text: 'done', data: { tool: 'native_tool_name' } }} /></>);
    expect(screen.getByText('src/remote.rs')).toBeTruthy();
    expect(screen.getByText(/\+真实会话/)).toBeTruthy();
    expect(screen.getByText('native_tool_name')).toBeTruthy();
  });
});

describe('session lifetime', () => {
  it('closes only the subscription when unmounted, leaving native execution untouched', async () => {
    const api = new RemoteApi('device-a');
    vi.spyOn(api, 'history').mockResolvedValue({ items: [], status: 'running', turnId: 'real-turn', cursor: 3, approvals: [] });
    const stop = vi.spyOn(api, 'stop');
    let subscriptionSignal: AbortSignal | undefined;
    vi.spyOn(api, 'events').mockImplementation(async (_client, _session, _after, signal, _event, onOpen) => {
      subscriptionSignal = signal; onOpen?.();
      await new Promise<void>(resolve => signal.addEventListener('abort', () => resolve(), { once: true }));
    });
    const { result, unmount } = renderHook(() => useSession(api, client, 'thread-a'));
    await waitFor(() => expect(result.current.connection).toBe('live'));
    expect(result.current.chat.turnId).toBe('real-turn');
    unmount();
    expect(subscriptionSignal?.aborted).toBe(true);
    expect(stop).not.toHaveBeenCalled();
  });

  it('coalesces baseline deltas into an absolute read without reconnecting SSE or rolling back a newer completion', async () => {
    const api = new RemoteApi('device-a');
    const initial: HistoryResponse = { items: [{ id: 'answer', role: 'assistant', type: 'message', text: 'A' }], status: 'running', turnId: 'real-turn', cursor: 5, approvals: [] };
    let resolveRead: (snapshot: HistoryResponse) => void = () => {};
    const nativeRead = vi.spyOn(api, 'history').mockResolvedValueOnce(initial).mockImplementation(() => new Promise(resolve => { resolveRead = resolve; }));
    let onEvent: ((event: RemoteEvent) => void | Promise<void>) | undefined;
    const events = vi.spyOn(api, 'events').mockImplementation(async (_client, _session, _after, signal, callback, onOpen) => {
      onEvent = callback; onOpen?.();
      await new Promise<void>(resolve => signal.addEventListener('abort', () => resolve(), { once: true }));
    });
    const { result, unmount } = renderHook(() => useSession(api, client, 'thread-a'));
    await waitFor(() => expect(result.current.connection).toBe('live'));
    await act(async () => { await onEvent?.({ id: 6, type: 'message.delta', clientId: client.id, sessionId: 'thread-a', itemId: 'answer', data: { text: 'B' } }); });
    expect(result.current.chat.items[0].text).toBe('A');
    await waitFor(() => expect(nativeRead).toHaveBeenCalledTimes(2));
    await act(async () => { await onEvent?.({ id: 7, type: 'item.updated', clientId: client.id, sessionId: 'thread-a', itemId: 'answer', data: { completed: true, item: { id: 'answer', role: 'assistant', type: 'message', text: 'AB', status: 'completed' } } }); });
    await act(async () => { resolveRead(initial); });
    expect(result.current.chat.items[0].text).toBe('AB');
    expect(result.current.chat.cursor).toBe(7);
    expect(events).toHaveBeenCalledTimes(1);
    unmount();
  });
});

describe('composer integration with the local gateway', () => {
  it('keeps IME input, sends one Ctrl+Enter request, clears on acceptance, and obeys model capabilities', async () => {
    let accepted = false;
    let turnRequests = 0;
    const fetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input);
      const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { 'Content-Type': 'application/json' } });
      if (path.endsWith('/devices')) return json({ devices: [{ id: 'device-a', name: '目标电脑', online: true, paired: true, connectionType: 'direct' }], localDeviceId: 'local' });
      expect(new Headers(init?.headers).get('X-Hub-Target')).toBe('device-a');
      if (path.endsWith('/clients')) return json({ clients: [client] });
      if (path.includes('/projects?')) return json({ projects: [] });
      if (path.includes('/history?')) return json({ items: accepted ? [{ id: 'native-user', role: 'user', type: 'message', text: '继续原会话' }] : [], status: accepted ? 'running' : 'idle', cursor: accepted ? 4 : 3, turnId: accepted ? 'real-turn' : undefined, approvals: [] });
      if (path.includes('/events?')) return new Response(new ReadableStream<Uint8Array>({ start(controller) { controller.enqueue(new TextEncoder().encode(': keep-alive\n\n')); } }), { headers: { 'Content-Type': 'text/event-stream' } });
      if (path.endsWith('/turns')) { turnRequests++; accepted = true; return json({ status: 'running', turnId: 'real-turn' }); }
      if (path.includes('/sessions?')) return json({ sessions: [{ id: 'thread-a', title: '原始会话', status: accepted ? 'running' : 'idle', updatedAt: Date.now() }] });
      throw new Error(`Unexpected test request: ${path}`);
    });
    vi.stubGlobal('fetch', fetch);
    render(<App />);
    const editor = await screen.findByRole('textbox', { name: '消息' });
    await waitFor(() => expect(screen.getByText('已同步')).toBeTruthy());
    expect(screen.queryByRole('combobox', { name: '本轮模型' })).toBeNull();
    expect(screen.queryByRole('combobox', { name: '思考强度' })).toBeNull();
    fireEvent.change(editor, { target: { value: '继续原会话' } });
    fireEvent.compositionStart(editor);
    fireEvent.keyDown(editor, { key: 'Enter', ctrlKey: true, isComposing: true, keyCode: 229 });
    expect(turnRequests).toBe(0);
    expect((editor as HTMLTextAreaElement).value).toBe('继续原会话');
    fireEvent.compositionEnd(editor);
    fireEvent.keyDown(editor, { key: 'Enter', ctrlKey: true, keyCode: 13 });
    await waitFor(() => expect((editor as HTMLTextAreaElement).value).toBe(''));
    expect(turnRequests).toBe(1);
    expect(await screen.findByText('继续原会话')).toBeTruthy();
    await act(async () => {});
  });

  it('switches native sources without sending the old source session id to the new client', async () => {
    const second: RemoteClient = { ...client, id: 'tiangong:desktop', kind: 'tiangong', name: '天工 Work' };
    const requests: string[] = [];
    const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { 'Content-Type': 'application/json' } });
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input); requests.push(path);
      if (path.endsWith('/devices')) return json({ devices: [{ id: 'device-a', name: '目标电脑', online: true, paired: true }], localDeviceId: 'local' });
      if (path.endsWith('/clients')) return json({ clients: [client, second] });
      if (path.includes('/projects?')) return json({ projects: [] });
      if (path.includes('/sessions?')) return json({ sessions: [{ id: path.includes('tiangong') ? 'native-b' : 'thread-a', title: path.includes('tiangong') ? '天工原会话' : 'Codex原会话', status: 'idle', updatedAt: Date.now() }] });
      if (path.includes('/history?')) return json({ items: [], status: 'idle', cursor: 1, approvals: [] });
      if (path.includes('/events?')) return new Response(new ReadableStream<Uint8Array>({ start(controller) { controller.enqueue(new TextEncoder().encode(': heartbeat\n\n')); } }));
      throw new Error(`Unexpected test request: ${path}`);
    }));
    render(<App />);
    await screen.findByRole('textbox', { name: '消息' });
    fireEvent.click(screen.getByRole('tab', { name: '天工 Work' }));
    await screen.findByRole('heading', { name: '天工原会话' });
    await waitFor(() => expect(requests.some(path => path.includes('/tiangong%3Adesktop/sessions/native-b/history'))).toBe(true));
    expect(requests.some(path => path.includes('/tiangong%3Adesktop/sessions/thread-a/'))).toBe(false);
  });

  it('does not enable sending while the native backend reports an unknown busy state', async () => {
    const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { 'Content-Type': 'application/json' } });
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path.endsWith('/devices')) return json({ devices: [{ id: 'device-a', name: '目标电脑', online: true, paired: true }], localDeviceId: 'local' });
      if (path.endsWith('/clients')) return json({ clients: [client] });
      if (path.includes('/projects?')) return json({ projects: [] });
      if (path.includes('/sessions?')) return json({ sessions: [{ id: 'thread-a', title: '原始会话', status: 'unknown', updatedAt: Date.now() }] });
      if (path.includes('/history?')) return json({ items: [], status: 'unknown', cursor: 1, approvals: [] });
      if (path.includes('/events?')) return new Response(new ReadableStream<Uint8Array>({ start(controller) { controller.enqueue(new TextEncoder().encode(': heartbeat\n\n')); } }));
      throw new Error(`Unexpected test request: ${path}`);
    }));
    render(<App />);
    const editor = await screen.findByRole('textbox', { name: '消息' });
    fireEvent.change(editor, { target: { value: '下一条消息' } });
    await screen.findByText('运行状态待核对');
    expect(screen.queryByRole('button', { name: '发送消息' })).toBeNull();
    expect(screen.getByRole('button', { name: '停止执行' })).toBeTruthy();
  });

  it.each<ClientKind>(['codex', 'tiangong'])('requires a usable target directory or project for new %s sessions', async kind => {
    const current: RemoteClient = { ...client, id: kind === 'tiangong' ? 'tiangong' : client.id, kind, name: kind === 'tiangong' ? '天工 Work' : 'Codex' };
    const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { 'Content-Type': 'application/json' } });
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path.endsWith('/devices')) return json({ devices: [{ id: 'device-a', name: '目标电脑', online: true, paired: true }], localDeviceId: 'local' });
      if (path.endsWith('/clients')) return json({ clients: [current] });
      if (path.includes('/projects?')) return json({ projects: [] });
      if (path.includes('/sessions?')) return json({ sessions: [{ id: 'thread-a', title: '原始会话', status: 'idle', updatedAt: Date.now() }] });
      if (path.includes('/history?')) return json({ items: [], status: 'idle', cursor: 1, approvals: [] });
      if (path.includes('/events?')) return new Response(new ReadableStream<Uint8Array>({ start(controller) { controller.enqueue(new TextEncoder().encode(': heartbeat\n\n')); } }));
      throw new Error(`Unexpected test request: ${path}`);
    }));
    render(<App />);
    await screen.findByRole('textbox', { name: '消息' });
    fireEvent.click(screen.getByRole('button', { name: /^新会话/ }));
    expect((screen.getByRole('button', { name: '创建会话' }) as HTMLButtonElement).disabled).toBe(true);
    if (kind === 'codex') expect((screen.getByRole('textbox', { name: '工作目录' }) as HTMLInputElement).required).toBe(true);
    else expect(screen.getByText('请先在目标电脑创建项目，再刷新项目列表。')).toBeTruthy();
  });

  it('loads the next project page once, deduplicates ids and shares the directory with new-session selection', async () => {
    const requests: string[] = [];
    const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { 'Content-Type': 'application/json' } });
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input); requests.push(path);
      if (path.endsWith('/devices')) return json({ devices: [{ id: 'device-a', name: '目标电脑', online: true, paired: true }], localDeviceId: 'local' });
      if (path.endsWith('/clients')) return json({ clients: [client] });
      if (path.includes('/projects?')) return path.includes('cursor=p2') ? json({ projects: [{ id: 'a', name: '项目 A', path: 'D:/a' }, { id: 'b', name: '项目 B', path: 'D:/b' }] }) : json({ projects: [{ id: 'a', name: '项目 A', path: 'D:/a' }], nextCursor: 'p2' });
      if (path.includes('/sessions?')) return json({ sessions: [{ id: 'thread-a', title: '原始会话', status: 'idle', updatedAt: Date.now() }] });
      if (path.includes('/history?')) return json({ items: [], status: 'idle', cursor: 1, approvals: [] });
      if (path.includes('/events?')) return new Response(new ReadableStream<Uint8Array>({ start(controller) { controller.enqueue(new TextEncoder().encode(': heartbeat\n\n')); } }));
      throw new Error(`Unexpected test request: ${path}`);
    }));
    render(<App />);
    fireEvent.click(await screen.findByRole('button', { name: '加载更多项目' }));
    await screen.findByRole('option', { name: '项目 B' });
    expect(screen.getAllByRole('option', { name: '项目 A' })).toHaveLength(1);
    expect(requests.filter(path => path.includes('/projects?') && path.includes('cursor=p2'))).toHaveLength(1);
    fireEvent.click(screen.getByRole('button', { name: /^新会话/ }));
    const dialog = screen.getByRole('dialog');
    expect([...dialog.querySelectorAll('select option')].some(option => option.textContent === '项目 B')).toBe(true);
  });
});
