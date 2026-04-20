export type SessionSyncState = 'LocalOnly' | 'PendingUpload' | 'Synced' | 'Conflict'
export type SessionStatus = 'Idle' | 'Connecting' | 'Connected' | 'Disconnected' | 'Failed'

export type AuthMethod = 'Password' | 'Agent' | { PrivateKey: { path: string } }

export interface SessionProfile {
  id: string
  name: string
  host: string
  port: number
  username: string
  auth_method: AuthMethod
  tags: string[]
  notes: string
  local_roots: string[]
  remote_roots: string[]
  created_at: number
  updated_at: number
  last_connected_at: number | null
  sync_state: SessionSyncState
}

export type DraftAuthType = 'password' | 'private-key' | 'agent'

export interface SessionDraftPayload {
  id?: string
  name: string
  host: string
  port: number
  username: string
  auth_method:
    | { type: 'password' }
    | { type: 'agent' }
    | { type: 'private-key'; path: string }
  tags: string[]
  notes: string
  local_roots: string[]
  remote_roots: string[]
}

export interface SessionFormState {
  id?: string
  name: string
  host: string
  port: string
  username: string
  authType: DraftAuthType
  keyPath: string
  tagsInput: string
  notes: string
  localRootsInput: string
  remoteRootsInput: string
}

export interface TerminalConnection {
  session_id: string
  session_name: string
}

export interface RemoteDirEntry {
  name: string
  path: string
  kind: string
  is_dir: boolean
  size: number | null
}

export interface RemoteDirectoryListing {
  directory: string
  entries: RemoteDirEntry[]
}

export interface TransferResult {
  path: string
  bytes: number
}

export interface MutationResult {
  path: string
}

export interface KnownHostEntry {
  line: number
  hosts: string
  key_type: string
  hashed: boolean
}

export interface RemoteTreeNode {
  path: string
  name: string
  depth: number
  expanded: boolean
  loaded: boolean
  loading: boolean
  children: RemoteTreeNode[]
}

export interface RemoteContextTarget {
  path: string
  name: string
  isDir: boolean
}

export type TransferJobStatus = 'queued' | 'running' | 'success' | 'error'
export type TransferJobKind = 'upload' | 'download' | 'upload-dir' | 'download-dir'
export type TransferEventLevel = 'info' | 'warning' | 'error'
export type DockTab = 'browser' | 'editor' | 'queue' | 'activity' | 'hosts'

export interface DockTabOption {
  id: DockTab
  label: string
}

export type TransferChannelMessage =
  | { kind: 'started'; job_id: string }
  | {
      kind: 'progress'
      job_id: string
      transferred: number
      total: number | null
      current_path: string | null
      completed_files: number | null
      total_files: number | null
    }
  | { kind: 'completed'; job_id: string; bytes: number; path: string }
  | { kind: 'failed'; job_id: string; error: string }
  | { kind: 'cancelled'; job_id: string }

export interface RemoteTextFilePayload {
  path: string
  content: string
  bytes: number
}

export interface LocalPathInspection {
  path: string
  is_file: boolean
  is_dir: boolean
}

export interface TerminalTab {
  sessionId: string
  sessionName: string
  status: SessionStatus
  buffer: string
  unread: number
  connectedAt: number
}

export interface SessionWorkspaceState {
  connectSecret: string
  rememberSecret: boolean
  secretHydrated: boolean
  sftpPath: string
  sftpEntries: RemoteDirEntry[]
  remoteTreeRoots: RemoteTreeNode[]
  remoteTransferPath: string
  localTransferPath: string
  remoteTransferIsDir: boolean
  selectedRemotePaths: string[]
  sftpCreatePath: string
  sftpRenameTarget: string
  remoteEditorPath: string
  remoteEditorContent: string
  remoteEditorOriginalContent: string
  activeDockTab: DockTab
}

export interface TransferJob {
  id: string
  kind: TransferJobKind
  sessionId: string
  localPath: string
  remotePath: string
  status: TransferJobStatus
  message: string
  bytes?: number
  transferred?: number
  total?: number | null
  attemptCount: number
  maxRetries: number
  createdAt: number
  updatedAt: number
}

export interface TransferJobRecordPayload {
  id: string
  session_id: string
  kind: TransferJobKind
  local_path: string
  remote_path: string
  status: TransferJobStatus
  message: string
  bytes?: number
  transferred?: number
  total?: number | null
  attempt_count: number
  max_retries: number
  created_at: number
  updated_at: number
}

export interface TransferEventRecordPayload {
  id: string
  job_id: string
  session_id: string
  level: TransferEventLevel
  message: string
  created_at: number
}

export type TerminalStreamMessage =
  | { kind: 'status'; status: SessionStatus }
  | { kind: 'output'; data: string }
  | { kind: 'closed' }
