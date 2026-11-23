import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { RemoteApi } from '../src/api';
import { FileChip, Markdown } from '../src/components';
import { saveRemoteFile } from '../src/file-download';
import type { SessionFiles } from '../src/file-download';
import type { RemoteFile } from '../src/types';

const clientId = 'codex:工作', sessionId = '会话 A';
const file: RemoteFile = { id: 'file-1', name: '季度 报告.xlsx', size: 4, mimeType: 'application/octet-stream', kind:'artifact' };
const url = '/api/remote/v1/clients/'+encodeURIComponent(clientId)+'/sessions/'+encodeURIComponent(sessionId)+'/files/'+file.id;
const scope = (download: (file: RemoteFile) => void): SessionFiles => ({clientId, sessionId, handles:new Map([[file.id, file]]), download});

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe('remote message file downloads', () => {
  it('renders local, cleared, sandbox and unregistered session URLs as readable text, and keeps ordinary external links', () => {
    const download = vi.fn();
    const examples = ['D:/work/%E6%8A%A5%E5%91%8A%20A.pdf','sandbox:/mnt/data/a.pdf','./report.pdf','',url.replace('file-1','unknown'),url.replace('file-1','%'),url.replace(encodeURIComponent(sessionId),'other')];
    const text = examples.map((value,i) => `[文件${i}](${value})`).join('\n\n') + '\n\n[网上文档](https://example.com/docs)';
    const view = render(<Markdown files={scope(download)} text={text} />);
    expect(view.container.querySelectorAll('a')).toHaveLength(1);
    expect(screen.getByRole('link', {name:'网上文档'}).getAttribute('href')).toBe('https://example.com/docs');
    expect(screen.getAllByText('（当前会话未提供可下载链接）')).toHaveLength(examples.length);
    expect(download).not.toHaveBeenCalled();
  });
  it('downloads the registered handle from the active device and saves the returned bytes using the original Chinese filename', async () => {
    const read = vi.fn(async (_path: RequestInfo | URL, init?: RequestInit) =>
      new Response(new Uint8Array([0,255,9,new Headers(init?.headers).get('X-Hub-Target') === 'B' ? 2 : 1]),
        {headers:{'Content-Disposition':'attachment','Content-Type':file.mimeType}}));
    vi.stubGlobal('fetch', read);
    const create = vi.fn((_blob:Blob) => 'blob:remote-result');
    const BrowserUrl = URL;
    class TestUrl extends BrowserUrl { static override createObjectURL = create; static override revokeObjectURL = vi.fn(); }
    vi.stubGlobal('URL',TestUrl);
    const saved: string[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function(this:HTMLAnchorElement) {saved.push(this.download);});
    const apiA = new RemoteApi('A'), apiB = new RemoteApi('B');
    const downloadA = (value:RemoteFile) => saveRemoteFile(apiA,clientId,sessionId,value);
    const downloadB = (value:RemoteFile) => saveRemoteFile(apiB,clientId,sessionId,value);
    const preview = vi.fn();
    const view = render(<Markdown files={scope(downloadA)} text={`[下载报告](${url})`}/>);
    fireEvent.click(screen.getByRole('button',{name:'下载报告'}));
    await waitFor(()=>expect(saved).toHaveLength(1));
    view.rerender(<FileChip file={file} onDownload={downloadB} onOpen={preview}/>);
    fireEvent.click(screen.getByRole('button',{name:`下载 ${file.name}`}));
    await waitFor(()=>expect(saved).toHaveLength(2));
    expect(preview).not.toHaveBeenCalled();
    expect(saved).toEqual([file.name,file.name]);
    expect(read.mock.calls.map(([path,init])=>[path,new Headers(init?.headers).get('X-Hub-Target')])).toEqual([[url+'?download=1','A'],[url+'?download=1','B']]);
    const bytes = await Promise.all(create.mock.calls.map(([blob]) => new Promise<number[]>((resolve,reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(Array.from(new Uint8Array(reader.result as ArrayBuffer)));
      reader.onerror = () => reject(reader.error);
      reader.readAsArrayBuffer(blob);
    })));
    expect(bytes).toEqual([[0,255,9,1],[0,255,9,2]]);
    fireEvent.click(screen.getByRole('button',{name:`预览 ${file.name}`}));
    expect(preview).toHaveBeenCalledOnce();
    expect(read).toHaveBeenCalledTimes(2);
  });
  it('surfaces disappearing files and unexpected webpage responses without starting a browser save', async () => {
    const click = vi.spyOn(HTMLAnchorElement.prototype,'click');
    const read = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify({error:{message:'关联文件已移除'}}),{status:409}))
      .mockResolvedValueOnce(new Response('<html>网页</html>',{headers:{'Content-Type':'text/html'}}));
    vi.stubGlobal('fetch', read);
    const api = new RemoteApi('A');
    await expect(saveRemoteFile(api,clientId,sessionId,file)).rejects.toThrow('关联文件已移除');
    await expect(saveRemoteFile(api,clientId,sessionId,file)).rejects.toThrow('没有返回下载文件');
    expect(click).not.toHaveBeenCalled();
  });
});
