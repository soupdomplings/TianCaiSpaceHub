import type { DisplayContent, RemoteEvent, RemoteItem } from './types';

export const activityLabels = { reasoning: '思考过程', tool: '工具执行', fileChange: '文件变更' } as const;
export type Category = keyof typeof activityLabels;
export type DisplayOption = Category | 'artifact';
export type ActivityPreferences = DisplayContent;
export const activityCategories = Object.keys(activityLabels) as Category[];
export const defaultActivity: ActivityPreferences = { reasoning: false, tool: false, fileChange: false, artifact: false };
export function activityPreferences(raw: unknown): ActivityPreferences {
  const value = raw as Partial<DisplayContent> | null | undefined;
  return { reasoning: value?.reasoning === true, tool: value?.tool === true, fileChange: value?.fileChange === true, artifact: value?.artifact === true };
}

export interface ActivityRow {
  key: number;
  itemId: string;
  category: Category;
  text: string;
  anchor?: string;
}

export function captureActivity(rows: ActivityRow[], event: RemoteEvent, prefs: ActivityPreferences, anchor?: string): ActivityRow[] {
  const detail = event.data.liveActivity as { category?: Category; itemId?: string; text?: string; mode?: string } | undefined;
  if (!detail?.category || !activityCategories.includes(detail.category) || !prefs[detail.category] ||
      !detail.itemId || typeof detail.text !== 'string' || !['started','append','entry','completed'].includes(detail.mode || '')) return rows;
  const prior = rows.at(-1);
  if (prior && prior.anchor === anchor && prior.itemId === detail.itemId && prior.category === detail.category && detail.mode !== 'started') {
    if (!detail.text) return rows;
    const separator = detail.mode !== 'append' && prior.text ? '\n' : '';
    return [...rows.slice(0, -1), { ...prior, text: prior.text + separator + detail.text }];
  }
  if (!detail.text) return rows;
  return [...rows, { key: event.id, itemId: detail.itemId, category: detail.category, text: detail.text, anchor }];
}

// Anchor ephemeral cards to dialogue visible at arrival. Older pages can be
// prepended without moving cards or inheriting the current display choices.
export function withActivity(messages: RemoteItem[], rows: ActivityRow[]): (RemoteItem | ActivityRow)[] {
  if (!rows.length) return messages;
  const byAnchor = new Map<string | undefined, ActivityRow[]>();
  for (const row of rows) {
    const list = byAnchor.get(row.anchor);
    if (list) list.push(row); else byAnchor.set(row.anchor, [row]);
  }
  const result: (RemoteItem | ActivityRow)[] = [...(byAnchor.get(undefined) || [])];
  byAnchor.delete(undefined);
  for (const message of messages) {
    result.push(message, ...(byAnchor.get(message.id) || []));
    byAnchor.delete(message.id);
  }
  for (const list of byAnchor.values()) result.push(...list);
  return result;
}
