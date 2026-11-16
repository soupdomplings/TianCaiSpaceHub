import { memo, useEffect, useId, useRef, useState } from 'react';
import type { ButtonHTMLAttributes, ReactNode } from 'react';
import { Check, CheckCircle2, ChevronDown, Copy, Download, File, FileCode2, FileImage, FileText, LoaderCircle, Terminal, X, XCircle } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { errorMessage, isBusy } from './types';
import type { ApprovalDecision, QuestionAnswers, RemoteApproval, RemoteFile, RemoteItem } from './types';
import type { RemoteApi } from './api';

export function IconButton({ label, children, className = '', ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { label: string; children: ReactNode }) {
  return <button {...props} type="button" className={`icon-button ${className}`} aria-label={label} title={label}>{children}</button>;
}

export function Spinner({ label = '正在加载' }: { label?: string }) {
  return <LoaderCircle className="spin" size={17} aria-label={label} />;
}

export function Dots() {
  return <span className="thinking-dots" aria-label="正在回复"><i /><i /><i /></span>;
}

export interface ToastMessage { id: number; text: string; tone: 'success' | 'error' }

export function Toasts({ messages, dismiss }: { messages: ToastMessage[]; dismiss: (id: number) => void }) {
  return <div className="toasts" aria-live="polite">{messages.map(message => <Toast key={message.id} message={message} dismiss={dismiss} />)}</div>;
}

function Toast({ message, dismiss }: { message: ToastMessage; dismiss: (id: number) => void }) {
  useEffect(() => {
    const timer = window.setTimeout(() => dismiss(message.id), 6500);
    return () => clearTimeout(timer);
  }, [message.id, dismiss]);
  return <div className={`toast ${message.tone}`} role={message.tone === 'error' ? 'alert' : 'status'}>
    {message.tone === 'success' ? <CheckCircle2 size={20} /> : <XCircle size={20} />}
    <span>{message.text}</span>
    <IconButton label="关闭提示" onClick={() => dismiss(message.id)}><X size={16} /></IconButton>
    <span className="toast-timer" />
  </div>;
}

export function CopyButton({ text, label = '复制' }: { text: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  const [failed, setFailed] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => { if (timer.current) clearTimeout(timer.current); }, []);
  return <IconButton label={failed ? '复制失败，请选择文字复制' : copied ? '已复制' : label} onClick={async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true); setFailed(false);
      timer.current = setTimeout(() => setCopied(false), 1800);
    } catch { setFailed(true); }
  }}>{copied ? <Check size={15} /> : <Copy size={15} />}</IconButton>;
}

const gfm = [remarkGfm];

export function Markdown({ text }: { text: string }) {
  return <div className="markdown"><ReactMarkdown remarkPlugins={gfm} components={{
    a: ({ children, href }) => <a href={href} target="_blank" rel="noreferrer noopener">{children}</a>,
    img: ({ alt, src }) => <a href={src} target="_blank" rel="noreferrer noopener">{alt || '查看图片链接'}</a>,
    pre: ({ children }) => <div className="code-block"><pre>{children}</pre></div>,
    table: ({ children }) => <div className="table-scroll"><table>{children}</table></div>,
  }}>{text}</ReactMarkdown></div>;
}

function fileIcon(file: RemoteFile) {
  if (file.mimeType.startsWith('image/')) return <FileImage size={18} />;
  if (/json|javascript|typescript|python|rust|xml/.test(file.mimeType) || /\.(rs|py|js|tsx?|jsx|sh|toml|json|ya?ml)$/.test(file.name)) return <FileCode2 size={18} />;
  return <FileText size={18} />;
}

export function fileSize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function FileChip({ file, onOpen, onRemove }: { file: RemoteFile; onOpen?: () => void; onRemove?: () => void }) {
  return <div className="file-chip">
    <button type="button" className="file-chip-main" onClick={onOpen} disabled={!onOpen}>
      {fileIcon(file)}<span><strong>{file.name}</strong><small>{fileSize(file.size)}{file.kind === 'artifact' ? ' · 产物' : ''}</small></span>
    </button>
    {onRemove && <IconButton label={`移除附件 ${file.name}`} onClick={onRemove}><X size={14} /></IconButton>}
  </div>;
}

function ToolItem({ item, onFile }: { item: RemoteItem; onFile: (file: RemoteFile) => void }) {
  const busy = isBusy(item.status || '');
  const title = String(item.data?.title || item.data?.name || item.data?.toolName || item.data?.tool || (item.type === 'fileChange' ? '文件变更' : item.type === 'reasoning' ? '思考过程' : '工具执行'));
  const command = typeof item.data?.command === 'string' ? item.data.command : '';
  const diff = typeof item.data?.diff === 'string' ? item.data.diff : '';
  const changes = Array.isArray(item.data?.changes) ? item.data.changes as { path?: string; kind?: string; diff?: string }[] : [];
  return <details className={`tool-card ${busy ? 'is-running' : ''}`}>
    <summary>{busy ? <Spinner label="工具执行中" /> : item.type === 'fileChange' ? <FileCode2 size={16} /> : <Terminal size={16} />}
      <strong>{title}</strong><span className="tool-state">{busy ? '执行中' : ['failed', 'error'].includes(item.status || '') ? '失败' : '查看详情'}</span><ChevronDown size={15} />
    </summary>
    <div className="tool-body">
      {command && <pre className="command">{command}</pre>}
      {diff && <pre className="diff">{diff}</pre>}
      {changes.map((change, index) => <div className="file-change" key={`${change.path || ''}-${index}`}>
        <div className="file-change-name"><FileCode2 size={14} /><span>{change.path || '文件变更'}</span>{change.kind && <small>{change.kind}</small>}</div>
        {change.diff && <pre className="diff">{change.diff}</pre>}
      </div>)}
      {typeof item.data?.exitCode === 'number' && <p className="muted tool-exit">退出码：{item.data.exitCode}</p>}
      {item.text && <Markdown text={item.text} />}
      {!item.text && !command && !diff && !changes.length && <p className="muted">运行端尚未提供详细内容。</p>}
      {!!item.files?.length && <div className="file-list">{item.files.map(file => <FileChip key={file.id} file={file} onOpen={() => onFile(file)} />)}</div>}
    </div>
  </details>;
}

export const MessageItem = memo(function MessageItem({ item, clientName, onFile }: { item: RemoteItem; clientName: string; onFile: (file: RemoteFile) => void }) {
  if (item.type !== 'message' || item.role === 'tool') return <ToolItem item={item} onFile={onFile} />;
  return <article className={`message ${item.role}`}>
    <div className="message-meta"><span>{item.role === 'user' ? '你' : item.role === 'system' ? '系统' : clientName}</span><CopyButton text={item.text} label="复制消息" /></div>
    <div className="message-content">
      {item.role === 'user' ? <p className="user-text">{item.text}</p> : <Markdown text={item.text} />}
      {!!item.files?.length && <div className="file-list">{item.files.map(file => <FileChip key={file.id} file={file} onOpen={() => onFile(file)} />)}</div>}
      {isBusy(item.status || '') && <Dots />}
    </div>
  </article>;
});

export function ApprovalCard({ approval, canRespond, onRespond }: {
  approval: RemoteApproval; canRespond: boolean;
  onRespond: (decision: ApprovalDecision, answers?: QuestionAnswers) => Promise<void>;
}) {
  const [answers, setAnswers] = useState<QuestionAnswers>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const id = useId();
  const respond = async (decision: ApprovalDecision) => {
    if (busy || !canRespond) return;
    setBusy(true); setError('');
    try { await onRespond(decision, answers); }
    catch (failure) { setError(errorMessage(failure)); }
    finally { setBusy(false); }
  };
  const unanswered = approval.questions?.some(question => {
    const value = answers[question.id];
    return question.required !== false && (!value || (Array.isArray(value) ? !value.length : !value.trim()));
  });
  return <section className="approval-card" aria-label={approval.title}>
    <div className="approval-heading"><span className="approval-indicator" /><strong>{approval.title || (approval.type === 'question' ? '需要你补充信息' : '需要你的确认')}</strong></div>
    {approval.message && <Markdown text={approval.message} />}
    {approval.questions?.map((question, index) => <fieldset key={question.id} className="question">
      <legend>{question.prompt}</legend>
      {!!question.options?.length && <div className="answer-options">{question.options.map(option => {
        const value = answers[question.id];
        const checked = Array.isArray(value) ? value.includes(option.value) : value === option.value;
        return <label key={option.value} className={checked ? 'answer selected' : 'answer'}>
          <input type={question.multiple ? 'checkbox' : 'radio'} name={`${id}-${question.id}`} value={option.value} checked={checked} disabled={busy || !canRespond} onChange={() => setAnswers(previous => {
            if (!question.multiple) return { ...previous, [question.id]: option.value };
            const old = previous[question.id];
            const values = Array.isArray(old) ? old : [];
            return { ...previous, [question.id]: values.includes(option.value) ? values.filter(item => item !== option.value) : [...values, option.value] };
          })} />
          <span>{option.label}</span>{checked && <Check size={15} />}
        </label>;
      })}</div>}
      <label className="answer-custom" htmlFor={`${id}-${index}`}>{question.options?.length ? '或填写你的回答' : '你的回答'}</label>
      <textarea id={`${id}-${index}`} rows={2} value={Array.isArray(answers[question.id]) ? '' : answers[question.id] || ''} disabled={busy || !canRespond} onChange={event => setAnswers(previous => ({ ...previous, [question.id]: event.target.value }))} />
    </fieldset>)}
    {error && <p className="inline-error" role="alert">{error}</p>}
    {!canRespond && <p className="muted">当前运行端未开放此操作，请在目标电脑处理。</p>}
    <div className="approval-actions">
      {approval.type !== 'question' && approval.choices?.length ? approval.choices.map((choice, index) => <button key={choice.value} type="button" className={`button ${index === 0 ? 'primary' : 'secondary'}`} disabled={busy || !canRespond} onClick={() => void respond(choice.value)}>{busy && index === 0 && <Spinner />}{choice.label}</button>) : <>
        <button type="button" className="button secondary" disabled={busy || !canRespond} onClick={() => void respond(approval.type === 'question' ? 'cancel' : 'decline')}>{approval.type === 'question' ? '取消' : '拒绝'}</button>
        <button type="button" className="button primary" disabled={busy || !canRespond || !!unanswered} onClick={() => void respond('accept')}>{busy && <Spinner />}{approval.type === 'question' ? '提交回答' : '允许执行'}</button>
      </>}
    </div>
  </section>;
}

export function Modal({ title, children, close }: { title: string; children: ReactNode; close: () => void }) {
  const panel = useRef<HTMLDivElement>(null);
  const id = useId();
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    panel.current?.focus();
    return () => previous?.focus();
  }, []);
  return <div className="modal-backdrop" onMouseDown={event => { if (event.target === event.currentTarget) close(); }}>
    <div className="modal" role="dialog" aria-modal="true" aria-labelledby={id} tabIndex={-1} ref={panel} onKeyDown={event => {
      if (event.key === 'Escape') { event.stopPropagation(); close(); }
      if (event.key === 'Tab') {
        const focusable = Array.from(panel.current?.querySelectorAll<HTMLElement>('button:not(:disabled), a[href], input:not(:disabled), textarea:not(:disabled), select:not(:disabled), [tabindex="0"]') || []);
        const first = focusable[0], last = focusable[focusable.length - 1];
        if (event.shiftKey && (document.activeElement === first || document.activeElement === panel.current)) { event.preventDefault(); last?.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
      }
    }}>
      <div className="modal-header"><h2 id={id}>{title}</h2><IconButton label="关闭" onClick={close}><X size={19} /></IconButton></div>
      {children}
    </div>
  </div>;
}

export function FilePreview({ api, clientId, sessionId, file, close, notify }: {
  api: RemoteApi; clientId: string; sessionId: string; file: RemoteFile; close: () => void;
  notify: (text: string, tone?: 'success' | 'error') => void;
}) {
  const [url, setUrl] = useState('');
  const [text, setText] = useState<string | null>(null);
  const [mime, setMime] = useState('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [downloading, setDownloading] = useState(false);
  useEffect(() => {
    const abort = new AbortController();
    let objectUrl = '';
    void (async () => {
      try {
        const blob = await api.readFile(clientId, sessionId, file.id, false, abort.signal);
        if (abort.signal.aborted) return;
        const type = blob.type || file.mimeType;
        setMime(type);
        if (type.startsWith('text/') || /json|javascript|xml/.test(type) || /\.(rs|py|js|ts|tsx|jsx|sh|toml|ya?ml|md|log|diff|csv|json)$/i.test(file.name)) {
          setText(await blob.slice(0, 1024 * 1024).text());
        } else if (type.startsWith('image/') || type === 'application/pdf') {
          objectUrl = URL.createObjectURL(blob); setUrl(objectUrl);
        }
      } catch (failure) { if (!abort.signal.aborted) setError(errorMessage(failure)); }
      finally { if (!abort.signal.aborted) setLoading(false); }
    })();
    return () => { abort.abort(); if (objectUrl) URL.revokeObjectURL(objectUrl); };
  }, [api, clientId, sessionId, file.id, file.name, file.mimeType]);

  const download = async () => {
    if (downloading) return;
    setDownloading(true);
    try {
      const blob = await api.readFile(clientId, sessionId, file.id, true);
      const objectUrl = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = objectUrl; link.download = file.name; document.body.append(link); link.click(); link.remove();
      window.setTimeout(() => URL.revokeObjectURL(objectUrl), 30000);
    } catch (failure) { notify(errorMessage(failure), 'error'); }
    finally { setDownloading(false); }
  };
  return <Modal title={file.name} close={close}>
    <div className="preview-body">
      {loading ? <div className="preview-empty"><Spinner /><p>正在读取文件…</p></div> : error ? <p className="inline-error" role="alert">{error}</p> :
        text !== null ? <><pre className="text-preview">{text}</pre>{file.size > 1024 * 1024 && <p className="muted">预览前 1 MB，下载可查看完整内容。</p>}</> :
        url && mime.startsWith('image/') ? <img className="image-preview" src={url} alt={file.name} /> :
        url && mime === 'application/pdf' ? <iframe className="pdf-preview" title={file.name} src={url} /> :
        <div className="preview-empty"><File size={40} /><p>此格式可下载后查看</p></div>}
    </div>
    <div className="modal-footer"><span className="muted">{fileSize(file.size)}</span><button type="button" className="button primary" onClick={() => void download()} disabled={downloading}>{downloading ? <Spinner /> : <Download size={16} />}下载文件</button></div>
  </Modal>;
}
