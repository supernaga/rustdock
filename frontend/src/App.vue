<script setup lang="ts">
import { Channel, invoke } from '@tauri-apps/api/core'
import type { FitAddon as XTermFitAddon } from '@xterm/addon-fit'
import type { Terminal as XTermTerminal } from 'xterm'
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'

type SessionSyncState = 'LocalOnly' | 'PendingUpload' | 'Synced' | 'Conflict'
type SessionStatus = 'Idle' | 'Connecting' | 'Connected' | 'Disconnected' | 'Failed'

type AuthMethod =
  | 'Password'
  | 'Agent'
  | { PrivateKey: { path: string } }

interface SessionProfile {
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

type DraftAuthType = 'password' | 'private-key' | 'agent'

interface SessionDraftPayload {
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

interface SessionFormState {
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

interface TerminalConnection {
  session_id: string
  session_name: string
}

interface RemoteDirEntry {
  name: string
  path: string
  kind: string
  is_dir: boolean
  size: number | null
}

interface RemoteDirectoryListing {
  directory: string
  entries: RemoteDirEntry[]
}

interface TransferResult {
  path: string
  bytes: number
}

interface MutationResult {
  path: string
}

interface KnownHostEntry {
  line: number
  hosts: string
  key_type: string
  hashed: boolean
}

interface RemoteTreeNode {
  path: string
  name: string
  depth: number
  expanded: boolean
  loaded: boolean
  loading: boolean
  children: RemoteTreeNode[]
}

interface RemoteContextTarget {
  path: string
  name: string
  isDir: boolean
}

type TransferJobStatus = 'queued' | 'running' | 'success' | 'error'
type TransferJobKind = 'upload' | 'download'
type TransferEventLevel = 'info' | 'warning' | 'error'
type TransferChannelMessage =
  | { kind: 'started'; job_id: string }
  | { kind: 'progress'; job_id: string; transferred: number; total: number | null }
  | { kind: 'completed'; job_id: string; bytes: number; path: string }
  | { kind: 'failed'; job_id: string; error: string }
  | { kind: 'cancelled'; job_id: string }

interface TransferJob {
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

interface TransferJobRecordPayload {
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

interface TransferEventRecordPayload {
  id: string
  job_id: string
  session_id: string
  level: TransferEventLevel
  message: string
  created_at: number
}

type TerminalStreamMessage =
  | { kind: 'status'; status: SessionStatus }
  | { kind: 'output'; data: string }
  | { kind: 'closed' }

const sessions = ref<SessionProfile[]>([])
const selectedSessionId = ref<string | null>(null)
const loading = ref(false)
const busy = ref(false)
const sftpBusy = ref(false)
const statusLine = ref('Ready.')
const terminalStatus = ref<SessionStatus>('Idle')
const activeTerminalId = ref<string | null>(null)
const activeTerminalName = ref<string>('')
const connectSecret = ref('')
const rememberSecret = ref(false)
const terminalHost = ref<HTMLElement | null>(null)
const sftpPath = ref('/')
const sftpEntries = ref<RemoteDirEntry[]>([])
const remoteTreeRoots = ref<RemoteTreeNode[]>([])
const remoteTransferPath = ref('')
const localTransferPath = ref('')
const remoteTransferIsDir = ref(false)
const selectedRemotePaths = ref<string[]>([])
const sftpCreatePath = ref('')
const sftpRenameTarget = ref('')
const transferQueue = ref<TransferJob[]>([])
const queueRunning = ref(false)
const queueStopRequested = ref(false)
const autoResumeQueue = ref(false)
const autoRetryTransfers = ref(true)
const defaultMaxRetries = ref(2)
const enableNotifications = ref(true)
const transferEvents = ref<TransferEventRecordPayload[]>([])
const transferEventQuery = ref('')
const transferEventLevelFilter = ref<'all' | TransferEventLevel>('all')
const knownHosts = ref<KnownHostEntry[]>([])
const backgroundOnClose = ref(true)
const autoRemoveSuccessfulJobs = ref(false)
const retryBaseDelaySeconds = ref(2)
const retryMaxDelaySeconds = ref(20)
const remoteContextMenu = ref<{ x: number; y: number; target: RemoteContextTarget } | null>(null)
const AUTO_RESUME_QUEUE_KEY = 'rustdock.auto-resume-queue'
const AUTO_RETRY_TRANSFERS_KEY = 'rustdock.auto-retry-transfers'
const DEFAULT_MAX_RETRIES_KEY = 'rustdock.default-max-retries'
const ENABLE_NOTIFICATIONS_KEY = 'rustdock.enable-notifications'
const BACKGROUND_ON_CLOSE_KEY = 'rustdock.background-on-close'
const AUTO_REMOVE_SUCCESSFUL_KEY = 'rustdock.auto-remove-successful'
const RETRY_BASE_DELAY_KEY = 'rustdock.retry-base-delay'
const RETRY_MAX_DELAY_KEY = 'rustdock.retry-max-delay'

const form = ref<SessionFormState>(newDraft())

let terminal: XTermTerminal | null = null
let fitAddon: XTermFitAddon | null = null
let resizeHandler: (() => void) | null = null

const selectedSession = computed(() =>
  sessions.value.find((session) => session.id === selectedSessionId.value) ?? null
)

const visibleRemoteTreeNodes = computed(() => flattenRemoteTree(remoteTreeRoots.value))
const filteredTransferEvents = computed(() =>
  transferEvents.value.filter((event) => {
    const levelMatches =
      transferEventLevelFilter.value === 'all' || event.level === transferEventLevelFilter.value
    const query = transferEventQuery.value.trim().toLowerCase()
    const queryMatches =
      query.length === 0 ||
      event.message.toLowerCase().includes(query) ||
      event.job_id.toLowerCase().includes(query) ||
      event.session_id.toLowerCase().includes(query)
    return levelMatches && queryMatches
  })
)

const secretPrompt = computed(() => {
  if (!selectedSession.value) {
    return 'Session-only secret'
  }
  if (selectedSession.value.auth_method === 'Password') {
    return 'Password'
  }
  if (typeof selectedSession.value.auth_method === 'object' && 'PrivateKey' in selectedSession.value.auth_method) {
    return 'Key passphrase (optional)'
  }
  return 'Session-only secret'
})

async function loadSessions() {
  loading.value = true
  try {
    sessions.value = await invoke<SessionProfile[]>('list_sessions')
    if (selectedSessionId.value) {
      const refreshed = sessions.value.find((session) => session.id === selectedSessionId.value)
      if (refreshed) {
        form.value = draftFromSession(refreshed)
      }
    }
    statusLine.value = `Loaded ${sessions.value.length} sessions.`
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    loading.value = false
  }
}

async function loadTransferQueue() {
  try {
    const records = await invoke<TransferJobRecordPayload[]>('list_transfer_jobs')
    transferQueue.value = records.map(fromTransferRecord)
  } catch (error) {
    statusLine.value = renderError(error)
  }
}

async function loadTransferEvents() {
  try {
    transferEvents.value = await invoke<TransferEventRecordPayload[]>('list_transfer_events', {
      limit: 200
    })
  } catch (error) {
    statusLine.value = renderError(error)
  }
}

async function loadSavedSecret(sessionId: string) {
  try {
    const secret = await invoke<string | null>('load_session_secret', { sessionId })
    if (secret) {
      connectSecret.value = secret
      rememberSecret.value = true
    } else {
      connectSecret.value = ''
      rememberSecret.value = false
    }
  } catch (error) {
    statusLine.value = renderError(error)
  }
}

async function loadKnownHosts() {
  try {
    knownHosts.value = await invoke<KnownHostEntry[]>('list_known_hosts_entries')
  } catch (error) {
    statusLine.value = renderError(error)
  }
}

function startNewSession() {
  selectedSessionId.value = null
  form.value = newDraft()
  remoteTreeRoots.value = []
  statusLine.value = 'Draft reset.'
}

function selectSession(session: SessionProfile) {
  selectedSessionId.value = session.id
  form.value = draftFromSession(session)
  sftpPath.value = session.remote_roots[0] || '/'
  hydrateRemoteTree(session)
  sftpEntries.value = []
  remoteTransferPath.value = ''
  remoteTransferIsDir.value = false
  selectedRemotePaths.value = []
  sftpRenameTarget.value = ''
  statusLine.value = `Selected ${session.name}.`
  void loadSavedSecret(session.id)
}

async function saveSession() {
  busy.value = true
  try {
    const payload = toPayload(form.value)
    const saved = await invoke<SessionProfile>('save_session', { draft: payload })
    await loadSessions()
    selectedSessionId.value = saved.id
    form.value = draftFromSession(saved)
    statusLine.value = `Saved ${saved.name}.`
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    busy.value = false
  }
}

async function deleteSession() {
  if (!selectedSessionId.value) {
    statusLine.value = 'Select a session first.'
    return
  }

  const accepted = await dialogConfirm(
    `Delete session "${selectedSession.value?.name ?? selectedSessionId.value}"?`,
    { title: 'Delete Session', kind: 'warning' }
  )
  if (!accepted) {
    statusLine.value = 'Session deletion cancelled.'
    return
  }

  busy.value = true
  try {
    await invoke('delete_session', { sessionId: selectedSessionId.value })
    if (activeTerminalId.value === selectedSessionId.value) {
      closeTerminalState()
    }
    selectedSessionId.value = null
    form.value = newDraft()
    await loadSessions()
    statusLine.value = 'Session deleted.'
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    busy.value = false
  }
}

async function connectTerminal() {
  if (!selectedSessionId.value) {
    statusLine.value = 'Save and select a session before connecting.'
    return
  }
  if (!terminal || !fitAddon) {
    statusLine.value = 'Terminal is not ready yet.'
    return
  }

  fitAddon.fit()
  const cols = terminal.cols || 120
  const rows = terminal.rows || 32

  busy.value = true
  terminalStatus.value = 'Connecting'
  terminal.reset()

  try {
    if (rememberSecret.value && connectSecret.value.trim()) {
      await invoke('save_session_secret', {
        sessionId: selectedSessionId.value,
        secret: connectSecret.value.trim()
      })
    }

    const channel = new Channel<TerminalStreamMessage>()
    channel.onmessage = (message) => {
      if (!terminal) {
        return
      }
      if (message.kind === 'output') {
        terminal.write(message.data)
      } else if (message.kind === 'status') {
        terminalStatus.value = message.status
      } else if (message.kind === 'closed') {
        closeTerminalState()
      }
    }

    const connection = await invoke<TerminalConnection>('connect_terminal', {
      sessionId: selectedSessionId.value,
      cols,
      rows,
      secret: connectSecret.value.trim() || null,
      channel
    })

    activeTerminalId.value = connection.session_id
    activeTerminalName.value = connection.session_name
    connectSecret.value = ''
    terminal.focus()
    statusLine.value = `Terminal attached to ${connection.session_name}.`
    await loadSessions()
    await loadKnownHosts()
  } catch (error) {
    terminalStatus.value = 'Failed'
    statusLine.value = renderError(error)
  } finally {
    busy.value = false
  }
}

async function disconnectTerminal() {
  if (!activeTerminalId.value) {
    statusLine.value = 'No active terminal session.'
    return
  }

  try {
    await invoke('disconnect_terminal', { sessionId: activeTerminalId.value })
    closeTerminalState()
    statusLine.value = 'Terminal disconnected.'
  } catch (error) {
    statusLine.value = renderError(error)
  }
}

function closeTerminalState() {
  activeTerminalId.value = null
  activeTerminalName.value = ''
  terminalStatus.value = 'Disconnected'
}

async function loadSftpDirectory(path?: string) {
  if (!selectedSessionId.value) {
    statusLine.value = 'Select a saved session first.'
    return
  }

  sftpBusy.value = true
  try {
    const listing = await invoke<RemoteDirectoryListing>('list_sftp_dir', {
      request: {
        session_id: selectedSessionId.value,
        path: path ?? sftpPath.value,
        secret: connectSecret.value.trim() || null
      }
    })
    sftpPath.value = listing.directory
    sftpEntries.value = listing.entries
    selectedRemotePaths.value = selectedRemotePaths.value.filter((selectedPath) =>
      listing.entries.some((entry) => entry.path === selectedPath)
    )
    statusLine.value = `Loaded ${listing.entries.length} entries from ${listing.directory}.`
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    sftpBusy.value = false
  }
}

function hydrateRemoteTree(session: SessionProfile) {
  const roots = session.remote_roots.length > 0 ? session.remote_roots : ['/']
  const uniqueRoots = Array.from(new Set(roots.map((root) => root.trim()).filter(Boolean)))
  remoteTreeRoots.value = uniqueRoots.map((rootPath) =>
    makeRemoteTreeNode(rootPath, rootPath === '/' ? '/' : basename(rootPath), 0)
  )
}

function makeRemoteTreeNode(path: string, name: string, depth: number): RemoteTreeNode {
  return {
    path,
    name,
    depth,
    expanded: false,
    loaded: false,
    loading: false,
    children: []
  }
}

function flattenRemoteTree(nodes: RemoteTreeNode[]): RemoteTreeNode[] {
  const flat: RemoteTreeNode[] = []
  for (const node of nodes) {
    flat.push(node)
    if (node.expanded && node.children.length) {
      flat.push(...flattenRemoteTree(node.children))
    }
  }
  return flat
}

async function toggleRemoteTreeNode(node: RemoteTreeNode) {
  if (node.expanded) {
    node.expanded = false
    return
  }

  await openRemoteTreeNode(node)
}

async function openRemoteTreeNode(node: RemoteTreeNode) {
  if (!node.loaded) {
    await loadRemoteTreeChildren(node)
  }
  node.expanded = true
  await loadSftpDirectory(node.path)
}

async function loadRemoteTreeChildren(node: RemoteTreeNode) {
  if (!selectedSessionId.value || node.loading) {
    return
  }

  node.loading = true
  try {
    const secret = await resolveSecretForSession(selectedSessionId.value)
    const listing = await invoke<RemoteDirectoryListing>('list_sftp_dir', {
      request: {
        session_id: selectedSessionId.value,
        path: node.path,
        secret
      }
    })
    node.children = listing.entries
      .filter((entry) => entry.is_dir)
      .map((entry) => makeRemoteTreeNode(entry.path, entry.name, node.depth + 1))
    node.loaded = true
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    node.loading = false
  }
}

function openRemoteEntry(entry: RemoteDirEntry) {
  if (entry.is_dir) {
    void loadSftpDirectory(entry.path)
    return
  }
  remoteTransferPath.value = entry.path
  remoteTransferIsDir.value = entry.is_dir
  sftpRenameTarget.value = entry.path
  if (!localTransferPath.value) {
    localTransferPath.value = entry.name
  }
  statusLine.value = `Selected remote file ${entry.path}.`
}

function selectRemotePathForMutation(entry: RemoteDirEntry) {
  remoteTransferPath.value = entry.path
  remoteTransferIsDir.value = entry.is_dir
  sftpRenameTarget.value = entry.path
  statusLine.value = `Selected ${entry.path}.`
}

function toggleRemoteSelection(entry: RemoteDirEntry) {
  if (selectedRemotePaths.value.includes(entry.path)) {
    selectedRemotePaths.value = selectedRemotePaths.value.filter((path) => path !== entry.path)
  } else {
    selectedRemotePaths.value = [...selectedRemotePaths.value, entry.path]
  }
}

function isRemoteSelected(path: string): boolean {
  return selectedRemotePaths.value.includes(path)
}

function clearRemoteSelection() {
  selectedRemotePaths.value = []
}

function goToParentDirectory() {
  if (!sftpPath.value || sftpPath.value === '/') {
    sftpPath.value = '/'
    return
  }

  const parts = sftpPath.value.split('/').filter(Boolean)
  parts.pop()
  const parent = parts.length === 0 ? '/' : `/${parts.join('/')}`
  void loadSftpDirectory(parent)
}

async function downloadSelectedRemoteFile() {
  if (!selectedSessionId.value) {
    statusLine.value = 'Select a saved session first.'
    return
  }
  if (!remoteTransferPath.value.trim() || !localTransferPath.value.trim()) {
    statusLine.value = 'Set both remote and local paths before downloading.'
    return
  }

  sftpBusy.value = true
  try {
    const result = await invoke<TransferResult>('download_sftp_file', {
      request: {
        session_id: selectedSessionId.value,
        remote_path: remoteTransferPath.value.trim(),
        local_path: localTransferPath.value.trim(),
        secret: connectSecret.value.trim() || null
      }
    })
    statusLine.value = `Downloaded ${result.bytes} bytes to ${result.path}.`
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    sftpBusy.value = false
  }
}

async function uploadLocalFile() {
  if (!selectedSessionId.value) {
    statusLine.value = 'Select a saved session first.'
    return
  }
  if (!remoteTransferPath.value.trim() || !localTransferPath.value.trim()) {
    statusLine.value = 'Set both local and remote paths before uploading.'
    return
  }

  sftpBusy.value = true
  try {
    const result = await invoke<TransferResult>('upload_sftp_file', {
      request: {
        session_id: selectedSessionId.value,
        local_path: localTransferPath.value.trim(),
        remote_path: remoteTransferPath.value.trim(),
        secret: connectSecret.value.trim() || null
      }
    })
    statusLine.value = `Uploaded ${result.bytes} bytes to ${result.path}.`
    await loadSftpDirectory(sftpPath.value)
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    sftpBusy.value = false
  }
}

async function chooseUploadLocalFile() {
  const selected = await dialogOpen({
    multiple: false,
    directory: false
  })

  if (!selected || Array.isArray(selected)) {
    return
  }

  localTransferPath.value = selected
  if (!remoteTransferPath.value.trim()) {
    remoteTransferPath.value = joinRemotePath(sftpPath.value, basename(selected))
  }
}

async function chooseDownloadLocalPath() {
  const selected = await dialogSave({
    defaultPath: localTransferPath.value || basename(remoteTransferPath.value) || 'download.txt'
  })

  if (!selected) {
    return
  }

  localTransferPath.value = selected
}

async function queueDownloadJob() {
  if (!selectedSessionId.value) {
    statusLine.value = 'Select a saved session first.'
    return
  }
  if (!remoteTransferPath.value.trim() || !localTransferPath.value.trim()) {
    statusLine.value = 'Set both remote and local paths before queueing a download.'
    return
  }

  const job: TransferJob = {
    id: `${Date.now()}-download`,
    kind: 'download',
    sessionId: selectedSessionId.value,
    remotePath: remoteTransferPath.value.trim(),
    localPath: localTransferPath.value.trim(),
    status: 'queued',
    message: 'Waiting',
    attemptCount: 0,
    maxRetries: defaultMaxRetries.value,
    createdAt: nowEpoch(),
    updatedAt: nowEpoch()
  }
  transferQueue.value.push(job)
  await persistTransferJob(job)
  await appendTransferEvent(job, 'info', `Queued download ${job.remotePath}`)
  await updateTrayQueueStatus()
  statusLine.value = 'Download queued.'
}

async function queueUploadJob() {
  if (!selectedSessionId.value) {
    statusLine.value = 'Select a saved session first.'
    return
  }
  if (!remoteTransferPath.value.trim() || !localTransferPath.value.trim()) {
    statusLine.value = 'Set both local and remote paths before queueing an upload.'
    return
  }

  const job: TransferJob = {
    id: `${Date.now()}-upload`,
    kind: 'upload',
    sessionId: selectedSessionId.value,
    remotePath: remoteTransferPath.value.trim(),
    localPath: localTransferPath.value.trim(),
    status: 'queued',
    message: 'Waiting',
    attemptCount: 0,
    maxRetries: defaultMaxRetries.value,
    createdAt: nowEpoch(),
    updatedAt: nowEpoch()
  }
  transferQueue.value.push(job)
  await persistTransferJob(job)
  await appendTransferEvent(job, 'info', `Queued upload ${job.localPath}`)
  await updateTrayQueueStatus()
  statusLine.value = 'Upload queued.'
}

async function runTransferQueue() {
  if (queueRunning.value) {
    return
  }

  queueRunning.value = true
  queueStopRequested.value = false
  try {
    while (!queueStopRequested.value) {
      const nextJob = transferQueue.value.find((job) => job.status === 'queued')
      if (!nextJob) {
        break
      }
      await runQueuedTransfer(nextJob)
    }

    statusLine.value = queueStopRequested.value ? 'Transfer queue paused.' : 'Transfer queue finished.'
    if (!queueStopRequested.value) {
      await notifyUser('Transfer Queue Finished', 'All queued transfers have completed.')
    }
    if (selectedSessionId.value) {
      await loadSftpDirectory(sftpPath.value)
    }
  } finally {
    queueRunning.value = false
    queueStopRequested.value = false
  }
  await updateTrayQueueStatus()
}

async function clearCompletedTransfers() {
  const completed = transferQueue.value.filter(
    (job) => job.status === 'success' || job.status === 'error'
  )
  for (const job of completed) {
    await deletePersistedTransferJob(job.id)
  }
  transferQueue.value = transferQueue.value.filter(
    (job) => job.status === 'queued' || job.status === 'running'
  )
  await updateTrayQueueStatus()
}

function requestQueueStop() {
  if (!queueRunning.value) {
    return
  }
  queueStopRequested.value = true
  statusLine.value = 'Queue will stop after the current transfer.'
}

async function retryTransferJob(jobId: string) {
  const job = transferQueue.value.find((entry) => entry.id === jobId)
  if (!job || job.status === 'running') {
    return
  }
  job.status = 'queued'
  job.message = 'Retry queued'
  delete job.bytes
  delete job.transferred
  delete job.total
  job.updatedAt = nowEpoch()
  await persistTransferJob(job)
  await appendTransferEvent(job, 'info', `Queued retry for ${job.remotePath}`)
}

async function removeTransferJob(jobId: string) {
  const job = transferQueue.value.find((entry) => entry.id === jobId)
  if (!job || job.status === 'running') {
    return
  }
  transferQueue.value = transferQueue.value.filter((entry) => entry.id !== jobId)
  await deletePersistedTransferJob(jobId)
  if (job) {
    await appendTransferEvent(job, 'warning', `Removed job ${job.id} from queue`)
  }
  await updateTrayQueueStatus()
}

async function removeTransferJobInternal(job: TransferJob) {
  transferQueue.value = transferQueue.value.filter((entry) => entry.id !== job.id)
  await deletePersistedTransferJob(job.id)
  await updateTrayQueueStatus()
}

async function runQueuedTransfer(job: TransferJob) {
  job.attemptCount += 1
  job.status = 'running'
  job.message = `Starting attempt ${job.attemptCount}/${job.maxRetries + 1}`
  job.transferred = 0
  job.total = null
  job.updatedAt = nowEpoch()
  await persistTransferJob(job)
  await appendTransferEvent(
    job,
    'info',
    `Started ${job.kind} attempt ${job.attemptCount}/${job.maxRetries + 1}`
  )
  const secret = await resolveSecretForSession(job.sessionId)

  await new Promise<void>((resolve) => {
    const channel = new Channel<TransferChannelMessage>()
    channel.onmessage = (message) => {
      if (message.job_id !== job.id) {
        return
      }

      if (message.kind === 'started') {
        job.message = 'Connected'
      } else if (message.kind === 'progress') {
        job.transferred = message.transferred
        job.total = message.total
        job.message = renderTransferProgress(message.transferred, message.total)
      } else if (message.kind === 'completed') {
        job.status = 'success'
        job.bytes = message.bytes
        job.transferred = message.bytes
        job.total = message.bytes
        job.updatedAt = nowEpoch()
        job.message =
          job.kind === 'upload'
            ? `Uploaded ${message.bytes} bytes`
            : `Downloaded ${message.bytes} bytes`
        void persistTransferJob(job)
        void appendTransferEvent(job, 'info', job.message)
        void notifyUser('Transfer Completed', job.message)
        if (autoRemoveSuccessfulJobs.value) {
          void removeTransferJobInternal(job)
        } else {
          void updateTrayQueueStatus()
        }
        resolve()
      } else if (message.kind === 'cancelled') {
        job.status = 'error'
        job.message = 'Cancelled'
        job.updatedAt = nowEpoch()
        void persistTransferJob(job)
        void appendTransferEvent(job, 'warning', `Cancelled ${job.kind} job`)
        void updateTrayQueueStatus()
        resolve()
      } else if (message.kind === 'failed') {
        void finalizeTransferFailure(job, message.error)
        resolve()
      }
    }

    invoke('start_sftp_transfer', {
      request: {
        job_id: job.id,
        kind: job.kind,
        session_id: job.sessionId,
        local_path: job.localPath,
        remote_path: job.remotePath,
        secret
      },
      channel
    }).catch((error) => {
      void finalizeTransferFailure(job, renderError(error))
      resolve()
    })
  })
}

async function cancelRunningTransfer(jobId: string) {
  const job = transferQueue.value.find((entry) => entry.id === jobId)
  if (!job || job.status !== 'running') {
    return
  }

  try {
    await invoke('cancel_sftp_transfer', {
      request: { job_id: jobId }
    })
    job.message = 'Cancelling...'
    job.updatedAt = nowEpoch()
    await persistTransferJob(job)
  } catch (error) {
    statusLine.value = renderError(error)
  }
}

async function createRemoteDirectory() {
  if (!selectedSessionId.value) {
    statusLine.value = 'Select a saved session first.'
    return
  }
  if (!sftpCreatePath.value.trim()) {
    statusLine.value = 'Enter a remote directory path to create.'
    return
  }

  sftpBusy.value = true
  try {
    const result = await invoke<MutationResult>('create_sftp_dir', {
      request: {
        session_id: selectedSessionId.value,
        remote_path: sftpCreatePath.value.trim(),
        secret: connectSecret.value.trim() || null
      }
    })
    statusLine.value = `Created directory ${result.path}.`
    sftpCreatePath.value = ''
    await loadSftpDirectory(sftpPath.value)
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    sftpBusy.value = false
  }
}

async function renameRemotePath() {
  if (!selectedSessionId.value) {
    statusLine.value = 'Select a saved session first.'
    return
  }
  if (!remoteTransferPath.value.trim() || !sftpRenameTarget.value.trim()) {
    statusLine.value = 'Set both source and target paths before renaming.'
    return
  }

  sftpBusy.value = true
  try {
    const result = await invoke<MutationResult>('rename_sftp_path', {
      request: {
        session_id: selectedSessionId.value,
        source_path: remoteTransferPath.value.trim(),
        target_path: sftpRenameTarget.value.trim(),
        secret: connectSecret.value.trim() || null
      }
    })
    remoteTransferPath.value = result.path
    sftpRenameTarget.value = result.path
    statusLine.value = `Renamed remote path to ${result.path}.`
    await loadSftpDirectory(sftpPath.value)
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    sftpBusy.value = false
  }
}

async function deleteRemotePath() {
  if (!selectedSessionId.value) {
    statusLine.value = 'Select a saved session first.'
    return
  }
  if (!remoteTransferPath.value.trim()) {
    statusLine.value = 'Select a remote path before deleting.'
    return
  }

  const accepted = await dialogConfirm(
    `Delete ${remoteTransferIsDir.value ? 'directory' : 'file'} "${remoteTransferPath.value.trim()}"?`,
    { title: 'Delete Remote Path', kind: 'warning' }
  )
  if (!accepted) {
    statusLine.value = 'Remote delete cancelled.'
    return
  }

  sftpBusy.value = true
  try {
    const result = await invoke<MutationResult>('delete_sftp_path', {
      request: {
        session_id: selectedSessionId.value,
        remote_path: remoteTransferPath.value.trim(),
        is_dir: remoteTransferIsDir.value,
        secret: connectSecret.value.trim() || null
      }
    })
    statusLine.value = `Deleted ${result.path}.`
    remoteTransferPath.value = ''
    remoteTransferIsDir.value = false
    sftpRenameTarget.value = ''
    await loadSftpDirectory(sftpPath.value)
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    sftpBusy.value = false
  }
}

async function batchQueueDownloads() {
  if (!selectedSessionId.value) {
    statusLine.value = 'Select a saved session first.'
    return
  }

  const selectedEntries = sftpEntries.value.filter(
    (entry) => selectedRemotePaths.value.includes(entry.path) && !entry.is_dir
  )
  if (!selectedEntries.length) {
    statusLine.value = 'Select one or more files to queue downloads.'
    return
  }

  const selected = await dialogOpen({
    multiple: false,
    directory: true
  })
  if (!selected || Array.isArray(selected)) {
    return
  }

  for (const entry of selectedEntries) {
    const job: TransferJob = {
      id: `${Date.now()}-${entry.name}-download`,
      kind: 'download',
      sessionId: selectedSessionId.value,
      remotePath: entry.path,
      localPath: `${selected.replace(/[\\/]$/, '')}/${entry.name}`,
      status: 'queued',
      message: 'Waiting',
      attemptCount: 0,
      maxRetries: defaultMaxRetries.value,
      createdAt: nowEpoch(),
      updatedAt: nowEpoch()
    }
    transferQueue.value.push(job)
    await persistTransferJob(job)
    await appendTransferEvent(job, 'info', `Queued batch download ${job.remotePath}`)
  }

  statusLine.value = `Queued ${selectedEntries.length} downloads.`
}

async function batchQueueUploads() {
  if (!selectedSessionId.value) {
    statusLine.value = 'Select a saved session first.'
    return
  }

  const selected = await dialogOpen({
    multiple: true,
    directory: false
  })
  if (!selected) {
    return
  }

  const files = Array.isArray(selected) ? selected : [selected]
  const targetDirectory = sftpPath.value || '/'

  for (const localPath of files) {
    const job: TransferJob = {
      id: `${Date.now()}-${basename(localPath)}-upload`,
      kind: 'upload',
      sessionId: selectedSessionId.value,
      remotePath: joinRemotePath(targetDirectory, basename(localPath)),
      localPath,
      status: 'queued',
      message: 'Waiting',
      attemptCount: 0,
      maxRetries: defaultMaxRetries.value,
      createdAt: nowEpoch(),
      updatedAt: nowEpoch()
    }
    transferQueue.value.push(job)
    await persistTransferJob(job)
    await appendTransferEvent(job, 'info', `Queued batch upload ${job.localPath}`)
  }

  statusLine.value = `Queued ${files.length} uploads to ${targetDirectory}.`
}

async function batchDeleteSelectedRemote() {
  if (!selectedSessionId.value) {
    statusLine.value = 'Select a saved session first.'
    return
  }

  const selectedEntries = sftpEntries.value.filter((entry) => selectedRemotePaths.value.includes(entry.path))
  if (!selectedEntries.length) {
    statusLine.value = 'Select one or more remote paths to delete.'
    return
  }

  const accepted = await dialogConfirm(
    `Delete ${selectedEntries.length} selected remote paths from ${sftpPath.value}?`,
    { title: 'Batch Delete Remote Paths', kind: 'warning' }
  )
  if (!accepted) {
    statusLine.value = 'Batch delete cancelled.'
    return
  }

  sftpBusy.value = true
  try {
    for (const entry of selectedEntries) {
      await invoke('delete_sftp_path', {
        request: {
          session_id: selectedSessionId.value,
          remote_path: entry.path,
          is_dir: entry.is_dir,
          secret: connectSecret.value.trim() || null
        }
      })
    }
    selectedRemotePaths.value = []
    statusLine.value = `Deleted ${selectedEntries.length} remote paths.`
    await loadSftpDirectory(sftpPath.value)
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    sftpBusy.value = false
  }
}

function draftFromSession(session: SessionProfile): SessionFormState {
  const auth = decodeAuthMethod(session.auth_method)

  return {
    id: session.id,
    name: session.name,
    host: session.host,
    port: String(session.port),
    username: session.username,
    authType: auth.type,
    keyPath: auth.path,
    tagsInput: session.tags.join(', '),
    notes: session.notes,
    localRootsInput: session.local_roots.join('\n'),
    remoteRootsInput: session.remote_roots.join('\n')
  }
}

function newDraft(): SessionFormState {
  return {
    name: '',
    host: '',
    port: '22',
    username: '',
    authType: 'private-key',
    keyPath: '',
    tagsInput: '',
    notes: '',
    localRootsInput: '',
    remoteRootsInput: ''
  }
}

function toPayload(state: SessionFormState): SessionDraftPayload {
  const auth_method =
    state.authType === 'password'
      ? { type: 'password' as const }
      : state.authType === 'agent'
        ? { type: 'agent' as const }
        : { type: 'private-key' as const, path: state.keyPath.trim() }

  return {
    id: state.id,
    name: state.name.trim(),
    host: state.host.trim(),
    port: Number.parseInt(state.port, 10),
    username: state.username.trim(),
    auth_method,
    tags: splitList(state.tagsInput, /,/),
    notes: state.notes,
    local_roots: splitList(state.localRootsInput, /\n/),
    remote_roots: splitList(state.remoteRootsInput, /\n/)
  }
}

function splitList(value: string, separator: RegExp): string[] {
  return value
    .split(separator)
    .map((item) => item.trim())
    .filter(Boolean)
}

function decodeAuthMethod(auth: AuthMethod): { type: DraftAuthType; path: string } {
  if (typeof auth === 'string') {
    if (auth === 'Password') {
      return { type: 'password', path: '' }
    }
    return { type: 'agent', path: '' }
  }

  if ('PrivateKey' in auth) {
    return { type: 'private-key', path: auth.PrivateKey.path }
  }

  return { type: 'private-key', path: '' }
}

function syncLabel(value: SessionSyncState): string {
  switch (value) {
    case 'PendingUpload':
      return 'Pending upload'
    case 'Synced':
      return 'Synced'
    case 'Conflict':
      return 'Conflict'
    default:
      return 'Local only'
  }
}

function renderError(error: unknown): string {
  if (typeof error === 'string') {
    return error
  }
  if (error && typeof error === 'object' && 'message' in error) {
    return String((error as { message: unknown }).message)
  }
  return 'Operation failed.'
}

function renderTransferProgress(transferred: number, total: number | null): string {
  if (!total || total <= 0) {
    return `${transferred} bytes`
  }
  const percent = Math.min(100, Math.round((transferred / total) * 100))
  return `${percent}% · ${transferred}/${total} bytes`
}

function computeRetryDelaySeconds(attemptCount: number): number {
  const exponent = Math.max(0, attemptCount - 1)
  return Math.min(retryMaxDelaySeconds.value, retryBaseDelaySeconds.value * 2 ** exponent)
}

async function delay(ms: number) {
  await new Promise((resolve) => window.setTimeout(resolve, ms))
}

async function finalizeTransferFailure(job: TransferJob, errorMessage: string) {
  const retryable = isRetryableTransferError(errorMessage)
  if (retryable && autoRetryTransfers.value && job.attemptCount <= job.maxRetries) {
    const delaySeconds = computeRetryDelaySeconds(job.attemptCount)
    job.status = 'queued'
    job.message = `Retry in ${delaySeconds}s after error: ${errorMessage}`
    job.updatedAt = nowEpoch()
    await persistTransferJob(job)
    await appendTransferEvent(job, 'warning', job.message)
    await updateTrayQueueStatus()
    await delay(delaySeconds * 1000)
    return
  }

  job.status = 'error'
  job.message = errorMessage
  job.updatedAt = nowEpoch()
  await persistTransferJob(job)
  await appendTransferEvent(job, 'error', errorMessage)
  await notifyUser('Transfer Failed', errorMessage)
  await updateTrayQueueStatus()
}

async function saveCurrentSecret() {
  if (!selectedSessionId.value) {
    statusLine.value = 'Select a saved session first.'
    return
  }
  if (!connectSecret.value.trim()) {
    statusLine.value = 'Secret is empty.'
    return
  }
  await invoke('save_session_secret', {
    sessionId: selectedSessionId.value,
    secret: connectSecret.value.trim()
  })
  rememberSecret.value = true
  statusLine.value = 'Secret saved to the system keychain.'
}

async function forgetSavedSecret() {
  if (!selectedSessionId.value) {
    statusLine.value = 'Select a saved session first.'
    return
  }
  await invoke('delete_session_secret', { sessionId: selectedSessionId.value })
  connectSecret.value = ''
  rememberSecret.value = false
  statusLine.value = 'Saved secret removed from the system keychain.'
}

async function forgetSelectedSessionKnownHost() {
  if (!selectedSessionId.value) {
    statusLine.value = 'Select a saved session first.'
    return
  }

  const accepted = await dialogConfirm(
    `Forget known_hosts entries for "${selectedSession.value?.host ?? selectedSessionId.value}"?`,
    { title: 'Forget Known Host', kind: 'warning' }
  )
  if (!accepted) {
    statusLine.value = 'Known host removal cancelled.'
    return
  }

  try {
    const removed = await invoke<number>('forget_session_known_host', {
      sessionId: selectedSessionId.value
    })
    await loadKnownHosts()
    statusLine.value = removed > 0 ? `Removed ${removed} known_hosts entries.` : 'No known_hosts entries matched this session.'
  } catch (error) {
    statusLine.value = renderError(error)
  }
}

async function removeKnownHostLine(entry: KnownHostEntry) {
  const accepted = await dialogConfirm(
    `Remove known_hosts line ${entry.line} for ${entry.hosts}?`,
    { title: 'Remove Known Host Entry', kind: 'warning' }
  )
  if (!accepted) {
    return
  }

  try {
    await invoke('remove_known_host_entry', { line: entry.line })
    await loadKnownHosts()
    statusLine.value = `Removed known_hosts line ${entry.line}.`
  } catch (error) {
    statusLine.value = renderError(error)
  }
}

async function resolveSecretForSession(sessionId: string): Promise<string | null> {
  if (selectedSessionId.value === sessionId && connectSecret.value.trim()) {
    return connectSecret.value.trim()
  }
  return invoke<string | null>('load_session_secret', { sessionId })
}

async function persistTransferJob(job: TransferJob) {
  await invoke('save_transfer_job', {
    job: toTransferRecord(job)
  })
}

async function deletePersistedTransferJob(jobId: string) {
  await invoke('delete_transfer_job', { jobId })
}

function toTransferRecord(job: TransferJob): TransferJobRecordPayload {
  return {
    id: job.id,
    session_id: job.sessionId,
    kind: job.kind,
    local_path: job.localPath,
    remote_path: job.remotePath,
    status: job.status,
    message: job.message,
    bytes: job.bytes,
    transferred: job.transferred,
    total: job.total,
    attempt_count: job.attemptCount,
    max_retries: job.maxRetries,
    created_at: job.createdAt,
    updated_at: job.updatedAt
  }
}

function fromTransferRecord(record: TransferJobRecordPayload): TransferJob {
  return {
    id: record.id,
    sessionId: record.session_id,
    kind: record.kind,
    localPath: record.local_path,
    remotePath: record.remote_path,
    status: record.status,
    message: record.message,
    bytes: record.bytes,
    transferred: record.transferred,
    total: record.total,
    attemptCount: record.attempt_count,
    maxRetries: record.max_retries,
    createdAt: record.created_at,
    updatedAt: record.updated_at
  }
}

function nowEpoch(): number {
  return Math.floor(Date.now() / 1000)
}

async function appendTransferEvent(
  job: TransferJob,
  level: TransferEventLevel,
  message: string
) {
  const event: TransferEventRecordPayload = {
    id: `${job.id}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    job_id: job.id,
    session_id: job.sessionId,
    level,
    message,
    created_at: nowEpoch()
  }
  transferEvents.value.unshift(event)
  if (transferEvents.value.length > 200) {
    transferEvents.value = transferEvents.value.slice(0, 200)
  }
  await invoke('save_transfer_event', { event })
}

async function clearTransferEvents() {
  await invoke('clear_transfer_events')
  transferEvents.value = []
}

function isRetryableTransferError(message: string): boolean {
  const lower = message.toLowerCase()
  if (
    lower.includes('timed out') ||
    lower.includes('timeout') ||
    lower.includes('connection reset') ||
    lower.includes('broken pipe') ||
    lower.includes('temporarily unavailable') ||
    lower.includes('connection refused') ||
    lower.includes('failed to open ssh transport') ||
    lower.includes('failed to initialize sftp session') ||
    lower.includes('senderror')
  ) {
    return true
  }

  if (
    lower.includes('permission denied') ||
    lower.includes('authentication failed') ||
    lower.includes('rejected') ||
    lower.includes('no such file') ||
    lower.includes('not found') ||
    lower.includes('agent has no loaded identities')
  ) {
    return false
  }

  return false
}

function loadAutoResumeQueueSetting() {
  autoResumeQueue.value = window.localStorage.getItem(AUTO_RESUME_QUEUE_KEY) === 'true'
}

function persistAutoResumeQueueSetting() {
  window.localStorage.setItem(AUTO_RESUME_QUEUE_KEY, String(autoResumeQueue.value))
}

function loadNotificationSetting() {
  enableNotifications.value = window.localStorage.getItem(ENABLE_NOTIFICATIONS_KEY) !== 'false'
}

function persistNotificationSetting() {
  window.localStorage.setItem(ENABLE_NOTIFICATIONS_KEY, String(enableNotifications.value))
}

function loadTransferBehaviorSettings() {
  autoRemoveSuccessfulJobs.value = window.localStorage.getItem(AUTO_REMOVE_SUCCESSFUL_KEY) === 'true'
  const retryBase = Number.parseInt(window.localStorage.getItem(RETRY_BASE_DELAY_KEY) || '2', 10)
  const retryMax = Number.parseInt(window.localStorage.getItem(RETRY_MAX_DELAY_KEY) || '20', 10)
  retryBaseDelaySeconds.value = Number.isFinite(retryBase) ? Math.max(1, retryBase) : 2
  retryMaxDelaySeconds.value = Number.isFinite(retryMax)
    ? Math.max(retryBaseDelaySeconds.value, retryMax)
    : 20
}

function persistTransferBehaviorSettings() {
  window.localStorage.setItem(AUTO_REMOVE_SUCCESSFUL_KEY, String(autoRemoveSuccessfulJobs.value))
  window.localStorage.setItem(RETRY_BASE_DELAY_KEY, String(retryBaseDelaySeconds.value))
  window.localStorage.setItem(RETRY_MAX_DELAY_KEY, String(retryMaxDelaySeconds.value))
}

function loadBackgroundOnCloseSetting() {
  const stored = window.localStorage.getItem(BACKGROUND_ON_CLOSE_KEY)
  backgroundOnClose.value = stored === null ? true : stored === 'true'
}

async function persistBackgroundOnCloseSetting() {
  window.localStorage.setItem(BACKGROUND_ON_CLOSE_KEY, String(backgroundOnClose.value))
  await invoke('set_background_on_close', { enabled: backgroundOnClose.value })
}

function loadAutoRetrySettings() {
  autoRetryTransfers.value = window.localStorage.getItem(AUTO_RETRY_TRANSFERS_KEY) !== 'false'
  const stored = Number.parseInt(window.localStorage.getItem(DEFAULT_MAX_RETRIES_KEY) || '2', 10)
  defaultMaxRetries.value = Number.isFinite(stored) ? Math.max(0, stored) : 2
}

function persistAutoRetrySettings() {
  window.localStorage.setItem(AUTO_RETRY_TRANSFERS_KEY, String(autoRetryTransfers.value))
  window.localStorage.setItem(DEFAULT_MAX_RETRIES_KEY, String(defaultMaxRetries.value))
}

async function notifyUser(title: string, body: string) {
  if (!enableNotifications.value) {
    return
  }

  const { isPermissionGranted, requestPermission, sendNotification } = await import('@tauri-apps/plugin-notification')
  let granted = await isPermissionGranted()
  if (!granted) {
    const permission = await requestPermission()
    granted = permission === 'granted'
  }

  if (!granted) {
    return
  }

  await sendNotification({ title, body })
}

function closeRemoteContextMenu() {
  remoteContextMenu.value = null
}

function openRemoteContextMenu(event: MouseEvent, target: RemoteContextTarget) {
  event.preventDefault()
  remoteContextMenu.value = {
    x: event.clientX,
    y: event.clientY,
    target
  }
}

async function updateTrayQueueStatus() {
  const running = transferQueue.value.filter((job) => job.status === 'running').length
  const queued = transferQueue.value.filter((job) => job.status === 'queued').length
  const failed = transferQueue.value.filter((job) => job.status === 'error').length

  const title = running > 0 ? `RD ${running}` : null
  const tooltip =
    running > 0 || queued > 0 || failed > 0
      ? `RustDock: ${running} running, ${queued} queued, ${failed} failed`
      : 'RustDock: idle'

  await invoke('update_tray_status', { title, tooltip })
}

async function resumePersistedQueueIfNeeded() {
  if (!autoResumeQueue.value || queueRunning.value) {
    return
  }
  const hasQueuedJobs = transferQueue.value.some((job) => job.status === 'queued')
  if (!hasQueuedJobs) {
    return
  }
  statusLine.value = 'Recovered queued transfers from the previous session.'
  await runTransferQueue()
}

function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path
}

function joinRemotePath(directory: string, name: string): string {
  if (!directory || directory === '/') {
    return `/${name}`
  }
  return `${directory.replace(/\/$/, '')}/${name}`
}

async function dialogOpen(options: Parameters<typeof import('@tauri-apps/plugin-dialog')['open']>[0]) {
  const { open } = await import('@tauri-apps/plugin-dialog')
  return open(options)
}

async function dialogSave(options: Parameters<typeof import('@tauri-apps/plugin-dialog')['save']>[0]) {
  const { save } = await import('@tauri-apps/plugin-dialog')
  return save(options)
}

async function dialogConfirm(message: string, options?: Parameters<typeof import('@tauri-apps/plugin-dialog')['confirm']>[1]) {
  const { confirm } = await import('@tauri-apps/plugin-dialog')
  return confirm(message, options)
}

async function ensureTerminalRuntime() {
  if (!terminalHost.value) {
    return
  }
  if (terminal && fitAddon) {
    return
  }

  const [{ Terminal }, { FitAddon }, { WebglAddon }] = await Promise.all([
    import('xterm'),
    import('@xterm/addon-fit'),
    import('@xterm/addon-webgl')
  ])

  terminal = new Terminal({
    cursorBlink: true,
    fontFamily: '"IBM Plex Mono", "SFMono-Regular", Consolas, monospace',
    fontSize: 14,
    lineHeight: 1.18,
    convertEol: true,
    theme: {
      background: '#07131a',
      foreground: '#e5f1ec',
      cursor: '#ffb347',
      black: '#07131a',
      brightBlack: '#466370',
      red: '#ff6b6b',
      brightRed: '#ff8d7a',
      green: '#75d69c',
      brightGreen: '#9ef0af',
      yellow: '#f8d66d',
      brightYellow: '#ffe39a',
      blue: '#5cc2ff',
      brightBlue: '#7ed2ff',
      magenta: '#d3a4ff',
      brightMagenta: '#e0b8ff',
      cyan: '#69e3db',
      brightCyan: '#95efe8',
      white: '#c3d6ce',
      brightWhite: '#ffffff'
    }
  })

  fitAddon = new FitAddon()
  terminal.loadAddon(fitAddon)

  try {
    terminal.loadAddon(new WebglAddon())
  } catch {
    statusLine.value = 'WebGL terminal renderer unavailable. Falling back to canvas.'
  }

  terminal.open(terminalHost.value)
  fitAddon.fit()

  terminal.onData((data) => {
    if (!activeTerminalId.value) {
      return
    }
    invoke('send_terminal_input', {
      sessionId: activeTerminalId.value,
      input: data
    }).catch((error) => {
      statusLine.value = renderError(error)
    })
  })

  terminal.onResize(({ cols, rows }) => {
    if (!activeTerminalId.value) {
      return
    }
    invoke('resize_terminal', {
      sessionId: activeTerminalId.value,
      cols,
      rows
    }).catch((error) => {
      statusLine.value = renderError(error)
    })
  })
}

async function installTerminal() {
  await ensureTerminalRuntime()

  resizeHandler = () => fitAddon?.fit()
  window.addEventListener('resize', resizeHandler)
}

onMounted(async () => {
  loadAutoResumeQueueSetting()
  loadAutoRetrySettings()
  loadNotificationSetting()
  loadBackgroundOnCloseSetting()
  await invoke('set_background_on_close', { enabled: backgroundOnClose.value })
  installTerminal()
  await nextTick()
  await loadSessions()
  await loadTransferQueue()
  await loadTransferEvents()
  await loadKnownHosts()
  await updateTrayQueueStatus()
  await resumePersistedQueueIfNeeded()
})

onBeforeUnmount(() => {
  if (resizeHandler) {
    window.removeEventListener('resize', resizeHandler)
  }
  if (activeTerminalId.value) {
    invoke('disconnect_terminal', { sessionId: activeTerminalId.value }).catch(() => undefined)
  }
  terminal?.dispose()
})
</script>

<template>
  <div class="shell">
    <aside class="rail">
      <div class="brand">
        <p class="eyebrow">Desktop Control Plane</p>
        <h1>RustDock</h1>
        <p class="subcopy">
          Local-first session manager with a Tauri shell and streamed terminal transport.
        </p>
      </div>

      <button class="primary" @click="startNewSession">New Session</button>

      <div class="session-list">
        <div class="section-title">
          <span>Saved Sessions</span>
          <span>{{ sessions.length }}</span>
        </div>
        <button
          v-for="session in sessions"
          :key="session.id"
          class="session-card"
          :class="{ selected: session.id === selectedSessionId }"
          @click="selectSession(session)"
        >
          <strong>{{ session.name }}</strong>
          <span>{{ session.username }}@{{ session.host }}:{{ session.port }}</span>
          <span class="badge">{{ syncLabel(session.sync_state) }}</span>
        </button>
      </div>
    </aside>

    <main class="workspace">
      <section class="panel">
        <div class="panel-head">
          <div>
            <p class="eyebrow">Session Editor</p>
            <h2>{{ form.id ? 'Edit Session' : 'Create Session' }}</h2>
          </div>
          <div class="actions">
            <button class="ghost" :disabled="busy || loading" @click="loadSessions">Reload</button>
            <button class="ghost danger" :disabled="busy || !selectedSessionId" @click="deleteSession">
              Delete
            </button>
            <button class="primary" :disabled="busy" @click="saveSession">Save</button>
          </div>
        </div>

        <div class="form-grid">
          <label>
            <span>Name</span>
            <input v-model="form.name" placeholder="Production Bastion" />
          </label>
          <label>
            <span>Host</span>
            <input v-model="form.host" placeholder="bastion.example.com" />
          </label>
          <label>
            <span>Port</span>
            <input v-model="form.port" inputmode="numeric" />
          </label>
          <label>
            <span>Username</span>
            <input v-model="form.username" placeholder="root" />
          </label>
        </div>

        <div class="form-grid compact">
          <label>
            <span>Auth Method</span>
            <select v-model="form.authType">
              <option value="private-key">Private key</option>
              <option value="agent">SSH agent</option>
              <option value="password">Password</option>
            </select>
          </label>
          <label v-if="form.authType === 'private-key'">
            <span>Private Key Path</span>
            <input v-model="form.keyPath" placeholder="~/.ssh/id_ed25519" />
          </label>
          <label v-else>
            <span>Notes</span>
            <input
              :value="form.authType === 'agent' ? 'Agent-backed session' : 'Password is not persisted in this cut'"
              disabled
            />
          </label>
        </div>

        <div class="stack">
          <label>
            <span>Tags</span>
            <input v-model="form.tagsInput" placeholder="prod, ssh, eu-west" />
          </label>
          <label>
            <span>Notes</span>
            <textarea v-model="form.notes" rows="4" />
          </label>
        </div>

        <div class="form-grid">
          <label>
            <span>Local Bookmarks</span>
            <textarea v-model="form.localRootsInput" rows="5" placeholder="/srv/app&#10;/var/log" />
          </label>
          <label>
            <span>Remote Bookmarks</span>
            <textarea v-model="form.remoteRootsInput" rows="5" placeholder="/home/root&#10;/var/www" />
          </label>
        </div>
      </section>

      <section class="panel terminal-panel">
        <div class="panel-head">
          <div>
            <p class="eyebrow">Live Terminal</p>
            <h2>{{ activeTerminalName || 'xterm.js viewport' }}</h2>
          </div>
          <div class="terminal-meta">
            <span class="status-pill" :data-state="terminalStatus.toLowerCase()">{{ terminalStatus }}</span>
            <button class="ghost" :disabled="busy || !selectedSessionId" @click="connectTerminal">Connect</button>
            <button class="ghost danger" :disabled="!activeTerminalId" @click="disconnectTerminal">
              Disconnect
            </button>
          </div>
        </div>

        <div class="terminal-auth">
          <label>
            <span>{{ secretPrompt }}</span>
            <input
              v-model="connectSecret"
              type="password"
              autocomplete="off"
              :placeholder="secretPrompt"
            />
          </label>
          <div class="actions">
            <label class="checkbox-row">
              <input v-model="rememberSecret" type="checkbox" />
              <span>Remember in system keychain</span>
            </label>
            <button class="ghost" :disabled="!selectedSessionId || !connectSecret.trim()" @click="saveCurrentSecret">
              Save Secret
            </button>
            <button class="ghost danger" :disabled="!selectedSessionId" @click="forgetSavedSecret">
              Forget Secret
            </button>
          </div>
          <p>
            This secret is used only for the current connection attempt and is not saved to SQLite.
          </p>
        </div>

        <div ref="terminalHost" class="terminal-host"></div>

        <div class="terminal-footer">
          <div>
            <strong>Transport</strong>
            <p>Tauri Channel stream backed by a real russh SSH session.</p>
          </div>
          <div>
            <strong>Focus model</strong>
            <p>Keyboard input is captured directly from xterm and forwarded to the backend session.</p>
          </div>
        </div>
      </section>

      <section class="panel">
        <div class="panel-head">
          <div>
            <p class="eyebrow">SFTP</p>
            <h2>Remote Browser</h2>
          </div>
          <div class="actions">
            <button class="ghost" :disabled="!selectedRemotePaths.length" @click="clearRemoteSelection">
              Clear Selection
            </button>
            <button class="ghost" :disabled="queueRunning || sftpBusy || !selectedSessionId" @click="batchQueueUploads">
              Batch Queue Uploads
            </button>
            <button class="ghost" :disabled="!selectedRemotePaths.length || sftpBusy" @click="batchQueueDownloads">
              Batch Queue Downloads
            </button>
            <button class="ghost danger" :disabled="!selectedRemotePaths.length || sftpBusy" @click="batchDeleteSelectedRemote">
              Batch Delete
            </button>
            <button class="ghost" :disabled="sftpBusy" @click="goToParentDirectory">Parent</button>
            <button class="ghost" :disabled="sftpBusy || !selectedSessionId" @click="loadSftpDirectory()">
              Refresh
            </button>
          </div>
        </div>

        <div class="sftp-toolbar">
          <label>
            <span>Remote Directory</span>
            <input v-model="sftpPath" placeholder="/" />
          </label>
          <button class="primary" :disabled="sftpBusy || !selectedSessionId" @click="loadSftpDirectory()">
            Load Directory
          </button>
        </div>

        <div class="sftp-grid">
          <div class="sftp-browser-layout">
            <div class="tree-panel">
              <div class="section-title">
                <span>Directory Tree</span>
                <span>{{ visibleRemoteTreeNodes.length }}</span>
              </div>
              <button
                v-for="node in visibleRemoteTreeNodes"
                :key="node.path"
                class="tree-node"
                :style="{ paddingLeft: `${12 + node.depth * 18}px` }"
                @click="openRemoteTreeNode(node)"
              >
                <span class="tree-toggle" @click.stop="toggleRemoteTreeNode(node)">
                  {{ node.loading ? '…' : node.expanded ? '▾' : '▸' }}
                </span>
                <span class="tree-label">{{ node.name }}</span>
              </button>
              <p v-if="!visibleRemoteTreeNodes.length" class="empty-copy">
                Save a session with at least one remote root to initialize the tree.
              </p>
            </div>

            <div class="sftp-list">
              <div class="section-title">
                <span>Current Directory</span>
                <span>{{ sftpEntries.length }}</span>
              </div>
              <button
                v-for="entry in sftpEntries"
                :key="entry.path"
                class="sftp-entry"
                @click="selectRemotePathForMutation(entry)"
                @dblclick="openRemoteEntry(entry)"
              >
                <label class="checkbox-row">
                  <input
                    :checked="isRemoteSelected(entry.path)"
                    type="checkbox"
                    @click.stop
                    @change="toggleRemoteSelection(entry)"
                  />
                  <span>Select</span>
                </label>
                <strong>{{ entry.is_dir ? 'DIR' : 'FILE' }}</strong>
                <span>{{ entry.name }}</span>
                <small>{{ entry.path }}</small>
              </button>
              <p v-if="!sftpEntries.length" class="empty-copy">
                Load a directory to browse the remote filesystem.
              </p>
            </div>
          </div>

          <div class="sftp-transfer">
            <label>
              <span>New Directory Path</span>
              <input v-model="sftpCreatePath" placeholder="/tmp/new-folder" />
            </label>
            <button class="ghost" :disabled="sftpBusy" @click="createRemoteDirectory">
              Create Directory
            </button>

            <label>
              <span>Remote Path</span>
              <input v-model="remoteTransferPath" placeholder="/root/example.txt" />
            </label>
            <label>
              <span>Rename Target</span>
              <input v-model="sftpRenameTarget" placeholder="/root/example-renamed.txt" />
            </label>
            <label>
              <span>Local Path</span>
              <input v-model="localTransferPath" placeholder="/root/downloads/example.txt" />
            </label>

            <div class="actions">
              <button class="ghost" :disabled="sftpBusy || queueRunning" @click="chooseUploadLocalFile">
                Pick File
              </button>
              <button class="ghost" :disabled="sftpBusy || queueRunning" @click="chooseDownloadLocalPath">
                Pick Save Path
              </button>
              <button class="ghost" :disabled="sftpBusy" @click="renameRemotePath">
                Rename
              </button>
              <button class="ghost danger" :disabled="sftpBusy" @click="deleteRemotePath">
                Delete
              </button>
              <button class="ghost" :disabled="sftpBusy || queueRunning" @click="queueDownloadJob">
                Queue Download
              </button>
              <button class="ghost" :disabled="sftpBusy || queueRunning" @click="queueUploadJob">
                Queue Upload
              </button>
              <button class="ghost" :disabled="sftpBusy" @click="downloadSelectedRemoteFile">
                Download
              </button>
              <button class="primary" :disabled="sftpBusy" @click="uploadLocalFile">
                Upload
              </button>
            </div>

            <div class="transfer-queue">
              <div class="queue-head">
                <strong>Transfer Queue</strong>
                <div class="actions">
                  <label class="checkbox-row">
                    <input v-model="autoResumeQueue" type="checkbox" @change="persistAutoResumeQueueSetting" />
                    <span>Auto resume on launch</span>
                  </label>
                  <label class="checkbox-row">
                    <input v-model="backgroundOnClose" type="checkbox" @change="persistBackgroundOnCloseSetting" />
                    <span>Close to tray</span>
                  </label>
                  <label class="checkbox-row">
                    <input v-model="enableNotifications" type="checkbox" @change="persistNotificationSetting" />
                    <span>System notifications</span>
                  </label>
                  <label class="checkbox-row">
                    <input v-model="autoRetryTransfers" type="checkbox" @change="persistAutoRetrySettings" />
                    <span>Auto retry transient failures</span>
                  </label>
                  <label class="checkbox-row">
                    <input v-model="autoRemoveSuccessfulJobs" type="checkbox" @change="persistTransferBehaviorSettings" />
                    <span>Auto remove successful jobs</span>
                  </label>
                  <label>
                    <span>Retries</span>
                    <input
                      v-model="defaultMaxRetries"
                      type="number"
                      min="0"
                      max="9"
                      @change="persistAutoRetrySettings"
                    />
                  </label>
                  <label>
                    <span>Base Delay</span>
                    <input
                      v-model="retryBaseDelaySeconds"
                      type="number"
                      min="1"
                      max="60"
                      @change="persistTransferBehaviorSettings"
                    />
                  </label>
                  <label>
                    <span>Max Delay</span>
                    <input
                      v-model="retryMaxDelaySeconds"
                      type="number"
                      min="1"
                      max="300"
                      @change="persistTransferBehaviorSettings"
                    />
                  </label>
                  <button class="ghost" :disabled="queueRunning || !transferQueue.length" @click="runTransferQueue">
                    {{ queueRunning ? 'Running...' : 'Run Queue' }}
                  </button>
                  <button class="ghost" :disabled="!queueRunning" @click="requestQueueStop">
                    Stop After Current
                  </button>
                  <button class="ghost" :disabled="!transferQueue.length" @click="clearCompletedTransfers">
                    Clear Completed
                  </button>
                </div>
              </div>

              <div v-if="transferQueue.length" class="queue-list">
                <div v-for="job in transferQueue" :key="job.id" class="queue-item">
                  <strong>{{ job.kind.toUpperCase() }}</strong>
                  <span>{{ job.remotePath }}</span>
                  <small>{{ job.localPath }}</small>
                  <small>attempt {{ job.attemptCount }}/{{ job.maxRetries + 1 }}</small>
                  <small>{{ job.status }} · {{ job.message }}</small>
                  <div v-if="job.status === 'running'" class="queue-progress">
                    <div
                      class="queue-progress-bar"
                      :style="{ width: `${job.total ? Math.min(100, Math.round(((job.transferred ?? 0) / job.total) * 100)) : 15}%` }"
                    ></div>
                  </div>
                  <div class="actions">
                    <button class="ghost" :disabled="job.status === 'running'" @click="retryTransferJob(job.id)">
                      Retry
                    </button>
                    <button class="ghost danger" :disabled="job.status !== 'running'" @click="cancelRunningTransfer(job.id)">
                      Cancel
                    </button>
                    <button class="ghost danger" :disabled="job.status === 'running'" @click="removeTransferJob(job.id)">
                      Remove
                    </button>
                  </div>
                </div>
              </div>
              <p v-else class="empty-copy">
                Queue uploads and downloads here to process them in order.
              </p>
            </div>

            <p class="empty-copy">
              File picker and a basic ordered queue are now in place. Confirmation dialogs and richer progress UI come next.
            </p>
          </div>
        </div>
      </section>

      <section class="panel">
        <div class="panel-head">
          <div>
            <p class="eyebrow">Transfer Activity</p>
            <h2>Recent Events</h2>
          </div>
          <div class="actions">
            <label>
              <span>Search</span>
              <input v-model="transferEventQuery" placeholder="job id, message, session id" />
            </label>
            <label>
              <span>Level</span>
              <select v-model="transferEventLevelFilter">
                <option value="all">All</option>
                <option value="info">Info</option>
                <option value="warning">Warning</option>
                <option value="error">Error</option>
              </select>
            </label>
            <button class="ghost" @click="loadTransferEvents">Refresh</button>
            <button class="ghost danger" :disabled="!transferEvents.length" @click="clearTransferEvents">
              Clear Log
            </button>
          </div>
        </div>

        <div class="known-hosts-list">
          <div v-for="event in filteredTransferEvents" :key="event.id" class="known-host-entry">
            <strong>{{ event.level.toUpperCase() }} · {{ event.job_id }}</strong>
            <span>{{ event.message }}</span>
            <small>{{ new Date(event.created_at * 1000).toLocaleString() }}</small>
          </div>
          <p v-if="!filteredTransferEvents.length" class="empty-copy">
            No transfer events recorded yet.
          </p>
        </div>
      </section>

      <section class="panel">
        <div class="panel-head">
          <div>
            <p class="eyebrow">Host Trust</p>
            <h2>known_hosts</h2>
          </div>
          <div class="actions">
            <button class="ghost" @click="loadKnownHosts">Refresh</button>
            <button class="ghost danger" :disabled="!selectedSessionId" @click="forgetSelectedSessionKnownHost">
              Forget Selected Host
            </button>
          </div>
        </div>

        <div class="known-hosts-list">
          <div v-for="entry in knownHosts" :key="entry.line" class="known-host-entry">
            <strong>#{{ entry.line }} · {{ entry.key_type }}</strong>
            <span>{{ entry.hosts }}</span>
            <small>{{ entry.hashed ? 'Hashed host pattern' : 'Plain host pattern' }}</small>
            <div class="actions">
              <button class="ghost danger" @click="removeKnownHostLine(entry)">Remove</button>
            </div>
          </div>
          <p v-if="!knownHosts.length" class="empty-copy">
            No known_hosts entries found for this user yet.
          </p>
        </div>
      </section>
    </main>

    <footer class="status-bar">
      <span>{{ statusLine }}</span>
      <span v-if="selectedSession">Selected: {{ selectedSession.name }}</span>
      <span v-else>No saved session selected.</span>
    </footer>
  </div>
</template>
