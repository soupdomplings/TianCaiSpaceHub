import { isBusy } from './types';
import type { HistoryResponse, RemoteApproval, RemoteEvent, RemoteItem } from './types';

export interface ChatState {
  items: RemoteItem[];
  approvals: RemoteApproval[];
  status: string;
  turnId?: string | null;
  cursor: number;
  nextCursor?: string | null;
  snapshotNextCursor?: string | null;
  // A native snapshot has no per-delta offset. Existing item ids therefore
  // need absolute refreshes instead of replaying ambiguous additive deltas.
  snapshotItemIds: ReadonlySet<string>;
}

export const emptyChat: ChatState = { items: [], approvals: [], status: 'idle', cursor: 0, snapshotItemIds: new Set() };

// Keep every native item in chat state for cursors, reconnect anchors and files.
// Only conversational messages belong in the browser transcript.
function activityType(raw: unknown): RemoteItem['type'] | null {
  const value = typeof raw === 'string' ? raw.replace(/[_./-]/g, '').toLowerCase() : '';
  if (value === 'progress' || value === 'commentary') return 'progress';
  if (['internal','hookprompt','contextcompaction'].includes(value)) return 'internal';
  if (['reasoning', 'think', 'thinking', 'analysis', 'reasoningsummary', 'reasoningtext'].includes(value)) return 'reasoning';
  if (['filechange', 'filewrite', 'applypatch', 'patchapply'].includes(value)) return 'fileChange';
  if (['tool', 'toolcall', 'tooluse', 'toolresult', 'commandexecution', 'functioncall', 'functioncalloutput', 'mcptoolcall', 'dynamictoolcall', 'collabagenttoolcall', 'websearch', 'imageview'].includes(value)) return 'tool';
  return null;
}
export function isTranscriptMessage(item: RemoteItem): boolean {
  return ['message','userMessage','agentMessage'].includes(item.type) && ['assistant','user','system'].includes(item.role)
    && ![item.data?.nativeType, item.data?.kind, item.data?.type, item.role === 'assistant' && item.data?.phase].some(activityType);
}

export function fromHistory(history: HistoryResponse): ChatState {
  return {
    items: history.items || [], approvals: history.approvals || [], status: history.status || 'idle',
    turnId: history.turnId, cursor: history.cursor || 0, nextCursor: history.nextCursor, snapshotNextCursor: history.nextCursor,
    snapshotItemIds: new Set((history.items || []).map(item => item.id)),
  };
}

export function prependHistory(state: ChatState, history: HistoryResponse): ChatState {
  const seen = new Set(state.items.map(item => item.id));
  const added = history.items.filter(item => !seen.has(item.id) && seen.add(item.id));
  return { ...state, items: [...added, ...state.items], nextCursor: history.nextCursor === state.nextCursor ? null : history.nextCursor,
    snapshotItemIds: new Set([...state.snapshotItemIds, ...added.map(item => item.id)]) };
}

export function isCompletedItem(event: RemoteEvent): boolean {
  const item = event.data.item as RemoteItem | undefined;
  return event.data.completed === true || (event.data.completed !== false && ['completed', 'failed', 'error', 'interrupted', 'cancelled'].includes(item?.status || ''));
}

export function applyEvent(state: ChatState, event: RemoteEvent, clientId: string, sessionId: string): ChatState {
  if (event.clientId !== clientId || event.sessionId !== sessionId || event.id <= state.cursor) return state;
  const next = { ...state, cursor: event.id };
  switch (event.type) {
    case 'message.delta': {
      const id = event.itemId;
      if (!id || typeof event.data.text !== 'string') return next;
      if (state.snapshotItemIds.has(id)) return next;
      const existing = state.items.find(item => item.id === id);
      const activity = activityType(event.data.type) || activityType(event.data.nativeType) || activityType(event.data.kind) || activityType(event.data.phase) || activityType(event.data.role);
      const updated: RemoteItem = existing
        ? { ...existing, type: activity || existing.type, text: existing.text + event.data.text, status: 'running' }
        : { id, role: event.data.role === 'tool' ? 'tool' : 'assistant',
          type: activity || 'message',
          text: event.data.text, status: 'running' };
      return { ...next, items: existing ? state.items.map(item => item.id === id ? updated : item) : [...state.items, updated] };
    }
    case 'item.updated': {
      const item = event.data.item as RemoteItem | undefined;
      if (!item?.id) return next;
      if (state.snapshotItemIds.has(item.id) && !isCompletedItem(event)) return next;
      const snapshotItemIds = new Set(state.snapshotItemIds);
      if (isCompletedItem(event)) snapshotItemIds.delete(item.id);
      return { ...next, snapshotItemIds, items: state.items.some(value => value.id === item.id)
        ? state.items.map(value => value.id === item.id ? item : value) : [...state.items, item] };
    }
    case 'turn.updated': {
      const status = String(event.data.status || state.status);
      return { ...next, status, turnId: event.turnId || String(event.data.turnId || state.turnId || ''),
        items: isBusy(status) ? state.items : state.items.map(item => isBusy(item.status || '') ? { ...item, status: ['failed', 'error', 'interrupted', 'cancelled'].includes(status) ? status : 'completed' } : item) };
    }
    case 'approval.requested': {
      const approval = event.data.approval as RemoteApproval | undefined;
      if (!approval?.id) return next;
      return { ...next, approvals: [...state.approvals.filter(value => value.id !== approval.id), approval] };
    }
    case 'approval.resolved':
      return { ...next, approvals: state.approvals.filter(value => value.id !== event.data.id) };
    default:
      return next;
  }
}

export function shouldSendKey(event: { key: string; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean; isComposing: boolean; keyCode?: number }): boolean {
  return event.key === 'Enter' && (event.ctrlKey || event.metaKey) && !event.shiftKey && !event.isComposing && event.keyCode !== 229;
}

export function mergeSessions<T extends { id: string }>(previous: T[], incoming: T[]): T[] {
  const seen = new Set(previous.map(item => item.id));
  return [...previous, ...incoming.filter(item => !seen.has(item.id))];
}
