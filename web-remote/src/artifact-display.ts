import type { RemoteFile, RemoteItem } from './types';

// File handles are scoped by the target to this client/session. Keep the
// catalogue separate from the most recent three history rows so a snapshot
// cannot reclassify an already displayed or suppressed file.
export class ArtifactDisplay {
  readonly files = new Map<string, RemoteFile>();
  readonly handles = new Map<string, RemoteFile>();
  handleVersion = 0;
  private readonly hidden = new Set<string>();
  private hiddenUrls: ReadonlySet<string> = new Set();
  private itemHiddenUrls = new Map<string, ReadonlySet<string>>();
  private knownItems = new Set<string>();
  private atEnable = new Set<string>();

  markEnable() { this.atEnable = new Set(this.knownItems); }
  allowComplete(item: RemoteItem) { return !this.atEnable.has(item.id); }

  record(items: readonly RemoteItem[], showNew: boolean) {
    for (const item of items) {
      this.knownItems.add(item.id);
      for (const file of item.files ?? []) {
        if (!file.id) continue;
        const known = this.handles.get(file.id);
        if (!known || known.url !== file.url || known.name !== file.name) {
          this.handles.set(file.id, file);
          this.handleVersion++;
        }
        if (item.role === 'user' || file.kind !== 'artifact') continue;
        if (!this.files.has(file.id) && !showNew) this.hidden.add(file.id);
        this.files.set(file.id, file);
        if (file.url && this.hidden.has(file.id)) {
          const url = file.url.split(/[?#]/, 1)[0];
          if (!this.hiddenUrls.has(url)) this.hiddenUrls = new Set([...this.hiddenUrls, url]);
        }
      }
    }
    for (const item of items) {
      if (item.role !== 'user' && item.text) this.bindLinks(item.id);
    }
  }

  bindLinks(itemId: string) {
    this.knownItems.add(itemId);
    if (this.hiddenUrls.size) this.itemHiddenUrls.set(itemId, this.hiddenUrls);
  }
  hiddenLinksFor(item: RemoteItem) { return this.itemHiddenUrls.get(item.id); }

  forTranscript(item: RemoteItem): RemoteItem | null {
    if (item.role === 'user' || !item.files?.some(file => file.kind === 'artifact' && this.hidden.has(file.id))) return item;
    const files = item.files.filter(file => file.kind !== 'artifact' || !this.hidden.has(file.id));
    return item.text || files.length ? { ...item, files } : null;
  }
}
