import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, renderHook, screen, waitFor } from '@testing-library/react';
import App from '../src/App';
import { ApprovalCard, Markdown, MessageItem } from '../src/components';
import { ApiError, RemoteApi } from '../src/api';
import { useSession } from '../src/use-session';
import { pollCatalog } from '../src/page-resume';
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

  it('keeps tool output, reasoning and file-change cards out of the transcript while retaining normal Markdown', () => {
    render(<>{(['fileChange','reasoning','tool','message'] as const).map((type, i) => <MessageItem key={i} clientName="Codex" onFile={()=>{}} item={{ id:String(i),role: type==='reasoning'?'assistant':'tool',type,text:'内部过程' }}/>)}
      <MessageItem clientName="Codex" onFile={()=>{}} item={{id:'result',role:'assistant',type:'message',text:'改动摘要：**已完成**'}}/></>);
    expect(screen.queryByText('内部过程')).toBeNull();
    expect(screen.getByText('已完成')).toBeTruthy();
  });

  it('uses structured activity metadata in mixed-version transcripts, without filtering words from normal chat', () => {
    render(<>{['command_execution','reasoning','file_write'].map(kind => <MessageItem key={kind} clientName="Codex" onFile={()=>{}} item={{id:kind,role:'assistant',type:'message',text:'内部日志',data:{nativeType:kind}}}/>)}
      <MessageItem clientName="Codex" onFile={()=>{}} item={{id:'human',role:'user',type:'message',text:'请说明工具执行和文件变更'}}/></>);
    expect(screen.queryByText('内部日志')).toBeNull();
    expect(screen.getByText('请说明工具执行和文件变更')).toBeTruthy();
  });
  it('renders an MCP boolean and integer, and only submits the original answered form once', async () => {
    const respond=vi.fn().mockResolvedValue(undefined);
    render(<ApprovalCard canRespond onRespond={respond} approval={{id:'req:7',type:'question',title:'确认参数',canDecline:true,questions:[
      {id:'ok',prompt:'是否继续',allowOther:false,options:[{value:'true',label:'是'},{value:'false',label:'否'}]},
      {id:'count',prompt:'次数',inputType:'integer'}]}}/>);
    expect(screen.queryByRole('textbox')).toBeNull();
    expect(screen.getByRole('button',{name:'拒绝'})).toBeTruthy();
    fireEvent.click(screen.getByRole('radio',{name:'否'}));
    fireEvent.change(screen.getByRole('spinbutton'),{target:{value:'1.5'}});
    expect((screen.getByRole('button',{name:'提交回答'}) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(screen.getByRole('spinbutton'),{target:{value:'2'}});
    const submit=screen.getByRole('button',{name:'提交回答'});
    fireEvent.click(submit);fireEvent.click(submit);
    await screen.findByText('已提交，正在同步目标端。');
    expect(respond).toHaveBeenCalledExactlyOnceWith('accept',{ok:'false',count:'2'});
  });
  it('never offers a local allow or decline action for a desktop-only prompt', () => {
    render(<ApprovalCard canRespond onRespond={vi.fn()} approval={{id:'native:7',type:'approval',title:'需要在桌面登录',desktopOnly:true,choices:[]}}/>);
    expect(screen.getByText('这项请求需在目标客户端处理。')).toBeTruthy();
    expect(screen.queryByRole('button')).toBeNull();
  });
});

describe('session lifetime', () => {
  it('keeps hidden artifacts in the manual catalogue through later completion, settings changes and background snapshots', async () => {
    localStorage.setItem('hub-remote-live-activity', JSON.stringify({reasoning:true,tool:true,fileChange:true,artifact:true}));
    const api = new RemoteApi('device-a');
    const file = (id: string, kind: 'artifact' | 'attachment' = 'artifact') => ({ id, kind, name:id+'.png', size:2, mimeType:'image/png', url:'/api/remote/v1/clients/c/sessions/s/files/'+id });
    const message = (id: string, files: ReturnType<typeof file>[]) => ({id,role:'assistant' as const,type:'message' as const,text:'',files});
    const initial = message('history',[file('historical')]);
    const mixed = {...message('partial',[file('off'),file('uploaded','attachment')]), text:'正文 ![隐藏图]('+file('off').url+') [公开链接](https://example.com/)'};
    const late = message('new',[file('on')]);
    const background = message('background',[file('snapshot')]);
    const reads = vi.spyOn(api,'history').mockResolvedValueOnce({items:[initial],cursor:1,status:'running',approvals:[]})
      .mockResolvedValue({items:[initial,mixed,late,background],cursor:5,status:'running',approvals:[]});
    let emit!: (value:RemoteEvent) => unknown;
    vi.spyOn(api,'events').mockImplementation(async (_c,_s,_a,signal,receive,open) => {
      emit=receive; open?.(); await new Promise<void>(resolve=>signal.addEventListener('abort',()=>resolve()));
    });
    const {result}=renderHook(()=>useSession(api,client,'thread-a'));
    await waitFor(()=>expect(result.current.connection).toBe('live'));
    let artifact = false;
    const send=(id:number,item:ReturnType<typeof message>,completed:boolean)=>act(()=>{emit({id,type:'item.updated',clientId:client.id,sessionId:'thread-a',data:{item,completed,displayContent:{artifact}}});});
    send(2,mixed,false);
    artifact = true;
    send(3,message('partial',[...mixed.files,file('only-in-completion')]),true);
    send(4,late,true);
    expect(reads).toHaveBeenCalledTimes(1);
    act(()=>result.current.refresh());
    await waitFor(()=>expect(result.current.chat.cursor).toBe(5));
    const display=result.current.artifactDisplay;
    const visible=(item:ReturnType<typeof message>)=>display.forTranscript(item)?.files?.map(f=>f.id) || [];
    expect(visible(initial)).toEqual(['historical']);
    expect(visible(message('partial',[...mixed.files,file('only-in-completion')]))).toEqual(['uploaded']);
    expect(visible(background)).toEqual([]);
    expect(visible(late)).toEqual(['on']);
    expect([...display.files.keys()]).toEqual(['historical','off','only-in-completion','on','snapshot']);
    const {container}=render(<Markdown text={mixed.text} hiddenUrls={display.hiddenLinksFor(mixed)}/>);
    expect(container.querySelector('img')).toBeNull();
    expect(container.textContent).toContain('公开链接');
    expect(container.textContent).not.toContain('隐藏图');
    localStorage.removeItem('hub-remote-live-activity');
  });
  it('follows target Hub choices on each event without reading local preferences, history or reopening the stream', async () => {
    localStorage.setItem('hub-remote-live-activity', JSON.stringify({reasoning:true,tool:true,fileChange:true,artifact:true}));
    const api = new RemoteApi('device-a');
    const supported = { ...client, capabilities: { ...client.capabilities, liveActivity: true } };
    const reads = vi.spyOn(api, 'history').mockResolvedValue({ items: [], status:'running', cursor:2, approvals:[] });
    let deliver: (event: RemoteEvent) => unknown = () => {};
    const streams = vi.spyOn(api, 'events').mockImplementation(async (_c, _s, _a, signal, receive, open) => {
      deliver = receive; open?.(); await new Promise<void>(resolve => signal.addEventListener('abort', () => resolve()));
    });
    const {result} = renderHook(() => useSession(api, supported, 'thread-a'));
    await waitFor(() => expect(result.current.connection).toBe('live'));
    let tool = false;
    const detail = (id: number, category: string, text: string) => act(() => { deliver({ id, type:'cursor.advanced', clientId: client.id, sessionId:'thread-a', data:{ displayContent:{tool}, liveActivity:{ category, itemId:'i',mode:'append',text } } }); });
    detail(3, 'tool', '之前');
    tool = true;
    detail(4, 'tool', '可见');
    detail(5, 'fileChange', '文件');
    tool = false;
    detail(6, 'tool', '之后');
    act(() => { deliver({id:7,type:'approval.requested',clientId:client.id,sessionId:'thread-a',data:{approval:{id:'q',type:'question',title:'选择目标'}}}); });
    expect(result.current.activityRows.map(r => r.text)).toEqual(['可见']);
    expect(result.current.chat.approvals[0].id).toBe('q');
    expect(reads).toHaveBeenCalledTimes(1);
    expect(streams).toHaveBeenCalledTimes(1);
    expect(streams.mock.calls[0][6]).toBe(true);
    expect(result.current.chat.cursor).toBe(7);
    expect('toggleActivity' in result.current).toBe(false);
    localStorage.removeItem('hub-remote-live-activity');
  });
  it('recovers a temporary loss when the network returns, but stops after the pair is revoked', async () => {
    const api = new RemoteApi('device-a');
    const historyReads = vi.spyOn(api, 'history').mockRejectedValueOnce(new ApiError(502, 'peer_unreachable', '网络暂不可用'))
      .mockResolvedValue({items:[],status:'idle',cursor:3,approvals:[]});
    const events = vi.spyOn(api, 'events').mockRejectedValue(new ApiError(410, 'pairing_revoked', '设备授权已撤销'));
    const writes = vi.spyOn(api, 'send');
    const {result} = renderHook(() => useSession(api, client, 'thread-a'));
    await waitFor(() => expect(result.current.connection).toBe('reconnecting'));
    fireEvent(window, new Event('online'));
    await waitFor(() => expect(result.current.error).toBe('设备授权已撤销'));
    fireEvent(window, new Event('blur')); fireEvent(window, new Event('focus')); fireEvent(window, new Event('online'));
    expect(result.current.connection).toBe('offline');
    expect(historyReads).toHaveBeenCalledTimes(2);
    expect(events).toHaveBeenCalledTimes(1);
    expect(writes).not.toHaveBeenCalled();
  });
  it('keeps the visible desktop transcript if the native session cannot be resumed', async () => {
    const api = new RemoteApi('device-a');
    vi.spyOn(api, 'history').mockResolvedValue({items:[{id:'old',role:'assistant',type:'message',text:'目标电脑的原回答'}],status:'idle',cursor:1,approvals:[]});
    vi.spyOn(api, 'events').mockRejectedValue(new ApiError(409, 'remote_request_failed', '当前模型渠道无法恢复运行'));
    const {result} = renderHook(() => useSession(api, client, 'thread-a'));
    await waitFor(() => expect(result.current.connection).toBe('offline'));
    expect(result.current.hasSnapshot).toBe(true);
    expect(result.current.chat.items.map(item => item.text)).toEqual(['目标电脑的原回答']);
    expect(result.current.error).toBe('当前模型渠道无法恢复运行');
  });
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
    await act(async () => { await onEvent?.({ id: 7, type: 'message.delta', clientId: client.id, sessionId:'thread-a', itemId:'answer', data:{text:'C'} }); });
    await act(async () => { await onEvent?.({ id: 8, type: 'item.updated', clientId: client.id, sessionId: 'thread-a', itemId: 'answer', data: { completed: true, item: { id: 'answer', role: 'assistant', type: 'message', text: 'ABC', status: 'completed' } } }); });
    await act(async () => { resolveRead(initial); });
    // The extra delta while the read was pending must not queue another read.
    await act(async () => { await new Promise(resolve=>setTimeout(resolve,300)); });
    expect(result.current.chat.items[0].text).toBe('ABC');
    expect(result.current.chat.cursor).toBe(8);
    expect(nativeRead).toHaveBeenCalledTimes(2);
    expect(events).toHaveBeenCalledTimes(1);
    unmount();
  });
});

describe('composer integration with the local gateway', () => {
  it('does not start or poll a catalog behind a hidden tab and coalesces visible recovery', async () => {
    vi.useFakeTimers();
    const hidden=vi.spyOn(document,'hidden','get').mockReturnValue(true);
    const read=vi.fn(async()=>{});
    const stop=pollCatalog(read,30000);
    try {
      await vi.advanceTimersByTimeAsync(90000);
      expect(read).not.toHaveBeenCalled();
      hidden.mockReturnValue(false);
      document.dispatchEvent(new Event('visibilitychange'));
      window.dispatchEvent(new Event('focus'));
      await Promise.resolve();
      expect(read).toHaveBeenCalledTimes(1);
      await vi.advanceTimersByTimeAsync(30000);
      expect(read).toHaveBeenCalledTimes(2);
    } finally {stop(); hidden.mockRestore(); vi.useRealTimers();}
  });
  it('opens a bookmark while its session list is slow, reads only summaries and requests options on demand', async () => {
    const apiCalls:string[]=[];
    const json=(v:unknown)=>new Response(JSON.stringify(v));
    const deferred={ ...client, models:[], modelsDeferred:true };
    vi.stubGlobal('fetch', vi.fn(async (input:RequestInfo|URL) => {
      const url=new URL(String(input),location.href), path=url.pathname;
      apiCalls.push(path+url.search);
      if(path.endsWith('/devices')) return json({devices:[{id:'device-a',name:'目标',online:true,paired:true}],localDeviceId:'local'});
      if(path.endsWith('/clients')) { expect(url.searchParams.get('view')).toBe('summary'); return json({clients:[deferred]}); }
      if(path.endsWith('/sessions')) { expect(url.searchParams.get('limit')).toBe('16'); return new Promise<Response>(()=>{}); }
      if(path.endsWith('/history')) return json({items:[{id:'m',type:'message',role:'user',text:'已直达正文'}],cursor:8,status:'idle',approvals:[]});
      if(path.endsWith('/events')) return new Response(new ReadableStream());
      if(path.endsWith('/models')) return json({models:client.models,modelSelection:true,effortSelection:false});
      if(path.endsWith('/projects')) { expect(url.searchParams.get('limit')).toBe('12'); return json({projects:[]}); }
      throw Error(path);
    }));
    render(<App/>);
    await screen.findByText('已直达正文');
    await screen.findByText('已同步');
    expect(apiCalls).toHaveLength(5); // devices, lightweight capabilities, sidebar, one history page, stream
    expect(apiCalls.filter(v=>/\/(models|projects)\b/.test(v))).toHaveLength(0);
    fireEvent.click(screen.getByRole('button',{name:'选择本轮模型'}));
    await screen.findByRole('combobox',{name:'本轮模型'});
    fireEvent.click(screen.getByRole('button',{name:'筛选项目'}));
    await screen.findByText('没有可筛选的项目');
    expect(apiCalls.filter(v=>v.endsWith('/models'))).toHaveLength(1);
    expect(apiCalls.filter(v=>v.includes('/projects?'))).toHaveLength(1);
    expect(apiCalls.filter(v=>v.includes('/history?'))).toHaveLength(1);
  });
  it('opens one small history page and prepends one page per upward gesture, without moving the reader or repeating writes', async () => {
    const reads: string[] = [], writes: string[] = [];
    let complete: (body: Response) => void = () => {};
    const text = (id:string) => ({id,role:'user',type:'message',text:id});
    const json = (v:unknown) => new Response(JSON.stringify(v));
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?:RequestInit) => {
      const url = new URL(String(input), location.href), path = url.pathname;
      if (init?.method && init.method !== 'GET') writes.push(init.method);
      if (path.endsWith('/devices')) return json({devices:[{id:'device-a',name:'目标',online:true,paired:true}],localDeviceId:'local'});
      if (path.split('?')[0].endsWith('/clients')) return json({clients:[client]});
      if (path.endsWith('/sessions')) return json({sessions:[{id:'thread-a',title:'已有会话',status:'idle',updatedAt:1}]});
      if (path.endsWith('/projects')) return json({projects:[]});
      if (path.endsWith('/history')) {
        expect(url.searchParams.get('limit')).toBe('3');
        const cursor = url.searchParams.get('cursor') || ''; reads.push(cursor);
        if (cursor === 'page2') return new Promise<Response>(resolve=>{complete=resolve;});
        return json({items:[text(cursor ? '最早' : '最近')],nextCursor:cursor ? null : 'page2',cursor:8,status:'idle',approvals:[]});
      }
      if (path.endsWith('/events')) return new Response(new ReadableStream());
      throw Error(path);
    }));
    render(<App />);
    await screen.findByText('已同步');
    const pane = screen.getByRole('region', {name:'会话记录'});
    Object.defineProperties(pane, {scrollHeight:{get:()=>pane.querySelectorAll('article').length*200+200},clientHeight:{value:150}});
    pane.scrollTop = 25;
    fireEvent.scroll(pane);
    fireEvent.wheel(pane,{deltaY:200});
    expect(reads).toEqual(['']);
    fireEvent.wheel(pane,{deltaY:-100}); fireEvent.wheel(pane,{deltaY:-100});
    await waitFor(()=>expect(reads).toEqual(['','page2']));
    await act(async()=>complete(json({items:[text('较早'), text('最近')],nextCursor:'page3',cursor:1,status:'idle',approvals:[]})));
    await screen.findByText('较早');
    expect(pane.scrollTop).toBe(225);
    expect(screen.getAllByText('最近')).toHaveLength(1);
    fireEvent.scroll(pane);
    expect(reads).toEqual(['','page2']);
    fireEvent.keyDown(pane,{key:'Home'});
    await screen.findByText('最早');
    expect(reads).toEqual(['','page2','page3']);
    expect(writes).toEqual([]);
  });
  it('resumes a frozen tab on the same thread, without replacing the editor, scrolling up to bottom or replaying a write', async () => {
    let historyReads = 0, deviceReads = 0;
    const streams: { after: string | null; signal?: AbortSignal | null }[] = [];
    const writes: string[] = [];
    const fetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = new URL(String(input), location.href), path = url.pathname;
      const json = (body: unknown) => new Response(JSON.stringify(body));
      if (init?.method && init.method !== 'GET') writes.push(init.method);
      if (path.endsWith('/devices')) { deviceReads++; return json({ devices:[{ id:'device-a',name:'目标电脑',online:true,paired:true }],localDeviceId:'local' }); }
      if (path.split('?')[0].endsWith('/clients')) return json({ clients:[client] });
      if (path.endsWith('/sessions')) return json({ sessions:[{ id:'thread-a',title:'已有会话',status:'idle',updatedAt:1 }] });
      if (path.endsWith('/projects')) return json({ projects:[] });
      if (path.endsWith('/thread-a/history')) {
        const newest = ++historyReads > 1;
        return json({items:[{id:'answer',role:'assistant',type:'message',text:'原对话的回答'}, ...(newest ? [{id:'next',role:'assistant',type:'message',text:'离开后产生的消息'}] : [])],status:'idle',cursor:newest ? 9 : 3,approvals:[]});
      }
      if (path.endsWith('/thread-a/events')) { streams.push({after:url.searchParams.get('after'),signal:init?.signal}); return new Response(new ReadableStream()); }
      throw new Error(path);
    });
    vi.stubGlobal('fetch', fetch);
    render(<App />);
    await screen.findByText('已同步');
    const editor = screen.getByRole('textbox', {name:'消息'});
    fireEvent.change(editor, {target:{value:'尚未发送的草稿'}});
    const transcript = screen.getByText('原对话的回答').closest('.transcript') as HTMLDivElement;
    Object.defineProperties(transcript, {scrollHeight:{value:1000},clientHeight:{value:300}});
    transcript.scrollTop = 160; fireEvent.scroll(transcript);
    try {
      Object.defineProperty(document, 'hidden', {value:true, configurable:true});
      fireEvent(document, new Event('visibilitychange'));
      fireEvent(window, new PageTransitionEvent('pagehide', {persisted:true}));
      Object.defineProperty(document, 'hidden', {value:false, configurable:true});
      fireEvent(document, new Event('visibilitychange'));
      fireEvent(window, new PageTransitionEvent('pageshow', {persisted:true}));
      await screen.findByText('离开后产生的消息');
      await waitFor(()=>expect(streams.map(stream=>stream.after)).toEqual(['3','9']));
      expect(streams[0].signal?.aborted).toBe(true);
      expect(historyReads).toBe(2);
      expect(deviceReads).toBe(2);
      expect(screen.getByRole('textbox',{name:'消息'})).toBe(editor);
      expect((editor as HTMLTextAreaElement).value).toBe('尚未发送的草稿');
      expect(transcript.scrollTop).toBe(160);
      expect(writes).toEqual([]);
    } finally { Reflect.deleteProperty(document, 'hidden'); }
  });
  it('finds earlier conversation across activity-only pages and preserves artifacts, approvals and live messages', async () => {
    const reads: string[] = [];
    let emit!: (event: RemoteEvent) => void | Promise<void>;
    vi.spyOn(RemoteApi.prototype, 'events').mockImplementation(async (_c,_s,_after,signal,onEvent,onOpen) => {
      emit = onEvent; onOpen?.(); await new Promise<void>(resolve => signal.addEventListener('abort', () => resolve(), {once:true}));
    });
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const url = new URL(String(input), location.href), path = url.pathname;
      const json = (v: unknown) => new Response(JSON.stringify(v));
      if(path.endsWith('/devices')) return json({devices:[{id:'device-a',name:'目标',online:true,paired:true}],localDeviceId:'local'});
      if(path.endsWith('/clients')) return json({clients:[client]});
      if(path.endsWith('/projects')) return json({projects:[]});
      if(path.endsWith('/sessions')) return json({sessions:[{id:'thread-a',title:'原会话',status:'running',updatedAt:1}]});
      if(path.endsWith('/history')) {
        const cursor=url.searchParams.get('cursor') || '0'; reads.push(cursor);
        const items=cursor==='4' ? [{id:'old',role:'user',type:'message',text:'早先的问题'}] :
          [{id:'activity'+cursor,role:'tool',type:'fileChange',text:'原生文件修改',files:cursor==='0'?[{id:'f',name:'result.pdf',kind:'artifact',mimeType:'application/pdf',size:3}]:[]}];
        return json({items, nextCursor:cursor==='4'?null:String(Number(cursor)+1),cursor:1,status:'running',approvals:[{id:'approval',type:'approval',title:'允许写入吗？'}]});
      }
      throw Error(path);
    }));
    render(<App/>);
    const prompt=await screen.findByText('允许写入吗？');
    expect(screen.queryByRole('button', {name:'显示内容'})).toBeNull();
    expect(screen.getByText('请更新会话所在电脑 Hub（发送端），在「远程连接 → 设置」管理显示内容。')).toBeTruthy();
    expect(screen.getByRole('region',{name:'待处理请求'}).contains(prompt)).toBe(true);
    expect(screen.getByRole('region',{name:'会话记录'}).contains(prompt)).toBe(false);
    await waitFor(()=>expect(reads).toEqual(['0','1','2','3']));
    expect(screen.queryByText('result.pdf')).toBeNull();
    fireEvent.click(screen.getByRole('button',{name:'查看产物'}));
    expect(await screen.findByText('result.pdf')).toBeTruthy();
    fireEvent.keyDown(screen.getByRole('dialog'), {key:'Escape'});
    expect(screen.queryByText('今天要做些什么？')).toBeNull();
    fireEvent.click(screen.getByRole('button',{name:'更早的消息'}));
    await screen.findByText('早先的问题');
    await act(async()=>{await emit({clientId:client.id,sessionId:'thread-a',id:2,itemId:'think',type:'message.delta',data:{type:'reasoning',text:'思考过程详情'}});
      await emit({clientId:client.id,sessionId:'thread-a',id:3,itemId:'answer',type:'message.delta',data:{text:'公开的回答'}});});
    expect(screen.queryByText('思考过程详情')).toBeNull();
    expect(screen.queryByText('原生文件修改')).toBeNull();
    expect(screen.getByText('公开的回答')).toBeTruthy();
  });
  it('shows a history failure distinctly from an empty conversation and retries the same thread without writing', async () => {
    let reads = 0;
    const writes: string[] = [];
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input);
      if (init?.method === 'POST') writes.push(path);
      const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status });
      if (path.endsWith('/devices')) return json({ devices:[{id:'device-a',name:'目标',online:true,paired:true}],localDeviceId:'device-a' });
      if (path.split('?')[0].endsWith('/clients')) return json({ clients:[client] });
      if (path.includes('/projects?')) return json({ projects:[] });
      if (path.includes('/sessions?')) return json({ sessions:[{id:'thread-a',title:'之前的会话',status:'idle',updatedAt:1}] });
      if (path.includes('/sessions/thread-a/history?')) {
        if (++reads === 1) return json({error:{message:'目标端未读到此会话记录'}}, 409);
        return json({items:[{id:'reply',type:'message',role:'assistant',text:'之前保存的回答'}],status:'idle',cursor:7,approvals:[]});
      }
      if (path.includes('/events?')) return new Response(new ReadableStream<Uint8Array>());
      throw new Error(path);
    }));
    render(<App />);
    await screen.findByText('尚未读到会话内容');
    expect(screen.queryByText('今天要做些什么？')).toBeNull();
    fireEvent.click(await screen.findByRole('button', {name:'重新读取'}));
    await screen.findByText('之前保存的回答');
    expect(reads).toBe(2);
    expect(writes).toEqual([]);
  });
  it('keeps IME input, sends one Ctrl+Enter request, clears on acceptance, and obeys model capabilities', async () => {
    let accepted = false;
    let turnRequests = 0;
    const fetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input);
      const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { 'Content-Type': 'application/json' } });
      if (path.endsWith('/devices')) return json({ devices: [{ id: 'device-a', name: '目标电脑', online: true, paired: true, connectionType: 'direct' }], localDeviceId: 'local' });
      expect(new Headers(init?.headers).get('X-Hub-Target')).toBe('device-a');
      if (path.split('?')[0].endsWith('/clients')) return json({ clients: [client] });
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
      if (path.split('?')[0].endsWith('/clients')) return json({ clients: [client, second] });
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
      if (path.split('?')[0].endsWith('/clients')) return json({ clients: [client] });
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
      if (path.split('?')[0].endsWith('/clients')) return json({ clients: [current] });
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
    if (kind === 'codex') {
      expect(screen.queryByRole('textbox', { name: '工作目录' })).toBeNull();
      fireEvent.click(screen.getByRole('button', { name: '选择工作目录' }));
      expect(await screen.findByText('还没有可选的目标目录。')).toBeTruthy();
      expect(screen.getByText(/添加工作目录，然后刷新此列表/)).toBeTruthy();
      fireEvent.click(screen.getByRole('button', { name: '取消选择' }));
      expect((screen.getByRole('button', { name: '创建会话' }) as HTMLButtonElement).disabled).toBe(true);
    }
    else expect(await screen.findByText('请先在目标电脑创建项目，再刷新项目列表。')).toBeTruthy();
  });

  it('loads the next project page once, deduplicates ids and shares the directory with new-session selection', async () => {
    const requests: string[] = [];
    const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { 'Content-Type': 'application/json' } });
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input); requests.push(path);
      if (path.endsWith('/devices')) return json({ devices: [{ id: 'device-a', name: '目标电脑', online: true, paired: true }], localDeviceId: 'local' });
      if (path.split('?')[0].endsWith('/clients')) return json({ clients: [client] });
      if (path.includes('/projects?')) return path.includes('cursor=p2') ? json({ projects: [{ id: 'a', name: '项目 A', path: 'D:/a' }, { id: 'b', name: '项目 B', path: 'D:/b' }] }) : json({ projects: [{ id: 'a', name: '项目 A', path: 'D:/a' }], nextCursor: 'p2' });
      if (path.includes('/sessions?')) return json({ sessions: [{ id: 'thread-a', title: '原始会话', status: 'idle', updatedAt: Date.now() }] });
      if (path.includes('/history?')) return json({ items: [], status: 'idle', cursor: 1, approvals: [] });
      if (path.includes('/events?')) return new Response(new ReadableStream<Uint8Array>({ start(controller) { controller.enqueue(new TextEncoder().encode(': heartbeat\n\n')); } }));
      throw new Error(`Unexpected test request: ${path}`);
    }));
    render(<App />);
    fireEvent.click(await screen.findByRole('button', { name: '筛选项目' }));
    fireEvent.click(await screen.findByRole('button', { name: '加载更多项目' }));
    await screen.findByRole('option', { name: '项目 B' });
    expect(screen.getAllByRole('option', { name: '项目 A' })).toHaveLength(1);
    expect(requests.filter(path => path.includes('/projects?') && path.includes('cursor=p2'))).toHaveLength(1);
    fireEvent.click(screen.getByRole('button', { name: /^新会话/ }));
    fireEvent.click(screen.getByRole('button', { name: '选择工作目录' }));
    expect(screen.getByRole('button', { name: /项目 B.*D:\/b/ })).toBeTruthy();
    expect(screen.getAllByRole('button', { name: /项目 A.*D:\/a/ })).toHaveLength(1);
  });

  it('refreshes target-approved directories, searches full paths and creates only after explicit submission', async () => {
    history.replaceState(null, '', '/remote/?device=target&client=codex%3Aa&session=prior');
    let refreshed = false;
    const creates: unknown[] = [];
    const response = (value: unknown) => new Response(JSON.stringify(value), { headers: { 'Content-Type': 'application/json' } });
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input);
      if (path.endsWith('/devices')) return response({ devices: [{ id: 'target', name: '工作电脑', online: true, paired: true }], localDeviceId: 'local' });
      if (path.split('?')[0].endsWith('/clients')) return response({ clients: [client] });
      if (path.includes('/projects?')) return response({ projects: refreshed ? [{id: '/approved/alpha', name:'alpha', path:'/approved/alpha'}] : [] });
      if (path.includes('/sessions?')) return response({ sessions:[{id:'prior',title:'已有会话',cwd:'/history/alpha',updatedAt:1,status:'idle'}] });
      if (path.endsWith('/sessions') && init?.method === 'POST') {
        expect(new Headers(init.headers).get('X-Hub-Target')).toBe('target');
        creates.push(JSON.parse(String(init.body))); return response({ session:{id:'new',title:'新会话的标题',status:'idle',updatedAt:2} });
      }
      if (path.includes('/history?')) return response({ items: [], status: 'idle', cursor: 1, approvals: [] });
      if (path.includes('/events?')) return new Response(new ReadableStream<Uint8Array>());
      throw new Error('unexpected request '+path);
    }));
    render(<App />);
    fireEvent.click(await screen.findByRole('button', { name: /^新会话/ }));
    fireEvent.change(screen.getByPlaceholderText('这次想完成什么？'), { target:{value:'新会话的标题'} });
    fireEvent.click(screen.getByRole('button', { name:'选择工作目录' }));
    expect(await screen.findByRole('button', { name:/alpha.*\/history\/alpha/ })).toBeTruthy();
    refreshed = true;
    fireEvent.click(screen.getByRole('button', { name:'刷新目标目录' }));
    const approved = await screen.findByRole('button', { name:/alpha.*\/approved\/alpha/ });
    fireEvent.change(screen.getByRole('searchbox', { name:'搜索目标目录' }), {target:{value:'/approved'}});
    expect(screen.queryByRole('button', { name:/\/history\/alpha/ })).toBeNull();
    fireEvent.click(approved);
    expect(screen.getByDisplayValue('新会话的标题')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name:'更换工作目录' }));
    fireEvent.click(screen.getByRole('button', { name:'取消选择' }));
    expect(screen.getByText('/approved/alpha')).toBeTruthy();
    expect(creates).toEqual([]);
    fireEvent.click(screen.getByRole('button', { name:'创建会话' }));
    await waitFor(()=>expect(creates).toEqual([{title:'新会话的标题',projectId:'/approved/alpha'}]));
  });
});
