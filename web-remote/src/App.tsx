import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { ArrowDown, ArrowUp, Check, ChevronDown, ChevronLeft, CodeXml, FileCode2, Folder, Laptop, Menu, Monitor, Moon, Network, Paperclip, Plus, RefreshCw, Search, Sparkles, Square, Sun, WifiOff } from 'lucide-react';
import { accessEnded, getDevices, RemoteApi } from './api';
import { ApprovalCard, Dots, FileChip, FilePreview, IconButton, MessageItem, Modal, Spinner, Toasts } from './components';
import type { ToastMessage } from './components';
import { isTranscriptMessage, mergeSessions, shouldSendKey } from './session-state';
import { useSession } from './use-session';
import { saveRemoteFile } from './file-download';
import { activityLabels, withActivity } from './live-activity';
import { pollCatalog } from './page-resume';
import { DirectoryPicker, targetDirectories } from './DirectoryPicker';
import type { TargetDirectory } from './DirectoryPicker';
import { errorMessage, isBusy } from './types';
import type { ClientKind, NewSessionInput, RemoteClient, RemoteDevice, RemoteFile, RemoteProject, RemoteSession } from './types';

type Notify = (text: string, tone?: 'success' | 'error') => void;
type Theme = 'system' | 'light' | 'dark';
type DraftCache = Map<string, { text: string; files: RemoteFile[] }>;

function ClientMark({ kind, size = 20 }: { kind: ClientKind; size?: number }) {
  return kind === 'tiangong' ? <Sparkles size={size} /> : <CodeXml size={size} />;
}

function EmptyState({ icon, title, children, action }: { icon: ReactNode; title: string; children: ReactNode; action?: ReactNode }) {
  return <div className="empty-state"><div className="empty-mark">{icon}</div><h1>{title}</h1><div className="empty-description">{children}</div>{action}</div>;
}

function useTheme() {
  const [theme, setTheme] = useState<Theme>(() => {
    const saved = localStorage.getItem('hub-remote-theme');
    return saved === 'light' || saved === 'dark' ? saved : 'system';
  });
  const [dark, setDark] = useState(false);
  useEffect(() => {
    const media = matchMedia('(prefers-color-scheme: dark)');
    const update = () => {
      const value = theme === 'dark' || (theme === 'system' && media.matches);
      document.documentElement.dataset.theme = value ? 'dark' : 'light';
      setDark(value);
    };
    update(); media.addEventListener('change', update);
    localStorage.setItem('hub-remote-theme', theme);
    return () => media.removeEventListener('change', update);
  }, [theme]);
  return { dark, theme, setTheme };
}

export default function App() {
  const [localWeb, setLocalWeb] = useState({ version: '', entry: '' });
  const [loadedEntry] = useState(() => Array.from(document.scripts).map(s => s.getAttribute('src') || '').find(s => s.startsWith('/remote/assets/index-'))?.slice('/remote/'.length) || '');
  const updateAvailable = !!(loadedEntry && localWeb.entry && loadedEntry !== localWeb.entry);

  const [devices, setDevices] = useState<RemoteDevice[]>([]);
  const [deviceId, setDeviceId] = useState(() => new URLSearchParams(location.search).get('device') || '');
  const [deviceLoading, setDeviceLoading] = useState(true);
  const [deviceError, setDeviceError] = useState('');
  const [devicesRevision, setDevicesRevision] = useState(0);
  const [clients, setClients] = useState<RemoteClient[]>([]);
  const [clientId, setClientId] = useState(() => new URLSearchParams(location.search).get('client') || '');
  const [clientLoading, setClientLoading] = useState(false);
  const [clientError, setClientError] = useState('');
  const [toasts, setToasts] = useState<ToastMessage[]>([]);
  const toastSequence = useRef(0);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const draftCaches = useRef(new Map<string, DraftCache>());
  const { dark, theme, setTheme } = useTheme();
  const notify = useCallback<Notify>((text, tone = 'success') => {
    setToasts(previous => [...previous.slice(-2), { id: ++toastSequence.current, text, tone }]);
  }, []);
  const dismiss = useCallback((id: number) => setToasts(previous => previous.filter(value => value.id !== id)), []);
  const api = useMemo(() => deviceId ? new RemoteApi(deviceId) : null, [deviceId]);
  const device = devices.find(value => value.id === deviceId);
  const client = clients.find(value => value.id === clientId) || null;
  const cacheKey = `${deviceId}\0${clientId}`;
  if (!draftCaches.current.has(cacheKey)) draftCaches.current.set(cacheKey, new Map());
  const chooseClient = (id: string) => {
    const url = new URL(location.href); url.searchParams.delete('session'); history.replaceState(null, '', url);
    setClientId(id);
  };

  useEffect(() => {
    return pollCatalog(async signal => {
      try {
        const result = await getDevices(signal);
        if (signal.aborted) return;
        setDevices(result.devices);
        setLocalWeb({ version:result.hubVersion || '', entry:result.webEntry || '' });
        setDeviceId(previous => previous || result.devices.find(value => value.paired && value.online && value.id !== result.localDeviceId)?.id || result.devices.find(value => value.paired && value.online)?.id || '');
        setDeviceError('');
      } catch (failure) {
        if (signal.aborted) return;
        setDeviceError(errorMessage(failure));
        if (accessEnded(failure)) { setDevices([]); return false; }
      }
      finally { if (!signal.aborted) setDeviceLoading(false); }
    });
  }, [devicesRevision]);

  const canQueryClients = !!api && (device ? device.online && device.paired : deviceLoading);
  useEffect(() => {
    setClients([]); setClientError('');
    if (!api || !canQueryClients) { setClientLoading(false); return; }
    let first = true;
    setClientLoading(true);
    return pollCatalog(async signal => {
      try {
        const result = await api.clients(signal);
        if (signal.aborted) return;
        setClients(result.clients);
        if (first) setClientId(previous => {
          if (result.clients.some(value => value.id === previous)) return previous;
          if (previous) { const url = new URL(location.href); url.searchParams.delete('session'); history.replaceState(null, '', url); }
          return result.clients.find(value => value.ready)?.id || result.clients[0]?.id || '';
        });
        first = false;
        setClientError('');
      } catch (failure) {
        if (signal.aborted) return;
        setClientError(errorMessage(failure));
        if (accessEnded(failure)) { setClients([]); return false; }
      }
      finally { if (!signal.aborted) setClientLoading(false); }
    }, 30000);
  }, [api, canQueryClients]);

  useEffect(() => {
    const url = new URL(location.href);
    if (deviceId) url.searchParams.set('device', deviceId); else url.searchParams.delete('device');
    if (clientId) url.searchParams.set('client', clientId); else url.searchParams.delete('client');
    history.replaceState(null, '', url);
  }, [deviceId, clientId]);

  return <div className={`app ${client?.kind || 'codex'} ${sidebarOpen ? 'sidebar-open' : ''}`}>
    <header className="app-bar">
      <div className="app-brand" title={localWeb.version ? `访问端 Hub ${localWeb.version} · ${loadedEntry || localWeb.entry}` : undefined}><span className="hub-mark"><Network size={19} /></span><span>天才 Hub <span className="brand-divider">/</span> <strong>远程会话</strong></span></div>
      <div className="app-bar-controls">
        <label className="device-picker"><Laptop size={16} /><select aria-label="目标设备" value={deviceId} onChange={event => { const url = new URL(location.href); url.searchParams.delete('session'); url.searchParams.delete('client'); history.replaceState(null, '', url); setDeviceId(event.target.value); setClientId(''); }}>
          {!deviceId && <option value="">选择设备</option>}
          {deviceId && !device && <option value={deviceId}>目标设备暂不可用</option>}
          {devices.map(value => <option key={value.id} value={value.id} disabled={!value.online || !value.paired}>{value.name}{!value.paired ? ' · 待授权' : !value.online ? ' · 离线' : ''}</option>)}
        </select><ChevronDown size={13} /></label>
        <IconButton label="刷新设备" onClick={() => setDevicesRevision(value => value + 1)}><RefreshCw size={16} /></IconButton>
        <IconButton label={theme === 'system' ? '主题：跟随系统；点击切换浅色' : theme === 'light' ? '主题：浅色；点击切换深色' : '主题：深色；点击跟随系统'} onClick={() => setTheme(theme === 'system' ? 'light' : theme === 'light' ? 'dark' : 'system')}>{theme === 'system' ? <Monitor size={17} /> : dark ? <Moon size={17} /> : <Sun size={17} />}</IconButton>
      </div>
    </header>
    {updateAvailable && <div className="connection-note" role="status"><span>访问端 Hub 已更新。请先保存草稿，再重新载入网页。</span><button type="button" className="text-button" onClick={() => location.reload()}>重新载入</button></div>}
    {((deviceError && devices.length > 0) || (clientError && clients.length > 0)) && <div className="connection-note" role="status"><span>{deviceError || clientError}</span><span>正在保留当前会话，等待连接恢复。</span></div>}
    {deviceLoading ? <div className="initial-loading"><Spinner /><span>正在连接本机 Hub…</span></div> : deviceError && !devices.length ?
      <EmptyState icon={<WifiOff size={32} />} title="暂时无法连接本机 Hub" action={<button className="button primary" onClick={() => setDevicesRevision(value => value + 1)}><RefreshCw size={16} />重新连接</button>}><p>{deviceError}</p><p>请确认本机 Hub 仍在运行，并从“远程连接”重新打开网页。</p></EmptyState> :
      !device ? <EmptyState icon={<Network size={32} />} title="连接你的另一台电脑"><p>在两台电脑的 Hub 中打开“远程连接”，加入同一个网络并完成设备授权。</p><p>设备就绪后，可以从右上角选择并继续原来的会话。</p></EmptyState> :
      !device.online || !device.paired ? <EmptyState icon={<WifiOff size={32} />} title={!device.paired ? '等待目标电脑授权' : '目标电脑已离线'}><p>{device.name}</p><p>{!device.paired ? '请在目标 Hub 中确认本次设备连接。' : '请在目标电脑打开 Hub 和对应客户端。连接恢复后会自动更新。'}</p></EmptyState> :
      clientLoading ? <div className="initial-loading"><Spinner /><span>正在读取目标客户端…</span></div> : clientError && !clients.length ?
        <EmptyState icon={<WifiOff size={32} />} title="暂时无法读取客户端"><p>{clientError}</p></EmptyState> :
        !clients.length ? <EmptyState icon={<CodeXml size={34} />} title="目标客户端尚未接入"><p>请在 {device.name} 上启动 Codex 或天工 Work，并完成 Hub 接入。</p></EmptyState> :
          <Workspace key={`${deviceId}/${clientId}`} api={api!} client={client} clients={clients} setClientId={chooseClient} device={device} notify={notify} sidebarOpen={sidebarOpen} setSidebarOpen={setSidebarOpen} draftCache={draftCaches.current.get(cacheKey)!} />}
    <Toasts messages={toasts} dismiss={dismiss} />
  </div>;
}

function Workspace({ api, client: summary, clients, setClientId, device, notify, sidebarOpen, setSidebarOpen, draftCache }: {
  api: RemoteApi; client: RemoteClient | null; clients: RemoteClient[]; setClientId: (id: string) => void; device: RemoteDevice; notify: Notify;
  sidebarOpen: boolean; setSidebarOpen: (value: boolean) => void;
  draftCache: DraftCache;
}) {
  const [modelDetail, setModelDetail] = useState<Awaited<ReturnType<RemoteApi['models']>> | null>(null);
  const [modelError, setModelError] = useState('');
  const [loadingModels, setLoadingModels] = useState(false);
  const modelsRequest = useRef<AbortController | null>(null);
  useEffect(() => () => modelsRequest.current?.abort(), []);
  const client = summary && summary.modelsDeferred && modelDetail ? { ...summary, modelsDeferred:false, models:modelDetail.models,
    capabilities:{...summary.capabilities, modelSelection:modelDetail.modelSelection, effortSelection:modelDetail.effortSelection} } : summary;
  const loadModels = async () => {
    if (!summary?.ready || !summary.modelsDeferred || modelDetail || modelsRequest.current) return;
    const request = new AbortController(); modelsRequest.current = request;
    setLoadingModels(true); setModelError('');
    try { const detail = await api.models(summary.id, request.signal); if (!request.signal.aborted) setModelDetail(detail); }
    catch (failure) { if (!request.signal.aborted) setModelError(errorMessage(failure)); }
    finally { if (!request.signal.aborted) setLoadingModels(false); modelsRequest.current = null; }
  };
  const [needProjects, setNeedProjects] = useState(false);
  const [projectRevision, setProjectRevision] = useState(0);
  const refreshProjects = () => { setNeedProjects(true); setProjectRevision(value=>value+1); };
  const [sessions, setSessions] = useState<RemoteSession[]>([]);
  const [projects, setProjects] = useState<RemoteProject[]>([]);
  const [sessionId, setSessionId] = useState<string | null>(() => new URLSearchParams(location.search).get('session'));
  const [nextCursor, setNextCursor] = useState<string | null | undefined>();
  const [nextProjectCursor, setNextProjectCursor] = useState<string | null | undefined>();
  const [loadingMoreProjects, setLoadingMoreProjects] = useState(false);
  const [projectError, setProjectError] = useState('');
  const [refreshingProjects, setRefreshingProjects] = useState(false);
  const projectPagesLoaded = useRef(false);
  const sessionPagesLoaded = useRef(false);
  const [loading, setLoading] = useState(true);
  const [listError, setListError] = useState('');
  const [loadingMore, setLoadingMore] = useState(false);
  const [search, setSearch] = useState('');
  const [projectId, setProjectId] = useState('');
  const [creating, setCreating] = useState(false);
  const [revision, setRevision] = useState(0);
  const clientId = client?.id;
  const ready = !!client?.ready;
  const canHistory = !!client?.capabilities.history;
  const session = sessions.find(value => value.id === sessionId);
  const refreshList = useCallback(() => setRevision(value => value + 1), []);

  useEffect(() => {
    const abort = new AbortController();
    if (!clientId || !ready || (!canHistory && !client?.capabilities.create)) { setLoading(false); return () => abort.abort(); }
    setListError('');
    if (!canHistory) { setLoading(false); return () => abort.abort(); }
    void api.sessions(clientId, null, abort.signal).then(page => {
      if (abort.signal.aborted) return;
      setSessions(previous => sessionPagesLoaded.current ? mergeSessions(page.sessions, previous) : page.sessions);
      if (!sessionPagesLoaded.current) setNextCursor(page.nextCursor);
      setSessionId(previous => previous || page.sessions[0]?.id || null);
    }).catch(failure=>{ if (!abort.signal.aborted) setListError(errorMessage(failure)); })
      .finally(()=>{ if (!abort.signal.aborted) setLoading(false); });
    return () => abort.abort();
  }, [api, clientId, ready, canHistory, client?.capabilities.create, revision]);

  useEffect(() => {
    if (!clientId || !ready || !needProjects) return;
    const abort = new AbortController();
    setProjectError(''); setRefreshingProjects(true);
    void api.projects(clientId, null, abort.signal).then(page=>{
      if (abort.signal.aborted) return;
      setProjects(previous=>projectPagesLoaded.current ? mergeSessions(page.projects, previous) : page.projects);
      if (!projectPagesLoaded.current) setNextProjectCursor(page.nextCursor);
    }).catch(failure=>{ if (!abort.signal.aborted) setProjectError(errorMessage(failure)); })
      .finally(()=>{ if (!abort.signal.aborted) setRefreshingProjects(false); });
    return () => abort.abort();
  }, [api, clientId, ready, needProjects, projectRevision]);
  const startCreating = () => {
    setCreating(true);
    if (client?.kind === 'tiangong') { setNeedProjects(true); void loadModels(); }
  };
  useEffect(() => {
    const url = new URL(location.href);
    if (sessionId) url.searchParams.set('session', sessionId); else url.searchParams.delete('session');
    history.replaceState(null, '', url);
  }, [sessionId]);

  const loadMore = async () => {
    if (!clientId || !nextCursor || loadingMore) return;
    setLoadingMore(true);
    try {
      const page = await api.sessions(clientId, nextCursor);
      sessionPagesLoaded.current = true;
      setSessions(previous => mergeSessions(previous, page.sessions)); setNextCursor(page.nextCursor);
    } catch (failure) { notify(errorMessage(failure), 'error'); }
    finally { setLoadingMore(false); }
  };

  const loadMoreProjects = async () => {
    if (!clientId || !nextProjectCursor || loadingMoreProjects) return;
    setLoadingMoreProjects(true); setProjectError('');
    try {
      const page = await api.projects(clientId, nextProjectCursor);
      projectPagesLoaded.current = true;
      setProjects(previous => mergeSessions(previous, page.projects)); setNextProjectCursor(page.nextCursor);
    } catch (failure) { setProjectError(errorMessage(failure)); notify(errorMessage(failure), 'error'); }
    finally { setLoadingMoreProjects(false); }
  };

  const filtered = sessions.filter(value => (!projectId || value.projectId === projectId || value.cwd === projects.find(project => project.id === projectId)?.path) && (!search || `${value.title} ${value.cwd || ''}`.toLocaleLowerCase().includes(search.toLocaleLowerCase())));
  const groups = groupSessions(filtered);
  return <div className="workspace">
    {sidebarOpen && <button className="sidebar-scrim" aria-label="收起会话列表" onClick={() => setSidebarOpen(false)} />}
    <aside className="sidebar">
      <div className="client-tabs" role="tablist" aria-label="客户端">{clients.map(value => <button key={value.id} role="tab" aria-selected={value.id === clientId} className={`client-tab ${value.id === clientId ? 'active' : ''}`} onClick={() => setClientId(value.id)} title={value.reason || value.name}>
        <ClientMark kind={value.kind} size={17} /><span>{value.name}</span>{!value.ready && <span className="unavailable-dot" />}
      </button>)}</div>
      <div className="sidebar-actions"><button type="button" className="new-session" title={!client?.capabilities.create && client?.kind === 'tiangong' ? '请先在目标电脑使用增强启动，再新建 AI Gateway 会话' : '在目标电脑创建会话'} disabled={!client?.ready || !client.capabilities.create} onClick={startCreating}><Plus size={17} />新会话<span className="new-hint">+</span></button>{client?.ready && !client.capabilities.create && client.kind === 'tiangong' && <p className="create-note">新建会话需在目标电脑使用增强启动。</p>}</div>
      <label className="search-box"><Search size={15} /><input aria-label="搜索已加载会话" placeholder="搜索会话" value={search} onChange={event => setSearch(event.target.value)} /><span className="search-shortcut">⌕</span></label>
      {client?.ready && !needProjects && <button type="button" className="text-button project-more" onClick={() => setNeedProjects(true)}><Folder size={14} />筛选项目</button>}
      {needProjects && !projects.length && <div className="muted" role="status">{refreshingProjects ? '正在读取项目…' : projectError ? <>{projectError} <button className="text-button" onClick={refreshProjects}>重试</button></> : '没有可筛选的项目'}</div>}
      {!!projects.length && <label className="project-filter"><Folder size={15} /><select aria-label="筛选项目" value={projectId} onChange={event => setProjectId(event.target.value)}><option value="">所有项目</option>{projects.map(project => <option key={project.id} value={project.id}>{project.name}</option>)}</select><ChevronDown size={13} /></label>}
      {nextProjectCursor && <button type="button" className="load-more project-more" disabled={loadingMoreProjects} onClick={() => void loadMoreProjects()}>{loadingMoreProjects ? <Spinner /> : <ChevronDown size={13} />}加载更多项目</button>}
      <div className="session-list" aria-label="会话列表">
        {loading ? <div className="list-skeleton" aria-label="正在加载会话"><i /><i /><i /><i /></div> : listError ? <div className="list-empty"><p>{listError}</p><button className="text-button" onClick={() => setRevision(value => value + 1)}>重新加载</button></div> : !filtered.length ? <div className="list-empty">{search ? '已加载会话中没有匹配结果' : '还没有会话'}</div> :
          groups.map(group => <section className="session-group" key={group.title}><h2>{group.title}</h2>{group.sessions.map(value => <button key={value.id} className={`session-row ${value.id === sessionId ? 'selected' : ''}`} onClick={() => { setSessionId(value.id); setSidebarOpen(false); }} title={value.title || '未命名会话'}>
            <span className="session-row-main"><span className="session-name">{value.title || '未命名会话'}</span>{isBusy(value.status) && <span className="running-indicator" aria-label="执行中" />}</span>
            {value.cwd && <small>{lastPath(value.cwd)}</small>}
          </button>)}</section>)}
        {nextCursor && <button type="button" className="load-more" disabled={loadingMore} onClick={() => void loadMore()}>{loadingMore ? <Spinner /> : <ChevronDown size={14} />}加载更多会话</button>}
      </div>
      <div className="sidebar-footer"><span className="connected-dot" /><span>{device.name}</span><small>{device.connectionType === 'direct' ? '直连' : device.connectionType === 'relay' ? '中继' : '已连接'}</small></div>
    </aside>
    <main className="conversation">
      <header className="conversation-header"><div className="conversation-heading"><IconButton label="展开会话列表" className="sidebar-toggle" onClick={() => setSidebarOpen(!sidebarOpen)}><Menu size={19} /></IconButton><ClientMark kind={client?.kind || 'codex'} size={19} /><div><h1>{session?.title || client?.name || '选择客户端'}</h1><p>{session?.cwd || (client?.kind === 'tiangong' ? '任务与工作空间' : '目标电脑上的原始会话')}</p></div></div><IconButton label="刷新会话列表" onClick={() => setRevision(value => value + 1)}><RefreshCw size={16} /></IconButton></header>
      {!client || !client.ready ? <EmptyState icon={<ClientMark kind={client?.kind || 'codex'} size={35} />} title="客户端尚未就绪"><p>{client?.reason || '请在目标电脑启动客户端并完成 Hub 接入。'}</p></EmptyState> : !client.capabilities.history ?
        <EmptyState icon={<FileCode2 size={35} />} title="当前版本未开放会话读取"><p>{client.reason || '请更新目标客户端或在目标电脑检查 Hub 接入。'}</p></EmptyState> :
        !sessionId ? <EmptyState icon={<ClientMark kind={client.kind} size={38} />} title={client.kind === 'tiangong' ? '从一个任务开始' : '继续你的工作'} action={<button type="button" className="button primary" disabled={!client.capabilities.create} onClick={startCreating}><Plus size={16} />新建会话</button>}><p>选择左侧会话继续，或在目标电脑上新建会话。</p></EmptyState> :
          <SessionPane key={sessionId} api={api} client={client} sessionId={sessionId} session={session} notify={notify} draftCache={draftCache} onTurn={refreshList} loadModels={loadModels} loadingModels={loadingModels} modelError={modelError} />}
    </main>
    {creating && client && <NewSession api={api} client={client} device={device.name} projects={projects} sessions={sessions} hasMoreProjects={!!nextProjectCursor} loadingProjects={refreshingProjects || loadingMoreProjects} projectError={projectError} requestProjects={()=>setNeedProjects(true)} loadModels={loadModels} modelError={modelError} loadingModels={loadingModels} refreshProjects={refreshProjects} loadMoreProjects={() => void loadMoreProjects()} close={() => setCreating(false)} onCreated={value => { setSessions(previous => [value, ...previous.filter(item => item.id !== value.id)]); setSessionId(value.id); setCreating(false); setSidebarOpen(false); }} />}
  </div>;
}

function lastPath(value: string) { return value.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || value; }

function groupSessions(sessions: RemoteSession[]): { title: string; sessions: RemoteSession[] }[] {
  const today = new Date(); today.setHours(0, 0, 0, 0);
  const groups = new Map<string, RemoteSession[]>();
  for (const session of sessions) {
    const value = typeof session.updatedAt === 'string' && /^\d+$/.test(session.updatedAt) ? Number(session.updatedAt) : session.updatedAt;
    const raw = typeof value === 'number' && value < 1e12 ? value * 1000 : value;
    const time = new Date(raw).getTime();
    const title = time >= today.getTime() ? '今天' : time >= today.getTime() - 86400000 ? '昨天' : '之前的会话';
    groups.set(title, [...(groups.get(title) || []), session]);
  }
  return [...groups].map(([title, values]) => ({ title, sessions: values }));
}

function NewSession({ requestProjects, loadModels, modelError, loadingModels, api, client, device, projects, sessions, hasMoreProjects, loadingProjects, projectError, refreshProjects, loadMoreProjects, close, onCreated }: { requestProjects:()=>void; loadModels:()=>void; modelError:string; loadingModels:boolean; api: RemoteApi; client: RemoteClient; device: string; projects: RemoteProject[]; sessions: RemoteSession[]; hasMoreProjects: boolean; loadingProjects: boolean; projectError: string; refreshProjects: () => void; loadMoreProjects: () => void; close: () => void; onCreated: (session: RemoteSession) => void }) {
  const [title, setTitle] = useState('');
  const [projectId, setProjectId] = useState(projects[0]?.id || '');
  const [directory, setDirectory] = useState<TargetDirectory>();
  const [picking, setPicking] = useState(false);
  const directories = useMemo(() => client.kind === 'codex' ? targetDirectories(projects, sessions) : [], [client.kind, projects, sessions]);
  const currentDir = directory && directories.find(d => d.path === directory.path);
  const [model, setModel] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => { if (!projectId && projects.length) setProjectId(projects[0].id); }, [projectId, projects]);
  const create = async () => {
    if (busy || (client.kind === 'tiangong' ? !projectId || !model : !currentDir)) return;
    setBusy(true); setError('');
    const value: NewSessionInput = {};
    if (title.trim()) value.title = title.trim();
    if (client.kind === 'tiangong') value.projectId = projectId;
    else if (currentDir?.projectId) value.projectId = currentDir.projectId;
    else if (currentDir) value.cwd = currentDir.path;
    if (model) value.model = model;
    try { onCreated((await api.create(client.id, value)).session); }
    catch (failure) { setError(errorMessage(failure)); setBusy(false); }
  };
  if (picking) return <DirectoryPicker device={device} choices={directories} selected={currentDir} more={hasMoreProjects} loading={loadingProjects} refresh={refreshProjects} loadMore={loadMoreProjects} error={projectError} onSelect={dir => { setDirectory(dir); setPicking(false); setError(''); }} close={() => setPicking(false)} />;
  return <Modal title="新建会话" close={() => { if (!busy) close(); }}><form onSubmit={event => { event.preventDefault(); void create(); }} className="new-session-form">
    <p className="muted">会话将保存在目标电脑的 {client.name} 中。</p>
    <label>名称 <span className="optional">可选</span><input autoFocus value={title} disabled={busy} onChange={event => setTitle(event.target.value)} placeholder="这次想完成什么？" /></label>
    {client.kind === 'codex' ? <div className="directory-field"><span>工作目录</span><button type="button" className="button secondary" disabled={busy} onClick={() => { requestProjects(); setPicking(true); }}><Folder size={16} />{currentDir ? '更换工作目录' : '选择工作目录'}</button>{currentDir && <small className="path-label">{currentDir.path}</small>}</div>
      : projects.length ? <label>项目<select value={projectId} required disabled={busy} onChange={event => setProjectId(event.target.value)}>{projects.map(project => <option key={project.id} value={project.id}>{project.name}</option>)}</select><small className="muted path-label">{projects.find(value => value.id === projectId)?.path}</small></label>
      : <p className="inline-error" role="status">{loadingProjects ? '正在读取项目…' : projectError || '请先在目标电脑创建项目，再刷新项目列表。'}{!loadingProjects && <button type="button" className="text-button" onClick={refreshProjects}>刷新项目</button>}</p>}
    {client.kind === 'tiangong' && hasMoreProjects && <button type="button" className="text-button project-form-more" disabled={busy || loadingProjects} onClick={loadMoreProjects}>{loadingProjects ? '正在读取项目…' : '加载更多项目'}</button>}
    {client.modelsDeferred && <button type="button" className="text-button" onClick={loadModels} disabled={loadingModels}>{loadingModels ? '正在读取模型…' : modelError ? '重试读取模型' : '选择模型'}</button>}
    {modelError && <p role="alert" className="inline-error">{modelError}</p>}
    {!!client.models.length && <label>模型<select value={model} required={client.kind === 'tiangong'} disabled={busy} onChange={event => setModel(event.target.value)}><option value="">{client.kind === 'tiangong' ? '请选择模型' : '沿用客户端配置'}</option>{client.models.map(value => <option key={value.id} value={value.id}>{value.name || value.id}</option>)}</select></label>}
    {error && <p className="inline-error" role="alert">{error}</p>}
    <div className="modal-footer"><button type="button" className="button secondary" disabled={busy} onClick={close}>取消</button><button type="submit" className="button primary" disabled={busy || (client.kind === 'tiangong' ? !model || !projectId : !currentDir)}>{busy ? <Spinner /> : <Plus size={16} />}创建会话</button></div>
  </form></Modal>;
}

function SessionPane({ api, client, sessionId, session, notify, draftCache, onTurn, loadModels, loadingModels, modelError }: { loadModels:()=>void; loadingModels:boolean; modelError:string; api: RemoteApi; client: RemoteClient; sessionId: string; session?: RemoteSession; notify: Notify; draftCache: DraftCache; onTurn: () => void }) {
  const { chat, setChat, connection, loading, hasSnapshot, loadingEarlier, error, refresh, earlier, displayManaged, activityRows, artifactDisplay } = useSession(api, client, sessionId);
  const [showArtifacts, setShowArtifacts] = useState(false);
  const initial = draftCache.get(sessionId);
  const [draft, setDraft] = useState(initial?.text || '');
  const [files, setFiles] = useState<RemoteFile[]>(initial?.files || []);
  const [uploading, setUploading] = useState<string[]>([]);
  const [removing, setRemoving] = useState<string[]>([]);
  const [sending, setSending] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [selectedModel, setSelectedModel] = useState('');
  const [effort, setEffort] = useState('');
  const [preview, setPreview] = useState<RemoteFile | null>(null);
  const [following, setFollowing] = useState(true);
  const [dragging, setDragging] = useState(false);
  const transcript = useRef<HTMLDivElement>(null);
  const editor = useRef<HTMLTextAreaElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const composing = useRef(false);
  const uploadAbort = useRef(new AbortController());
  const activeDownloads = useRef(new Set<string>());
  const download = useCallback(async (file: RemoteFile) => {
    const signal = uploadAbort.current.signal;
    if (signal.aborted || activeDownloads.current.has(file.id)) return;
    activeDownloads.current.add(file.id);
    try { await saveRemoteFile(api, client.id, sessionId, file, signal); }
    catch (failure) { if (!signal.aborted) notify(errorMessage(failure), 'error'); }
    finally { activeDownloads.current.delete(file.id); }
  }, [api, client.id, sessionId, notify]);
  const sessionFiles = useMemo(() => ({ clientId: client.id, sessionId, handles:artifactDisplay.handles, download }),
    [client.id, sessionId, artifactDisplay, artifactDisplay.handleVersion, download]);
  const beforeInsert = useRef<{ height: number; top: number; first?: HTMLElement; y?: number } | null>(null);
  const touchY = useRef(0);
  const mounted = useRef(true);
  const sendInFlight = useRef(false);
  const working = isBusy(chat.status) || chat.approvals.length > 0;
  const availableModel = client.models.find(value => value.id === (selectedModel || session?.model));
  const efforts = client.capabilities.effortSelection ? availableModel?.efforts || [] : [];
  const canSend = client.capabilities.send && connection === 'live' && !loading && !working && !sending && !uploading.length && !removing.length && (!!draft.trim() || files.length > 0);
  const messages = useMemo(() => chat.items.filter(isTranscriptMessage).map(item => artifactDisplay.forTranscript(item)).filter(item => item !== null), [chat.items, artifactDisplay]);
  const timeline = useMemo(() => withActivity(messages, activityRows), [messages, activityRows]);
  const checkedFirstPage = useRef(false);
  useEffect(() => {
    if (!hasSnapshot || checkedFirstPage.current) return;
    checkedFirstPage.current = true;
    if (!messages.length && chat.nextCursor) void earlier(() => {});
  }, [earlier, hasSnapshot, messages.length, chat.nextCursor]);

  useEffect(() => {
    return () => { mounted.current = false; uploadAbort.current.abort(); };
  }, []);
  useEffect(() => { draftCache.set(sessionId, { text: draft, files }); }, [draftCache, sessionId, draft, files]);
  useLayoutEffect(() => {
    const element = editor.current;
    if (!element) return;
    element.style.height = 'auto'; element.style.height = `${Math.min(element.scrollHeight, 220)}px`;
  }, [draft]);
  useLayoutEffect(() => {
    const element = transcript.current;
    if (!element) return;
    if (beforeInsert.current) {
      const anchor = beforeInsert.current;
      if (anchor.first?.isConnected && anchor.y !== undefined)
        element.scrollTop += anchor.first.getBoundingClientRect().top - anchor.y;
      else element.scrollTop = anchor.top + element.scrollHeight - anchor.height;
      beforeInsert.current = null;
    } else if (following) element.scrollTop = element.scrollHeight;
  }, [chat.items, chat.approvals, activityRows, following, loading]);
  useEffect(() => { if (chat.status && !isBusy(chat.status) && chat.items.length) onTurn(); }, [chat.status, onTurn]); // Refresh the sidebar on a real turn transition.

  const send = async () => {
    if (!canSend || sendInFlight.current) return;
    sendInFlight.current = true;
    const text = draft.trim();
    const sentFiles = files;
    setSending(true);
    try {
      const response = await api.send(client.id, sessionId, { text, attachmentIds: sentFiles.map(file => file.id), model: client.capabilities.modelSelection && selectedModel ? selectedModel : undefined, effort: efforts.includes(effort) ? effort : undefined, requestId: crypto.randomUUID() });
      // Clear only after acceptance; transport failures keep the draft for inspection.
      draftCache.set(sessionId, { text: '', files: [] });
      if (!mounted.current) return;
      setDraft(''); setFiles([]); setFollowing(true);
      setChat(previous => ({ ...previous, status: response.status, turnId: response.turnId }));
      refresh(); onTurn(); editor.current?.focus();
    } catch (failure) {
      if (!mounted.current) return;
      notify(errorMessage(failure), 'error');
      refresh();
    } finally { sendInFlight.current = false; if (mounted.current) setSending(false); }
  };

  const stop = async () => {
    if (stopping || !client.capabilities.stop) return;
    setStopping(true);
    try {
      const response = await api.stop(client.id, sessionId, chat.turnId);
      if (!mounted.current) return;
      setChat(previous => ({ ...previous, status: response.status })); refresh();
    } catch (failure) { if (mounted.current) notify(errorMessage(failure), 'error'); }
    finally { if (mounted.current) setStopping(false); }
  };

  const upload = async (input: FileList | File[]) => {
    if (!client.capabilities.attachments || working || sending) return;
    const chosen = Array.from(input);
    if (files.length + uploading.length + chosen.length > 20) { notify('每条消息最多添加 20 个附件。', 'error'); return; }
    setUploading(previous => [...previous, ...chosen.map(file => file.name)]);
    for (const file of chosen) {
      try {
        const result = await api.upload(client.id, sessionId, file, uploadAbort.current.signal);
        if (!mounted.current) return;
        setFiles(previous => [...previous, result]);
      } catch (failure) { if (!uploadAbort.current.signal.aborted) notify(`${file.name}：${errorMessage(failure)}`, 'error'); }
      finally { if (mounted.current) setUploading(previous => { const index = previous.indexOf(file.name); return previous.filter((_, item) => item !== index); }); }
    }
    if (fileInput.current) fileInput.current.value = '';
  };

  const loadEarlier = () => earlier(() => {
    const element = transcript.current;
    if (!element) return;
    const top = element.getBoundingClientRect().top;
    const first = Array.from(element.querySelectorAll<HTMLElement>('[data-history-item]'))
      .find(row => { const r = row.getBoundingClientRect(); return r.height > 0 && r.bottom > top; });
    beforeInsert.current = { height: element.scrollHeight, top: element.scrollTop, first, y: first?.getBoundingClientRect().top };
    setFollowing(false);
  });

  const removeFile = async (file: RemoteFile) => {
    if (sending || removing.includes(file.id)) return;
    setRemoving(previous => [...previous, file.id]);
    try { await api.removeFile(client.id, sessionId, file.id); if (mounted.current) setFiles(previous => previous.filter(value => value.id !== file.id)); }
    catch (failure) { if (mounted.current) notify(errorMessage(failure), 'error'); }
    finally { if (mounted.current) setRemoving(previous => previous.filter(value => value !== file.id)); }
  };

  const statusText = connection === 'connecting' ? '正在连接会话…' : connection === 'reconnecting' ? '正在重新连接' : connection === 'offline' ? (hasSnapshot ? '历史已读取，实时连接不可用' : '未能读取会话') : chat.status === 'unknown' ? '运行状态待核对' : chat.status === 'stopping' ? '正在停止' : chat.approvals.length ? (chat.approvals.every(a=>a.desktopOnly) ? '请在目标客户端处理' : '等待你的回复') : ['waiting_approval', 'waiting_confirmation', 'awaiting_confirmation', 'waiting_for_approval', 'waiting_for_user_input'].includes(chat.status) ? '请在目标客户端查看待处理请求' : working ? '正在处理' : '已同步';
  return <div className={`session-pane ${dragging ? 'is-dragging' : ''}`} onDragOver={event => { if (client.capabilities.attachments && event.dataTransfer.types.includes('Files')) { event.preventDefault(); setDragging(true); } }} onDragLeave={event => { if (!event.currentTarget.contains(event.relatedTarget as Node)) setDragging(false); }} onDrop={event => { event.preventDefault(); setDragging(false); if (event.dataTransfer.files.length) void upload(event.dataTransfer.files); }}>
    <div className={`session-status ${connection !== 'live' ? 'disconnected' : ''}`} role="status">{connection === 'live' ? working ? <Dots /> : <Check size={13} /> : connection === 'offline' ? <WifiOff size={13} /> : <Spinner />}<span>{statusText}</span>{connection === 'live' && <span className="native-badge">与桌面共用会话</span>}{connection === 'offline' && !error && <button type="button" className="text-button" onClick={refresh}>重试</button>}</div>
    <div className="display-content"><span className="display-source">{displayManaged ? '显示内容由会话所在电脑 Hub（发送端）的「远程连接 → 设置」统一管理。' : '请更新会话所在电脑 Hub（发送端），在「远程连接 → 设置」管理显示内容。'}</span><button className="text-button" type="button" aria-haspopup="dialog" onClick={() => setShowArtifacts(true)}>查看产物</button></div>
    {!!error && <div className="connection-note" role="alert"><span>{error}</span>{connection !== 'reconnecting' && <button type="button" className="text-button" onClick={refresh}>重新读取</button>}</div>}
    <div className="transcript" ref={transcript} tabIndex={0} role="region" aria-label="会话记录"
      onWheel={event => { if (event.deltaY < 0 && event.currentTarget.scrollTop + event.deltaY <= 96) void loadEarlier(); }}
      onTouchStart={event => { touchY.current = event.touches[0]?.clientY || 0; }}
      onTouchMove={event => { const y = event.touches[0]?.clientY ?? touchY.current; if (y > touchY.current && event.currentTarget.scrollTop <= 96) void loadEarlier(); touchY.current = y; }}
      onKeyDown={event => { if (event.target === event.currentTarget && (event.key === 'Home' || (event.key === 'PageUp' && event.currentTarget.scrollTop <= event.currentTarget.clientHeight + 96) || (event.key === 'ArrowUp' && event.currentTarget.scrollTop <= 96))) void loadEarlier(); }}
      onScroll={event => { const element = event.currentTarget; setFollowing(element.scrollHeight - element.scrollTop - element.clientHeight < 72); }}>
      <div className="transcript-content">
        {chat.nextCursor && <button className="earlier-button" disabled={loadingEarlier} onClick={() => void loadEarlier()}>{loadingEarlier ? <Spinner /> : <ChevronLeft size={14} />}更早的消息</button>}
        {loading ? <div className="message-skeleton" aria-label="正在读取会话"><i /><i /><i /></div> : !hasSnapshot ? <div className="conversation-welcome"><FileCode2 size={32} /><h2>尚未读到会话内容</h2></div> : timeline.length ? timeline.map(item => 'category' in item ? <article className="live-activity" key={`activity-${item.key}`}><strong>{activityLabels[item.category]}</strong><pre>{item.text}</pre></article> : <MessageItem key={item.id} item={item} hiddenUrls={artifactDisplay.hiddenLinksFor(item)} sessionFiles={sessionFiles} clientName={client.name} onFile={setPreview} />) : working ? null : chat.items.length || chat.nextCursor ? <div className="conversation-welcome"><p>{loadingEarlier ? '正在查找更早的消息…' : chat.nextCursor ? '这段记录没有对话正文，可以继续查看更早的消息。' : '这段记录没有对话正文。'}</p></div> : <div className="conversation-welcome"><ClientMark kind={client.kind} size={35} /><h2>{client.kind === 'tiangong' ? '这次想完成什么？' : '今天要做些什么？'}</h2><p>消息与操作会保存在目标电脑的原会话中。</p></div>}
        {working && !chat.approvals.length && !messages.some(item => isBusy(item.status || '')) && <div className="pending-answer"><span>{client.name}</span><Dots /></div>}
      </div>
    </div>
    {!following && <button className="jump-bottom" onClick={() => setFollowing(true)}><ArrowDown size={15} />回到最新消息</button>}
    {showArtifacts && !preview && <Modal title="本会话产物" close={() => setShowArtifacts(false)}>{artifactDisplay.files.size ?
      <div className="file-list">{Array.from(artifactDisplay.files.values(), file => <FileChip key={file.id} file={file} onOpen={() => setPreview(file)} onDownload={download} />)}</div> : <p>已读取的会话记录中暂无产物。</p>}</Modal>}
    {!!chat.approvals.length && <div className="approval-dock" role="region" aria-label="待处理请求">{chat.approvals.map(approval => <ApprovalCard key={approval.id} approval={approval} canRespond={connection === 'live' && !!(approval.type === 'question' ? client.capabilities.questions : client.capabilities.approvals)} onRespond={async (decision, answers) => { await api.respond(client.id, sessionId, approval.id, decision, answers); refresh(); }} />)}</div>}
    <div className="composer-dock">
      <div className={`composer ${sending ? 'is-sending' : ''}`}>
        {(files.length > 0 || uploading.length > 0) && <div className="composer-files">{files.map(file => <FileChip key={file.id} file={file} onOpen={() => setPreview(file)} onDownload={download} onRemove={sending || removing.includes(file.id) ? undefined : () => void removeFile(file)} />)}{uploading.map((name, index) => <div className="upload-chip" key={`${name}-${index}`}><Spinner /><span>{name}</span></div>)}</div>}
        <textarea ref={editor} aria-label="消息" placeholder={working ? '运行中，可以先写下一条消息…' : `向 ${client.name} 发送消息…`} rows={2} value={draft} disabled={sending || !client.capabilities.send} onChange={event => setDraft(event.target.value)} onCompositionStart={() => { composing.current = true; }} onCompositionEnd={() => { composing.current = false; }} onKeyDown={event => {
          if (shouldSendKey({ ...event, isComposing: composing.current || event.nativeEvent.isComposing, keyCode: event.nativeEvent.keyCode })) { event.preventDefault(); void send(); }
        }} />
        <div className="composer-toolbar"><div className="composer-options"><input className="hidden-input" ref={fileInput} type="file" multiple onChange={event => { if (event.target.files) void upload(event.target.files); }} /><IconButton label={client.capabilities.attachments ? '添加附件' : '当前运行端未开放附件'} disabled={!client.capabilities.attachments || working || sending || !!uploading.length} onClick={() => fileInput.current?.click()}><Paperclip size={18} /></IconButton>
          {client.kind==='codex' && client.modelsDeferred && <button type="button" className="text-button" disabled={working || sending || loadingModels} onClick={loadModels}>{loadingModels ? '正在读取模型…' : '选择本轮模型'}</button>}
          {modelError && <small role="alert">{modelError}</small>}
          {client.capabilities.modelSelection && client.models.length > 0 && <label className="model-picker"><select aria-label="本轮模型" value={selectedModel} onChange={event => { setSelectedModel(event.target.value); setEffort(''); }} disabled={working || sending}><option value="">{session?.model || '沿用当前模型'}</option>{client.models.map(model => <option key={model.id} value={model.id}>{model.name || model.id}</option>)}</select><ChevronDown size={12} /></label>}
          {!!efforts.length && <label className="effort-picker"><select aria-label="思考强度" value={effort} disabled={working || sending} onChange={event => setEffort(event.target.value)}><option value="">默认强度</option>{efforts.map(value => <option key={value} value={value}>{value}</option>)}</select><ChevronDown size={12} /></label>}
        </div>{working ? <IconButton label={client.capabilities.stop ? '停止执行' : '当前运行端未开放停止，请在目标电脑处理'} className="stop-button" disabled={!client.capabilities.stop || stopping || connection !== 'live'} onClick={() => void stop()}>{stopping ? <Spinner /> : <Square size={15} fill="currentColor" />}</IconButton> : <IconButton label="发送消息" className="send-button" disabled={!canSend} onClick={() => void send()}>{sending ? <Spinner /> : <ArrowUp size={20} />}</IconButton>}</div>
      </div>
      <div className="composer-hint"><span>{!client.capabilities.send ? '当前运行端未开放发送' : working ? '任务在目标电脑执行，关闭网页后仍可继续。' : '权限与模型配置沿用目标客户端。'}</span><kbd>{/Mac/.test(navigator.platform) ? '⌘' : 'Ctrl'} ↵ 发送</kbd></div>
    </div>
    {dragging && <div className="drop-overlay"><Paperclip size={30} /><strong>将文件添加到消息</strong></div>}
    {preview && <FilePreview key={preview.id} api={api} clientId={client.id} sessionId={sessionId} file={preview} close={() => setPreview(null)} download={download} />}
  </div>;
}
