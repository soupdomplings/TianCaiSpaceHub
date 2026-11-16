export type ClientKind = 'codex' | 'tiangong';
export type ConnectionState = 'connecting' | 'live' | 'reconnecting' | 'offline';

export interface RemoteDevice {
  id: string;
  name: string;
  online: boolean;
  paired: boolean;
  connectionType?: string;
}

export interface DevicesResponse {
  devices: RemoteDevice[];
  localDeviceId: string;
}

export interface RemoteCapabilities {
  history?: boolean;
  create?: boolean;
  send?: boolean;
  stop?: boolean;
  approvals?: boolean;
  questions?: boolean;
  attachments?: boolean;
  artifacts?: boolean;
  modelSelection?: boolean;
  effortSelection?: boolean;
}

export interface RemoteModel {
  id: string;
  name: string;
  efforts?: string[];
}

export interface RemoteClient {
  id: string;
  kind: ClientKind;
  name: string;
  ready: boolean;
  reason?: string;
  capabilities: RemoteCapabilities;
  models: RemoteModel[];
}

export interface RemoteProject {
  id: string;
  name: string;
  path: string;
}

export interface ProjectsResponse {
  projects: RemoteProject[];
  nextCursor?: string | null;
}

export interface RemoteSession {
  id: string;
  title: string;
  projectId?: string;
  cwd?: string;
  updatedAt: number | string;
  status: string;
  model?: string;
}

export interface RemoteFile {
  id: string;
  name: string;
  size: number;
  mimeType: string;
  url?: string;
  kind?: 'attachment' | 'artifact';
}

export interface RemoteItem {
  id: string;
  role: 'user' | 'assistant' | 'tool' | 'system';
  type: 'message' | 'tool' | 'fileChange' | 'reasoning';
  text: string;
  status?: string;
  files?: RemoteFile[];
  data?: Record<string, unknown>;
}

export interface RemoteChoice {
  value: string;
  label: string;
}

export interface RemoteQuestion {
  id: string;
  prompt: string;
  options?: RemoteChoice[];
  required?: boolean;
  multiple?: boolean;
}

export interface RemoteApproval {
  id: string;
  type: 'approval' | 'question';
  title: string;
  message?: string;
  choices?: RemoteChoice[];
  questions?: RemoteQuestion[];
}

export interface HistoryResponse {
  items: RemoteItem[];
  nextCursor?: string | null;
  turnId?: string | null;
  status: string;
  approvals: RemoteApproval[];
  cursor: number;
}

export interface SessionsResponse {
  sessions: RemoteSession[];
  nextCursor?: string | null;
}

export interface RemoteEvent {
  id: number;
  type: 'message.delta' | 'item.updated' | 'turn.updated' | 'approval.requested' | 'approval.resolved' | 'reset' | 'error';
  clientId: string;
  sessionId: string;
  turnId?: string;
  itemId?: string;
  data: Record<string, unknown>;
}

export interface NewSessionInput {
  title?: string;
  projectId?: string;
  cwd?: string;
  model?: string;
}

export interface TurnInput {
  text: string;
  attachmentIds: string[];
  model?: string;
  effort?: string;
  requestId: string;
}

// Native Codex choices (for example option:0) must reach the original request unchanged.
export type ApprovalDecision = string;
export type QuestionAnswers = Record<string, string | string[]>;

export function isBusy(status: string): boolean {
  return ['running', 'inProgress', 'in_progress', 'queued', 'pending', 'starting', 'finalizing', 'stopping', 'waiting_for_approval', 'waiting_for_user_input', 'awaiting_confirmation', 'waiting_approval', 'waiting_confirmation', 'unknown'].includes(status);
}

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
