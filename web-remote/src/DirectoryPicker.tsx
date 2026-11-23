import { useState } from 'react';
import { Check, Folder, RefreshCw, Search } from 'lucide-react';
import { IconButton, Modal } from './components';
import type { RemoteProject, RemoteSession } from './types';

export interface TargetDirectory { path: string; name: string; projectId?: string }

function directoryKey(path: string) {
  // Windows paths arrive from the target even when this browser runs on macOS.
  if (/^(?:[a-z]:[\\/]|\\\\)/i.test(path)) return path.replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase();
  return path === '/' ? path : path.replace(/\/+$/, '');
}

export function targetDirectories(projects: RemoteProject[], sessions: RemoteSession[]) {
  const found = new Map<string, TargetDirectory>();
  function remember(path: string | undefined, name?: string, projectId?: string) {
    if (!path || !/^(?:\/|[a-z]:[\\/]|\\\\)/i.test(path)) return;
    const key = directoryKey(path);
    if (!found.has(key)) found.set(key, { name: name || path.split(/[\\/]/).filter(Boolean).at(-1) || path, path, projectId });
  }
  projects.forEach(p => remember(p.path, p.name, p.id));
  sessions.forEach(s => remember(s.cwd));
  return [...found.values()];
}

interface Props {
  device: string; choices: TargetDirectory[]; selected?: TargetDirectory;
  more: boolean; loading: boolean; refresh: () => void; loadMore: () => void; error: string;
  onSelect: (choice: TargetDirectory) => void; close: () => void;
}
export function DirectoryPicker({ device, choices, selected, more, loading, refresh, loadMore, error, onSelect, close }: Props) {
  const [search, setSearch] = useState('');
  const matches = choices.filter(dir => (dir.name + '\n' + dir.path).toLocaleLowerCase().includes(search.trim().toLocaleLowerCase()));
  return <Modal title="选择工作目录" close={close}>
    <section className="directory-picker">
      <div className="directory-origin"><span>目标电脑：<strong>{device}</strong></span><IconButton label="刷新目标目录" disabled={loading} onClick={refresh}><RefreshCw size={16} /></IconButton></div>
      <label className="search-box"><Search size={15} /><input autoFocus type="search" aria-label="搜索目标目录" placeholder="搜索目录名称或路径" value={search} onChange={e => setSearch(e.target.value)} /></label>
      <div className="directory-list" aria-label="可选工作目录" aria-busy={loading}>
        {matches.map(dir => <button type="button" className="directory-row" key={directoryKey(dir.path)} onClick={() => onSelect(dir)}>
          <Folder size={18} /><span><b>{dir.name}</b><small>{dir.path}</small></span>{selected && directoryKey(selected.path) === directoryKey(dir.path) && <Check size={16} aria-label="当前选择" />}
        </button>)}
        {!matches.length && <p role="status" className="muted">{loading ? '正在读取目标目录…' : search ? '已加载的目录里没有匹配项。' : '还没有可选的目标目录。'}</p>}
      </div>
      {more && <button type="button" className="text-button" disabled={loading} onClick={loadMore}>{loading ? '正在读取…' : '加载更多目录'}</button>}
      {error && <p className="inline-error" role="alert">{error}</p>}
      <p className="directory-help">添加新目录：在「{device}」的 Hub → 远程连接 → 添加工作目录，然后刷新此列表。</p>
    </section>
    <div className="modal-footer"><button type="button" className="button secondary" onClick={close}>取消选择</button></div>
  </Modal>;
}
