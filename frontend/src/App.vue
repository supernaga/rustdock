<script setup lang="ts">
import { Channel, invoke } from '@tauri-apps/api/core'
import type { FitAddon as XTermFitAddon } from '@xterm/addon-fit'
import type { Terminal as XTermTerminal } from '@xterm/xterm'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'

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
type DockTab = 'browser' | 'editor' | 'queue' | 'activity' | 'hosts'
type TransferChannelMessage =
  | { kind: 'started'; job_id: string }
  | { kind: 'progress'; job_id: string; transferred: number; total: number | null }
  | { kind: 'completed'; job_id: string; bytes: number; path: string }
  | { kind: 'failed'; job_id: string; error: string }
  | { kind: 'cancelled'; job_id: string }

interface RemoteTextFilePayload {
  path: string
  content: string
  bytes: number
}

interface TerminalTab {
  sessionId: string
  sessionName: string
  status: SessionStatus
  buffer: string
  unread: number
  connectedAt: number
}

interface SessionWorkspaceState {
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
const statusLine = ref('就绪。')
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
const sessionFilter = ref('')
const activeDockTab = ref<DockTab>('browser')
const terminalTabs = ref<TerminalTab[]>([])
const remoteEditorPath = ref('')
const remoteEditorContent = ref('')
const remoteEditorOriginalContent = ref('')
const remoteEditorLoading = ref(false)
const sessionWorkspace = ref<Record<string, SessionWorkspaceState>>({})
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
const dockTabs: Array<{ id: DockTab; label: string }> = [
  { id: 'browser', label: 'SSH 浏览器' },
  { id: 'editor', label: '远程编辑器' },
  { id: 'queue', label: '传输队列' },
  { id: 'activity', label: '活动日志' },
  { id: 'hosts', label: 'known_hosts' }
]

const selectedSession = computed(() =>
  sessions.value.find((session) => session.id === selectedSessionId.value) ?? null
)

const filteredSessions = computed(() => {
  const query = sessionFilter.value.trim().toLowerCase()
  const sorted = [...sessions.value].sort((left, right) => {
    const leftStamp = left.last_connected_at ?? left.updated_at
    const rightStamp = right.last_connected_at ?? right.updated_at
    return rightStamp - leftStamp
  })

  if (!query) {
    return sorted
  }

  return sorted.filter((session) => {
    const haystack = [
      session.name,
      session.host,
      session.username,
      session.tags.join(' '),
      syncLabel(session.sync_state)
    ]
      .join(' ')
      .toLowerCase()
    return haystack.includes(query)
  })
})

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

const visibleTransferEvents = computed(() => filteredTransferEvents.value.slice(0, 14))
const activeTerminalTab = computed(() =>
  terminalTabs.value.find((tab) => tab.sessionId === activeTerminalId.value) ?? null
)
const orderedTerminalTabs = computed(() =>
  [...terminalTabs.value].sort((left, right) => right.connectedAt - left.connectedAt)
)
const queuedTransferCount = computed(
  () => transferQueue.value.filter((job) => job.status === 'queued').length
)
const runningTransferCount = computed(
  () => transferQueue.value.filter((job) => job.status === 'running').length
)
const failedTransferCount = computed(
  () => transferQueue.value.filter((job) => job.status === 'error').length
)
const selectedSessionSummary = computed(() => {
  if (!selectedSession.value) {
    return '先在左侧选择一个已保存会话，或先新建一个草稿。'
  }
  return `${selectedSession.value.username}@${selectedSession.value.host}:${selectedSession.value.port}`
})
const selectedSessionRemoteRoot = computed(
  () => selectedSession.value?.remote_roots[0]?.trim() || '/'
)
const selectedSessionAuthLabel = computed(() => {
  if (!selectedSession.value) {
    return '未选择认证方式'
  }
  if (selectedSession.value.auth_method === 'Password') {
    return '密码认证'
  }
  if (selectedSession.value.auth_method === 'Agent') {
    return 'SSH 代理'
  }
  return `密钥 ${basename(selectedSession.value.auth_method.PrivateKey.path)}`
})
const selectedSessionTagSummary = computed(() => {
  if (!selectedSession.value?.tags.length) {
    return '无标签'
  }
  return selectedSession.value.tags.join(' · ')
})
const remoteEditorDirty = computed(
  () => remoteEditorPath.value.length > 0 && remoteEditorContent.value !== remoteEditorOriginalContent.value
)
const remoteEditorTitle = computed(() =>
  remoteEditorPath.value ? basename(remoteEditorPath.value) : '未打开文件'
)
const editorConnectLabel = computed(() =>
  form.value.id || selectedSessionId.value ? '保存并连接' : '创建并连接'
)

const secretPrompt = computed(() => {
  if (!selectedSession.value) {
    return '本次连接使用的凭据'
  }
  if (selectedSession.value.auth_method === 'Password') {
    return '密码'
  }
  if (typeof selectedSession.value.auth_method === 'object' && 'PrivateKey' in selectedSession.value.auth_method) {
    return '密钥口令（可选）'
  }
  return '本次连接使用的凭据'
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
    statusLine.value = `已加载 ${sessions.value.length} 个会话。`
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

async function loadKnownHosts() {
  try {
    knownHosts.value = await invoke<KnownHostEntry[]>('list_known_hosts_entries')
  } catch (error) {
    statusLine.value = renderError(error)
  }
}

function startNewSession() {
  persistSessionWorkspace()
  selectedSessionId.value = null
  form.value = newDraft()
  sftpPath.value = '/'
  sftpEntries.value = []
  remoteTreeRoots.value = []
  remoteTransferPath.value = ''
  localTransferPath.value = ''
  selectedRemotePaths.value = []
  sftpCreatePath.value = ''
  sftpRenameTarget.value = ''
  remoteEditorPath.value = ''
  remoteEditorContent.value = ''
  remoteEditorOriginalContent.value = ''
  connectSecret.value = ''
  rememberSecret.value = false
  activeDockTab.value = 'browser'
  statusLine.value = '已重置草稿。'
}

async function selectSession(session: SessionProfile) {
  if (selectedSessionId.value && selectedSessionId.value !== session.id) {
    persistSessionWorkspace(selectedSessionId.value)
  }
  selectedSessionId.value = session.id
  form.value = draftFromSession(session)
  await restoreSessionContext(session)
  statusLine.value = `已选择 ${session.name}。`
}

async function connectSessionFromList(session: SessionProfile) {
  await selectSession(session)
  await nextTick()
  await connectTerminal()
}

function trimTerminalBuffer(buffer: string): string {
  const maxChars = 200_000
  if (buffer.length <= maxChars) {
    return buffer
  }
  return buffer.slice(buffer.length - maxChars)
}

function cloneRemoteEntries(entries: RemoteDirEntry[]): RemoteDirEntry[] {
  return entries.map((entry) => ({ ...entry }))
}

function cloneRemoteTreeNodes(nodes: RemoteTreeNode[]): RemoteTreeNode[] {
  return nodes.map((node) => ({
    ...node,
    children: cloneRemoteTreeNodes(node.children)
  }))
}

function buildRemoteTreeRoots(session: SessionProfile): RemoteTreeNode[] {
  const roots = session.remote_roots.length > 0 ? session.remote_roots : ['/']
  const uniqueRoots = Array.from(new Set(roots.map((root) => root.trim()).filter(Boolean)))
  return uniqueRoots.map((rootPath) =>
    makeRemoteTreeNode(rootPath, rootPath === '/' ? '/' : basename(rootPath), 0)
  )
}

function defaultWorkspaceState(session: SessionProfile): SessionWorkspaceState {
  return {
    connectSecret: '',
    rememberSecret: false,
    secretHydrated: false,
    sftpPath: session.remote_roots[0] || '/',
    sftpEntries: [],
    remoteTreeRoots: buildRemoteTreeRoots(session),
    remoteTransferPath: '',
    localTransferPath: '',
    remoteTransferIsDir: false,
    selectedRemotePaths: [],
    sftpCreatePath: '',
    sftpRenameTarget: '',
    remoteEditorPath: '',
    remoteEditorContent: '',
    remoteEditorOriginalContent: '',
    activeDockTab: 'browser'
  }
}

function getWorkspaceState(session: SessionProfile): SessionWorkspaceState {
  const existing = sessionWorkspace.value[session.id]
  if (existing) {
    return existing
  }

  const created = defaultWorkspaceState(session)
  sessionWorkspace.value = {
    ...sessionWorkspace.value,
    [session.id]: created
  }
  return created
}

function persistSessionWorkspace(sessionId = selectedSessionId.value) {
  if (!sessionId) {
    return
  }

  const session = sessions.value.find((entry) => entry.id === sessionId)
  if (!session) {
    return
  }
  const existing = sessionWorkspace.value[sessionId]
  const secretHydrated =
    connectSecret.value.trim().length > 0 ||
    rememberSecret.value ||
    existing?.secretHydrated ||
    false

  sessionWorkspace.value = {
    ...sessionWorkspace.value,
    [sessionId]: {
      connectSecret: connectSecret.value,
      rememberSecret: rememberSecret.value,
      secretHydrated,
      sftpPath: sftpPath.value,
      sftpEntries: cloneRemoteEntries(sftpEntries.value),
      remoteTreeRoots: cloneRemoteTreeNodes(remoteTreeRoots.value),
      remoteTransferPath: remoteTransferPath.value,
      localTransferPath: localTransferPath.value,
      remoteTransferIsDir: remoteTransferIsDir.value,
      selectedRemotePaths: [...selectedRemotePaths.value],
      sftpCreatePath: sftpCreatePath.value,
      sftpRenameTarget: sftpRenameTarget.value,
      remoteEditorPath: remoteEditorPath.value,
      remoteEditorContent: remoteEditorContent.value,
      remoteEditorOriginalContent: remoteEditorOriginalContent.value,
      activeDockTab: activeDockTab.value
    }
  }
}

function applyWorkspaceState(session: SessionProfile) {
  const state = getWorkspaceState(session)
  sftpPath.value = state.sftpPath
  sftpEntries.value = cloneRemoteEntries(state.sftpEntries)
  remoteTreeRoots.value = cloneRemoteTreeNodes(state.remoteTreeRoots)
  remoteTransferPath.value = state.remoteTransferPath
  localTransferPath.value = state.localTransferPath
  remoteTransferIsDir.value = state.remoteTransferIsDir
  selectedRemotePaths.value = [...state.selectedRemotePaths]
  sftpCreatePath.value = state.sftpCreatePath
  sftpRenameTarget.value = state.sftpRenameTarget
  remoteEditorPath.value = state.remoteEditorPath
  remoteEditorContent.value = state.remoteEditorContent
  remoteEditorOriginalContent.value = state.remoteEditorOriginalContent
  connectSecret.value = state.connectSecret
  rememberSecret.value = state.rememberSecret
  activeDockTab.value = state.activeDockTab
}

async function ensureSessionSecret(session: SessionProfile) {
  const state = getWorkspaceState(session)
  if (state.secretHydrated) {
    connectSecret.value = state.connectSecret
    rememberSecret.value = state.rememberSecret
    return
  }

  try {
    const secret = await invoke<string | null>('load_session_secret', { sessionId: session.id })
    state.connectSecret = secret || ''
    state.rememberSecret = Boolean(secret)
    state.secretHydrated = true
    sessionWorkspace.value = {
      ...sessionWorkspace.value,
      [session.id]: state
    }
    connectSecret.value = state.connectSecret
    rememberSecret.value = state.rememberSecret
  } catch (error) {
    statusLine.value = renderError(error)
  }
}

async function restoreSessionContext(session: SessionProfile) {
  applyWorkspaceState(session)
  await ensureSessionSecret(session)
}

function findTerminalTab(sessionId: string): TerminalTab | null {
  return terminalTabs.value.find((tab) => tab.sessionId === sessionId) ?? null
}

function setTerminalTabStatus(sessionId: string, status: SessionStatus) {
  const tab = findTerminalTab(sessionId)
  if (!tab) {
    return
  }
  tab.status = status
  if (activeTerminalId.value === sessionId) {
    terminalStatus.value = status
  }
}

function appendTerminalOutput(sessionId: string, chunk: string) {
  const tab = findTerminalTab(sessionId)
  if (!tab) {
    return
  }
  tab.buffer = trimTerminalBuffer(`${tab.buffer}${chunk}`)
  if (activeTerminalId.value === sessionId && terminal) {
    terminal.write(chunk)
  } else {
    tab.unread += 1
  }
}

function syncTerminalViewport() {
  if (!terminal) {
    return
  }
  terminal.reset()
  if (activeTerminalTab.value?.buffer) {
    terminal.write(activeTerminalTab.value.buffer)
  }
  fitAddon?.fit()
}

async function activateTerminalTab(sessionId: string) {
  const tab = findTerminalTab(sessionId)
  if (!tab) {
    return
  }
  if (selectedSessionId.value && selectedSessionId.value !== sessionId) {
    persistSessionWorkspace(selectedSessionId.value)
  }
  activeTerminalId.value = tab.sessionId
  activeTerminalName.value = tab.sessionName
  terminalStatus.value = tab.status
  tab.unread = 0
  selectedSessionId.value = tab.sessionId
  const session = sessions.value.find((entry) => entry.id === tab.sessionId)
  if (session) {
    form.value = draftFromSession(session)
    await restoreSessionContext(session)
  }
  syncTerminalViewport()
}

function registerTerminalTab(connection: TerminalConnection): TerminalTab {
  const existing = findTerminalTab(connection.session_id)
  if (existing) {
    existing.sessionName = connection.session_name
    existing.connectedAt = Date.now()
    existing.unread = 0
    return existing
  }

  const tab: TerminalTab = {
    sessionId: connection.session_id,
    sessionName: connection.session_name,
    status: 'Connecting',
    buffer: '',
    unread: 0,
    connectedAt: Date.now()
  }
  terminalTabs.value = [tab, ...terminalTabs.value]
  return tab
}

function removeTerminalTab(sessionId: string) {
  terminalTabs.value = terminalTabs.value.filter((tab) => tab.sessionId !== sessionId)
  if (activeTerminalId.value === sessionId) {
    const nextTab = terminalTabs.value[0] ?? null
    if (nextTab) {
      void activateTerminalTab(nextTab.sessionId)
    } else {
      activeTerminalId.value = null
      activeTerminalName.value = ''
      terminalStatus.value = 'Disconnected'
      syncTerminalViewport()
    }
  }
}

async function saveSession(): Promise<SessionProfile | null> {
  busy.value = true
  try {
    const payload = toPayload(form.value)
    const saved = await invoke<SessionProfile>('save_session', { draft: payload })
    await loadSessions()
    selectedSessionId.value = saved.id
    form.value = draftFromSession(saved)
    statusLine.value = `已保存 ${saved.name}。`
    return saved
  } catch (error) {
    statusLine.value = renderError(error)
    return null
  } finally {
    busy.value = false
  }
}

async function deleteSession() {
  if (!selectedSessionId.value) {
    statusLine.value = '请先选择一个会话。'
    return
  }

  const accepted = await dialogConfirm(
    `确认删除会话“${selectedSession.value?.name ?? selectedSessionId.value}”吗？`,
    { title: '删除会话', kind: 'warning' }
  )
  if (!accepted) {
    statusLine.value = '已取消删除会话。'
    return
  }

  busy.value = true
  try {
    const deletedId = selectedSessionId.value
    await invoke('delete_session', { sessionId: deletedId })
    const { [deletedId]: _deletedWorkspace, ...remainingWorkspaces } = sessionWorkspace.value
    sessionWorkspace.value = remainingWorkspaces
    removeTerminalTab(deletedId)
    if (!activeTerminalId.value) {
      selectedSessionId.value = null
      form.value = newDraft()
    }
    await loadSessions()
    statusLine.value = '会话已删除。'
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    busy.value = false
  }
}

async function saveAndConnectSession() {
  statusLine.value = '正在保存并连接...'
  const saved = await saveSession()
  if (!saved) {
    return
  }
  await selectSession(saved)
  await nextTick()
  await connectTerminal()
}

async function connectTerminal() {
  if (!selectedSessionId.value) {
    statusLine.value = '连接前请先保存并选中会话。'
    return
  }
  if (!terminal || !fitAddon) {
    statusLine.value = '终端尚未准备好。'
    return
  }

  fitAddon.fit()
  const cols = terminal.cols || 120
  const rows = terminal.rows || 32

  const existingTab = findTerminalTab(selectedSessionId.value)
  if (existingTab && existingTab.status !== 'Disconnected' && existingTab.status !== 'Failed') {
    await activateTerminalTab(existingTab.sessionId)
    activeDockTab.value = 'browser'
    const sftpLoaded = await loadSftpDirectory(selectedSessionRemoteRoot.value)
    terminal.focus()
    if (sftpLoaded) {
      statusLine.value = `已切换到 ${existingTab.sessionName}。`
    }
    return
  }
  if (existingTab) {
    removeTerminalTab(existingTab.sessionId)
  }

  busy.value = true
  terminalStatus.value = 'Connecting'

  try {
    if (rememberSecret.value && connectSecret.value.trim()) {
      await invoke('save_session_secret', {
        sessionId: selectedSessionId.value,
        secret: connectSecret.value.trim()
      })
    }

    const sessionId = selectedSessionId.value
    const fallbackSessionName = selectedSession.value?.name ?? sessionId
    const secret = await resolveSecretForSession(sessionId)
    const channel = new Channel<TerminalStreamMessage>()
    channel.onmessage = (message) => {
      if (message.kind === 'output') {
        appendTerminalOutput(sessionId, message.data)
      } else if (message.kind === 'status') {
        setTerminalTabStatus(sessionId, message.status)
      } else if (message.kind === 'closed') {
        setTerminalTabStatus(sessionId, 'Disconnected')
      }
    }

    const connection = await invoke<TerminalConnection>('connect_terminal', {
      sessionId,
      cols,
      rows,
      secret,
      channel
    })

    const tab = registerTerminalTab(connection)
    tab.status = 'Connecting'
    if (tab.buffer.length === 0) {
      tab.buffer = trimTerminalBuffer(`\x1b[1;34m[workspace]\x1b[0m Opening ${fallbackSessionName}\r\n`)
    }
    await activateTerminalTab(connection.session_id)
    activeDockTab.value = 'browser'
    const sftpLoaded = await loadSftpDirectory(selectedSessionRemoteRoot.value)
    const sftpStatus = statusLine.value
    terminal.focus()
    await loadSessions()
    await loadKnownHosts()
    statusLine.value = sftpLoaded ? `终端已连接到 ${connection.session_name}。` : sftpStatus
  } catch (error) {
    terminalStatus.value = 'Failed'
    statusLine.value = renderError(error)
  } finally {
    busy.value = false
  }
}

async function disconnectTerminal() {
  if (!activeTerminalId.value) {
    statusLine.value = '当前没有活动终端会话。'
    return
  }

  try {
    const sessionId = activeTerminalId.value
    await invoke('disconnect_terminal', { sessionId })
    removeTerminalTab(sessionId)
    statusLine.value = '终端已断开。'
  } catch (error) {
    statusLine.value = renderError(error)
  }
}

async function closeTerminalTab(sessionId: string) {
  const tab = findTerminalTab(sessionId)
  if (!tab) {
    return
  }

  if (tab.status === 'Connected' || tab.status === 'Connecting') {
    try {
      await invoke('disconnect_terminal', { sessionId })
    } catch {
      // Best effort close; remove the tab locally even if the backend already dropped it.
    }
  }

  removeTerminalTab(sessionId)
}

function closeTerminalState() {
  if (!activeTerminalId.value) {
    terminalStatus.value = 'Disconnected'
    return
  }
  removeTerminalTab(activeTerminalId.value)
}

async function loadSftpDirectory(path?: string): Promise<boolean> {
  if (!selectedSessionId.value) {
    statusLine.value = '请先选择一个已保存的会话。'
    return false
  }

  sftpBusy.value = true
  try {
    const sessionId = selectedSessionId.value
    const secret = await resolveSecretForSession(sessionId)
    const listing = await invoke<RemoteDirectoryListing>('list_sftp_dir', {
      request: {
        session_id: sessionId,
        path: path ?? sftpPath.value,
        secret
      }
    })
    sftpPath.value = listing.directory
    sftpEntries.value = listing.entries
    selectedRemotePaths.value = selectedRemotePaths.value.filter((selectedPath) =>
      listing.entries.some((entry) => entry.path === selectedPath)
    )
    statusLine.value = `已加载 ${listing.directory} 下的 ${listing.entries.length} 个条目。`
    return true
  } catch (error) {
    statusLine.value = renderError(error)
    return false
  } finally {
    sftpBusy.value = false
  }
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
  activeDockTab.value = 'editor'
  void openRemoteTextFile(entry.path)
}

function selectRemotePathForMutation(entry: RemoteDirEntry) {
  remoteTransferPath.value = entry.path
  remoteTransferIsDir.value = entry.is_dir
  sftpRenameTarget.value = entry.path
  statusLine.value = `已选择 ${entry.path}。`
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
    statusLine.value = '请先选择一个已保存的会话。'
    return
  }
  if (!remoteTransferPath.value.trim() || !localTransferPath.value.trim()) {
    statusLine.value = '下载前请先填写远程路径和本地路径。'
    return
  }

  sftpBusy.value = true
  try {
    const sessionId = selectedSessionId.value
    const secret = await resolveSecretForSession(sessionId)
    const result = await invoke<TransferResult>('download_sftp_file', {
      request: {
        session_id: sessionId,
        remote_path: remoteTransferPath.value.trim(),
        local_path: localTransferPath.value.trim(),
        secret
      }
    })
    statusLine.value = `已下载 ${result.bytes} 字节到 ${result.path}。`
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    sftpBusy.value = false
  }
}

async function uploadLocalFile() {
  if (!selectedSessionId.value) {
    statusLine.value = '请先选择一个已保存的会话。'
    return
  }
  if (!remoteTransferPath.value.trim() || !localTransferPath.value.trim()) {
    statusLine.value = '上传前请先填写本地路径和远程路径。'
    return
  }

  sftpBusy.value = true
  try {
    const sessionId = selectedSessionId.value
    const secret = await resolveSecretForSession(sessionId)
    const result = await invoke<TransferResult>('upload_sftp_file', {
      request: {
        session_id: sessionId,
        local_path: localTransferPath.value.trim(),
        remote_path: remoteTransferPath.value.trim(),
        secret
      }
    })
    statusLine.value = `已上传 ${result.bytes} 字节到 ${result.path}。`
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
    statusLine.value = '请先选择一个已保存的会话。'
    return
  }
  if (!remoteTransferPath.value.trim() || !localTransferPath.value.trim()) {
    statusLine.value = '加入下载队列前请先填写远程路径和本地路径。'
    return
  }

  const job: TransferJob = {
    id: `${Date.now()}-download`,
    kind: 'download',
    sessionId: selectedSessionId.value,
    remotePath: remoteTransferPath.value.trim(),
    localPath: localTransferPath.value.trim(),
    status: 'queued',
    message: '等待中',
    attemptCount: 0,
    maxRetries: defaultMaxRetries.value,
    createdAt: nowEpoch(),
    updatedAt: nowEpoch()
  }
  transferQueue.value.push(job)
  await persistTransferJob(job)
  await appendTransferEvent(job, 'info', `已加入下载队列：${job.remotePath}`)
  await updateTrayQueueStatus()
  statusLine.value = '已加入下载队列。'
}

async function queueUploadJob() {
  if (!selectedSessionId.value) {
    statusLine.value = '请先选择一个已保存的会话。'
    return
  }
  if (!remoteTransferPath.value.trim() || !localTransferPath.value.trim()) {
    statusLine.value = '加入上传队列前请先填写本地路径和远程路径。'
    return
  }

  const job: TransferJob = {
    id: `${Date.now()}-upload`,
    kind: 'upload',
    sessionId: selectedSessionId.value,
    remotePath: remoteTransferPath.value.trim(),
    localPath: localTransferPath.value.trim(),
    status: 'queued',
    message: '等待中',
    attemptCount: 0,
    maxRetries: defaultMaxRetries.value,
    createdAt: nowEpoch(),
    updatedAt: nowEpoch()
  }
  transferQueue.value.push(job)
  await persistTransferJob(job)
  await appendTransferEvent(job, 'info', `已加入上传队列：${job.localPath}`)
  await updateTrayQueueStatus()
  statusLine.value = '已加入上传队列。'
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

    statusLine.value = queueStopRequested.value ? '传输队列已暂停。' : '传输队列已完成。'
    if (!queueStopRequested.value) {
      await notifyUser('传输队列完成', '所有排队任务都已处理完成。')
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
  statusLine.value = '队列会在当前任务完成后停止。'
}

async function retryTransferJob(jobId: string) {
  const job = transferQueue.value.find((entry) => entry.id === jobId)
  if (!job || job.status === 'running') {
    return
  }
  job.status = 'queued'
  job.message = '已加入重试队列'
  delete job.bytes
  delete job.transferred
  delete job.total
  job.updatedAt = nowEpoch()
  await persistTransferJob(job)
  await appendTransferEvent(job, 'info', `已加入重试队列：${job.remotePath}`)
}

async function removeTransferJob(jobId: string) {
  const job = transferQueue.value.find((entry) => entry.id === jobId)
  if (!job || job.status === 'running') {
    return
  }
  transferQueue.value = transferQueue.value.filter((entry) => entry.id !== jobId)
  await deletePersistedTransferJob(jobId)
  if (job) {
    await appendTransferEvent(job, 'warning', `已从队列移除任务 ${job.id}`)
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
  job.message = `开始执行第 ${job.attemptCount}/${job.maxRetries + 1} 次尝试`
  job.transferred = 0
  job.total = null
  job.updatedAt = nowEpoch()
  await persistTransferJob(job)
  await appendTransferEvent(
    job,
    'info',
    `${transferKindLabel(job.kind)}任务开始执行，第 ${job.attemptCount}/${job.maxRetries + 1} 次尝试`
  )
  const secret = await resolveSecretForSession(job.sessionId)

  await new Promise<void>((resolve) => {
    const channel = new Channel<TransferChannelMessage>()
    channel.onmessage = (message) => {
      if (message.job_id !== job.id) {
        return
      }

      if (message.kind === 'started') {
        job.message = '已连接'
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
            ? `已上传 ${message.bytes} 字节`
            : `已下载 ${message.bytes} 字节`
        void persistTransferJob(job)
        void appendTransferEvent(job, 'info', job.message)
        void notifyUser('传输完成', job.message)
        if (autoRemoveSuccessfulJobs.value) {
          void removeTransferJobInternal(job)
        } else {
          void updateTrayQueueStatus()
        }
        resolve()
      } else if (message.kind === 'cancelled') {
        job.status = 'error'
        job.message = '已取消'
        job.updatedAt = nowEpoch()
        void persistTransferJob(job)
        void appendTransferEvent(job, 'warning', `已取消${transferKindLabel(job.kind)}任务`)
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
    job.message = '正在取消...'
    job.updatedAt = nowEpoch()
    await persistTransferJob(job)
  } catch (error) {
    statusLine.value = renderError(error)
  }
}

async function createRemoteDirectory() {
  if (!selectedSessionId.value) {
    statusLine.value = '请先选择一个已保存的会话。'
    return
  }
  if (!sftpCreatePath.value.trim()) {
    statusLine.value = '请输入要创建的远程目录路径。'
    return
  }

  sftpBusy.value = true
  try {
    const sessionId = selectedSessionId.value
    const secret = await resolveSecretForSession(sessionId)
    const result = await invoke<MutationResult>('create_sftp_dir', {
      request: {
        session_id: sessionId,
        remote_path: sftpCreatePath.value.trim(),
        secret
      }
    })
    statusLine.value = `已创建目录 ${result.path}。`
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
    statusLine.value = '请先选择一个已保存的会话。'
    return
  }
  if (!remoteTransferPath.value.trim() || !sftpRenameTarget.value.trim()) {
    statusLine.value = '重命名前请先填写源路径和目标路径。'
    return
  }

  sftpBusy.value = true
  try {
    const sessionId = selectedSessionId.value
    const secret = await resolveSecretForSession(sessionId)
    const result = await invoke<MutationResult>('rename_sftp_path', {
      request: {
        session_id: sessionId,
        source_path: remoteTransferPath.value.trim(),
        target_path: sftpRenameTarget.value.trim(),
        secret
      }
    })
    remoteTransferPath.value = result.path
    sftpRenameTarget.value = result.path
    statusLine.value = `已将远程路径重命名为 ${result.path}。`
    await loadSftpDirectory(sftpPath.value)
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    sftpBusy.value = false
  }
}

async function deleteRemotePath() {
  if (!selectedSessionId.value) {
    statusLine.value = '请先选择一个已保存的会话。'
    return
  }
  if (!remoteTransferPath.value.trim()) {
    statusLine.value = '删除前请先选择远程路径。'
    return
  }

  const accepted = await dialogConfirm(
    `确认删除${remoteTransferIsDir.value ? '目录' : '文件'}“${remoteTransferPath.value.trim()}”吗？`,
    { title: '删除远程路径', kind: 'warning' }
  )
  if (!accepted) {
    statusLine.value = '已取消删除远程路径。'
    return
  }

  sftpBusy.value = true
  try {
    const sessionId = selectedSessionId.value
    const secret = await resolveSecretForSession(sessionId)
    const result = await invoke<MutationResult>('delete_sftp_path', {
      request: {
        session_id: sessionId,
        remote_path: remoteTransferPath.value.trim(),
        is_dir: remoteTransferIsDir.value,
        secret
      }
    })
    statusLine.value = `已删除 ${result.path}。`
    if (remoteEditorPath.value === result.path) {
      remoteEditorPath.value = ''
      remoteEditorContent.value = ''
      remoteEditorOriginalContent.value = ''
    }
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
    statusLine.value = '请先选择一个已保存的会话。'
    return
  }

  const selectedEntries = sftpEntries.value.filter(
    (entry) => selectedRemotePaths.value.includes(entry.path) && !entry.is_dir
  )
  if (!selectedEntries.length) {
    statusLine.value = '请至少选择一个文件加入下载队列。'
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
      message: '等待中',
      attemptCount: 0,
      maxRetries: defaultMaxRetries.value,
      createdAt: nowEpoch(),
      updatedAt: nowEpoch()
    }
    transferQueue.value.push(job)
    await persistTransferJob(job)
    await appendTransferEvent(job, 'info', `已加入批量下载队列：${job.remotePath}`)
  }

  statusLine.value = `已加入 ${selectedEntries.length} 个下载任务。`
}

async function batchQueueUploads() {
  if (!selectedSessionId.value) {
    statusLine.value = '请先选择一个已保存的会话。'
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
      message: '等待中',
      attemptCount: 0,
      maxRetries: defaultMaxRetries.value,
      createdAt: nowEpoch(),
      updatedAt: nowEpoch()
    }
    transferQueue.value.push(job)
    await persistTransferJob(job)
    await appendTransferEvent(job, 'info', `已加入批量上传队列：${job.localPath}`)
  }

  statusLine.value = `已加入 ${files.length} 个上传任务到 ${targetDirectory}。`
}

async function batchDeleteSelectedRemote() {
  if (!selectedSessionId.value) {
    statusLine.value = '请先选择一个已保存的会话。'
    return
  }

  const selectedEntries = sftpEntries.value.filter((entry) => selectedRemotePaths.value.includes(entry.path))
  if (!selectedEntries.length) {
    statusLine.value = '请至少选择一个远程路径再删除。'
    return
  }

  const accepted = await dialogConfirm(
    `确认删除 ${sftpPath.value} 下选中的 ${selectedEntries.length} 个远程路径吗？`,
    { title: '批量删除远程路径', kind: 'warning' }
  )
  if (!accepted) {
    statusLine.value = '已取消批量删除。'
    return
  }

  sftpBusy.value = true
  try {
    const sessionId = selectedSessionId.value
    const secret = await resolveSecretForSession(sessionId)
    for (const entry of selectedEntries) {
      await invoke('delete_sftp_path', {
        request: {
          session_id: sessionId,
          remote_path: entry.path,
          is_dir: entry.is_dir,
          secret
        }
      })
    }
    selectedRemotePaths.value = []
    statusLine.value = `已删除 ${selectedEntries.length} 个远程路径。`
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
      return '待上传'
    case 'Synced':
      return '已同步'
    case 'Conflict':
      return '冲突'
    default:
      return '仅本地'
  }
}

function renderError(error: unknown): string {
  if (typeof error === 'string') {
    return error
  }
  if (error && typeof error === 'object' && 'message' in error) {
    return String((error as { message: unknown }).message)
  }
  return '操作失败。'
}

function terminalStatusLabel(value: SessionStatus): string {
  switch (value) {
    case 'Idle':
      return '空闲'
    case 'Connecting':
      return '连接中'
    case 'Connected':
      return '已连接'
    case 'Disconnected':
      return '已断开'
    case 'Failed':
      return '失败'
  }
}

function transferStatusLabel(value: TransferJobStatus): string {
  switch (value) {
    case 'queued':
      return '排队中'
    case 'running':
      return '执行中'
    case 'success':
      return '成功'
    case 'error':
      return '失败'
  }
}

function transferKindLabel(value: TransferJobKind): string {
  return value === 'upload' ? '上传' : '下载'
}

function transferLevelLabel(value: TransferEventLevel): string {
  switch (value) {
    case 'warning':
      return '警告'
    case 'error':
      return '错误'
    default:
      return '信息'
  }
}

function renderTransferProgress(transferred: number, total: number | null): string {
  if (!total || total <= 0) {
    return `${transferred} 字节`
  }
  const percent = Math.min(100, Math.round((transferred / total) * 100))
  return `${percent}% · ${transferred}/${total} 字节`
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
    await notifyUser('传输失败', errorMessage)
  await updateTrayQueueStatus()
}

async function saveCurrentSecret() {
  if (!selectedSessionId.value) {
    statusLine.value = '请先选择一个已保存的会话。'
    return
  }
  if (!connectSecret.value.trim()) {
    statusLine.value = '凭据不能为空。'
    return
  }
  await invoke('save_session_secret', {
    sessionId: selectedSessionId.value,
    secret: connectSecret.value.trim()
  })
  rememberSecret.value = true
  statusLine.value = '凭据已保存到系统钥匙串。'
}

async function forgetSavedSecret() {
  if (!selectedSessionId.value) {
    statusLine.value = '请先选择一个已保存的会话。'
    return
  }
  await invoke('delete_session_secret', { sessionId: selectedSessionId.value })
  connectSecret.value = ''
  rememberSecret.value = false
  statusLine.value = '已从系统钥匙串移除保存的凭据。'
}

async function forgetSelectedSessionKnownHost() {
  if (!selectedSessionId.value) {
    statusLine.value = '请先选择一个已保存的会话。'
    return
  }

  const accepted = await dialogConfirm(
    `确认忘记 “${selectedSession.value?.host ?? selectedSessionId.value}” 的 known_hosts 记录吗？`,
    { title: '忘记已知主机', kind: 'warning' }
  )
  if (!accepted) {
    statusLine.value = '已取消移除 known_hosts 记录。'
    return
  }

  try {
    const removed = await invoke<number>('forget_session_known_host', {
      sessionId: selectedSessionId.value
    })
    await loadKnownHosts()
    statusLine.value = removed > 0 ? `已移除 ${removed} 条 known_hosts 记录。` : '当前会话没有匹配的 known_hosts 记录。'
  } catch (error) {
    statusLine.value = renderError(error)
  }
}

async function removeKnownHostLine(entry: KnownHostEntry) {
  const accepted = await dialogConfirm(
    `确认删除 known_hosts 第 ${entry.line} 行（${entry.hosts}）吗？`,
    { title: '删除已知主机记录', kind: 'warning' }
  )
  if (!accepted) {
    return
  }

  try {
    await invoke('remove_known_host_entry', { line: entry.line })
    await loadKnownHosts()
    statusLine.value = `已删除 known_hosts 第 ${entry.line} 行。`
  } catch (error) {
    statusLine.value = renderError(error)
  }
}

async function resolveSecretForSession(sessionId: string): Promise<string | null> {
  if (selectedSessionId.value === sessionId && connectSecret.value.trim()) {
    return connectSecret.value.trim()
  }

  const secret = await invoke<string | null>('load_session_secret', { sessionId })
  if (selectedSessionId.value === sessionId) {
    connectSecret.value = secret || ''
    rememberSecret.value = Boolean(secret)
  }
  return secret
}

async function openRemoteTextFile(path = remoteTransferPath.value.trim()) {
  if (!selectedSessionId.value) {
    statusLine.value = '请先选择一个已保存的会话。'
    return
  }
  if (!path) {
    statusLine.value = '请先选择一个远程文件。'
    return
  }

  remoteEditorLoading.value = true
  try {
    const sessionId = selectedSessionId.value
    const secret = await resolveSecretForSession(sessionId)
    const file = await invoke<RemoteTextFilePayload>('read_remote_text', {
      request: {
        session_id: sessionId,
        remote_path: path,
        secret
      }
    })
    remoteEditorPath.value = file.path
    remoteEditorContent.value = file.content
    remoteEditorOriginalContent.value = file.content
    remoteTransferPath.value = file.path
    activeDockTab.value = 'editor'
    statusLine.value = `已从 ${file.path} 读取 ${file.bytes} 字节。`
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    remoteEditorLoading.value = false
  }
}

async function saveRemoteTextFile() {
  if (!selectedSessionId.value || !remoteEditorPath.value) {
    statusLine.value = '请先打开一个远程文本文件。'
    return
  }

  remoteEditorLoading.value = true
  try {
    const sessionId = selectedSessionId.value
    const secret = await resolveSecretForSession(sessionId)
    await invoke('write_remote_text', {
      request: {
        session_id: sessionId,
        remote_path: remoteEditorPath.value,
        content: remoteEditorContent.value,
        secret
      }
    })
    remoteEditorOriginalContent.value = remoteEditorContent.value
    statusLine.value = `已保存 ${remoteEditorPath.value}。`
    await loadSftpDirectory(sftpPath.value)
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    remoteEditorLoading.value = false
  }
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

function openDockTab(tab: DockTab) {
  activeDockTab.value = tab
  if (tab === 'editor' && !remoteEditorPath.value && remoteTransferPath.value.trim()) {
    void openRemoteTextFile()
  }
}

watch(
  [
    connectSecret,
    rememberSecret,
    sftpPath,
    sftpEntries,
    remoteTreeRoots,
    remoteTransferPath,
    localTransferPath,
    remoteTransferIsDir,
    selectedRemotePaths,
    sftpCreatePath,
    sftpRenameTarget,
    remoteEditorPath,
    remoteEditorContent,
    remoteEditorOriginalContent,
    activeDockTab
  ],
  () => {
    persistSessionWorkspace()
  },
  { deep: true }
)

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
      ? `RustDock：${running} 个执行中，${queued} 个排队中，${failed} 个失败`
      : 'RustDock：空闲'

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
  statusLine.value = '已恢复上次会话遗留的队列任务。'
  await runTransferQueue()
}

function formatFileSize(bytes: number | null | undefined): string {
  if (bytes == null) return '--'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
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

function formatTimestamp(timestamp: number | null): string {
  if (!timestamp) {
    return '从未'
  }
  return new Date(timestamp * 1000).toLocaleString()
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
    import('@xterm/xterm'),
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
    statusLine.value = 'WebGL 终端渲染不可用，已回退到 canvas。'
  }

  terminal.open(terminalHost.value)
  fitAddon.fit()
  syncTerminalViewport()

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
  loadTransferBehaviorSettings()
  await invoke('set_background_on_close', { enabled: backgroundOnClose.value })
  await installTerminal()
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
  for (const tab of terminalTabs.value) {
    invoke('disconnect_terminal', { sessionId: tab.sessionId }).catch(() => undefined)
  }
  terminal?.dispose()
})
</script>

<template>
  <div class="app-shell">
    <aside class="left-rail">
      <div class="brand-block">
        <p class="eyebrow">中文工作台</p>
        <h1>RustDock</h1>
        <p class="subcopy">
          左侧管理会话书签，中间是终端主工作区，右侧是可停靠的 SSH 文件浏览与编辑面板。
        </p>
      </div>

      <div class="rail-actions">
        <button class="primary" @click="startNewSession">新建会话</button>
        <button class="ghost" :disabled="loading" @click="loadSessions">刷新列表</button>
      </div>

      <label class="search-card">
        <span>会话搜索</span>
        <input v-model="sessionFilter" placeholder="主机、标签、用户名" />
      </label>

      <div class="session-directory">
        <div class="section-title">
          <span>已保存会话</span>
          <span>{{ filteredSessions.length }}</span>
        </div>

        <button
          v-for="session in filteredSessions"
          :key="session.id"
          class="session-card"
          :class="{
            selected: session.id === selectedSessionId,
            live: session.id === activeTerminalId
          }"
          @click="selectSession(session)"
          @dblclick="connectSessionFromList(session)"
        >
          <div class="session-card-head">
            <strong>{{ session.name }}</strong>
            <span class="badge">{{ session.id === activeTerminalId ? '在线' : syncLabel(session.sync_state) }}</span>
          </div>
          <span>{{ session.username }}@{{ session.host }}:{{ session.port }}</span>
          <small>
            {{
              session.tags.length
                ? session.tags.join(' · ')
                : `上次连接 ${formatTimestamp(session.last_connected_at)}`
            }}
          </small>
        </button>

        <p v-if="!filteredSessions.length" class="empty-copy">
          还没有已保存会话。先在下方填写草稿并保存，然后双击即可连接。
        </p>
      </div>

      <section class="editor-card">
        <div class="panel-head panel-head--tight">
          <div>
            <p class="eyebrow">会话草稿</p>
            <h2>{{ form.id ? '编辑配置' : '快速连接草稿' }}</h2>
          </div>
          <div class="actions">
            <button class="ghost danger" :disabled="busy || !selectedSessionId" @click="deleteSession">
              删除
            </button>
            <button class="primary" :disabled="busy" @click="saveSession">保存</button>
          </div>
        </div>

        <div class="editor-grid">
          <label>
            <span>名称</span>
            <input v-model="form.name" placeholder="生产堡垒机" />
          </label>
          <label>
            <span>主机</span>
            <input v-model="form.host" placeholder="bastion.example.com" />
          </label>
          <label>
            <span>端口</span>
            <input v-model="form.port" inputmode="numeric" />
          </label>
          <label>
            <span>用户名</span>
            <input v-model="form.username" placeholder="root" />
          </label>
          <label>
            <span>认证方式</span>
            <select v-model="form.authType">
              <option value="private-key">私钥</option>
              <option value="agent">SSH 代理</option>
              <option value="password">密码</option>
            </select>
          </label>
          <label v-if="form.authType === 'private-key'">
            <span>密钥路径</span>
            <input v-model="form.keyPath" placeholder="~/.ssh/id_ed25519" />
          </label>
          <label v-else-if="form.authType === 'agent'">
            <span>说明</span>
            <input :value="'通过 SSH 代理认证'" disabled />
          </label>
          <label v-else>
            <span>连接密码</span>
            <input
              v-model="connectSecret"
              type="password"
              autocomplete="off"
              placeholder="输入 SSH 密码"
            />
          </label>
        </div>

        <div v-if="form.authType === 'password'" class="draft-secret-panel">
          <label class="checkbox-row">
            <input v-model="rememberSecret" type="checkbox" />
            <span>连接成功前，将密码保存到系统钥匙串</span>
          </label>
          <p class="empty-copy">
            现在可以直接在左侧输入密码，然后点击下方“{{ editorConnectLabel }}”。不需要先去中间区域输入。
          </p>
        </div>

        <div class="stack compact-stack">
          <label>
            <span>标签</span>
            <input v-model="form.tagsInput" placeholder="prod, ssh, eu-west" />
          </label>
          <label>
            <span>远程书签</span>
            <textarea v-model="form.remoteRootsInput" rows="4" placeholder="/home/root&#10;/var/www" />
          </label>
          <label>
            <span>本地书签</span>
            <textarea v-model="form.localRootsInput" rows="3" placeholder="/srv/app&#10;/var/log" />
          </label>
          <label>
            <span>备注</span>
            <textarea v-model="form.notes" rows="3" placeholder="跳板机、仅生产环境、密钥按月轮换" />
          </label>
        </div>

        <div class="actions actions--spread">
          <button class="ghost" :disabled="busy" @click="saveSession">
            仅保存
          </button>
          <button class="primary" :disabled="busy" @click="saveAndConnectSession">
            {{ editorConnectLabel }}
          </button>
        </div>
      </section>
    </aside>

    <main class="workspace-shell">
      <header class="topbar">
        <div class="workspace-tabs">
          <button class="workspace-tab active">工作台</button>
          <button
            v-for="tab in orderedTerminalTabs"
            :key="tab.sessionId"
            class="workspace-tab"
            :class="{
              connected: tab.sessionId === activeTerminalId,
              disconnected: tab.status === 'Disconnected' || tab.status === 'Failed'
            }"
            @click="activateTerminalTab(tab.sessionId)"
          >
            <span>{{ tab.sessionName }}</span>
            <small v-if="tab.unread">{{ tab.unread }}</small>
            <span class="workspace-tab-status">{{ terminalStatusLabel(tab.status) }}</span>
            <span class="workspace-tab-close" @click.stop="closeTerminalTab(tab.sessionId)">×</span>
          </button>
          <span class="workspace-caption">{{ selectedSessionSummary }}</span>
        </div>

        <div class="actions toolbar-actions">
          <button class="ghost" :disabled="busy || !selectedSessionId" @click="connectTerminal">连接</button>
          <button class="ghost danger" :disabled="!activeTerminalId" @click="disconnectTerminal">断开</button>
          <button class="ghost" :disabled="!selectedSessionId" @click="openDockTab('browser')">文件</button>
          <button class="ghost" :disabled="!selectedSessionId" @click="openDockTab('editor')">编辑器</button>
          <button class="ghost" @click="openDockTab('queue')">队列</button>
          <button class="ghost" @click="openDockTab('activity')">活动</button>
          <button class="ghost" @click="openDockTab('hosts')">主机</button>
        </div>
      </header>

      <section class="workspace-grid">
        <section class="panel terminal-panel shell-panel">
          <div class="panel-head">
            <div>
              <p class="eyebrow">终端工作区</p>
              <h2>{{ activeTerminalName || selectedSession?.name || '等待连接' }}</h2>
              <p class="subcopy">
                {{ activeTerminalId ? `当前已连接到 ${activeTerminalName}` : '选择一个书签并连接后，就会在这里打开实时终端。' }}
              </p>
            </div>

            <div class="terminal-meta">
              <span class="status-pill" :data-state="terminalStatus.toLowerCase()">{{ terminalStatusLabel(terminalStatus) }}</span>
              <span class="badge">{{ selectedSessionAuthLabel }}</span>
              <span v-if="selectedSession" class="badge">{{ selectedSessionTagSummary }}</span>
            </div>
          </div>

          <div class="terminal-auth terminal-auth--toolbar">
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
                <span>保存到系统钥匙串</span>
              </label>
              <button class="ghost" :disabled="!selectedSessionId || !connectSecret.trim()" @click="saveCurrentSecret">
                保存凭据
              </button>
              <button class="ghost danger" :disabled="!selectedSessionId" @click="forgetSavedSecret">
                忘记凭据
              </button>
            </div>
          </div>

          <div class="terminal-stack">
            <div ref="terminalHost" class="terminal-host" :class="{ 'terminal-host--idle': !activeTerminalId }"></div>

            <div v-if="!activeTerminalId" class="terminal-overlay">
              <div class="terminal-empty">
                <p class="eyebrow">连接中心</p>
                <h3>{{ selectedSession?.name || '尚未选择会话' }}</h3>
                <p>
                  当前界面按终端优先的工作流组织：左侧选会话，点击连接，中间跑终端，右侧处理文件浏览和编辑。
                </p>

                <div class="actions">
                  <button class="primary" :disabled="busy || !selectedSessionId" @click="connectTerminal">
                    连接当前会话
                  </button>
                  <button class="ghost" :disabled="busy" @click="saveAndConnectSession">保存并连接</button>
                  <button class="ghost" @click="openDockTab('browser')">打开 SSH 浏览器</button>
                </div>

                <div class="summary-grid">
                  <div class="summary-card">
                    <strong>{{ sessions.length }}</strong>
                    <span>已保存会话</span>
                  </div>
                  <div class="summary-card">
                    <strong>{{ queuedTransferCount }}</strong>
                    <span>排队任务</span>
                  </div>
                  <div class="summary-card">
                    <strong>{{ knownHosts.length }}</strong>
                    <span>known_hosts 记录</span>
                  </div>
                  <div class="summary-card">
                    <strong>{{ selectedSessionRemoteRoot }}</strong>
                    <span>主远程目录</span>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <div class="terminal-footer terminal-footer--cards">
            <div class="terminal-foot-card">
              <strong>连接目标</strong>
              <p>{{ selectedSessionSummary }}</p>
            </div>
            <div class="terminal-foot-card">
              <strong>远程根目录</strong>
              <p>{{ selectedSessionRemoteRoot }}</p>
            </div>
            <div class="terminal-foot-card">
              <strong>队列状态</strong>
              <p>{{ runningTransferCount }} 个执行中 · {{ queuedTransferCount }} 个排队中 · {{ failedTransferCount }} 个失败</p>
            </div>
            <div class="terminal-foot-card">
              <strong>终端标签</strong>
              <p>{{ terminalTabs.length }} 个已打开 · {{ orderedTerminalTabs.filter((tab) => tab.status === 'Connected').length }} 个已连接</p>
            </div>
          </div>
        </section>

        <aside class="panel dock-panel">
          <div class="dock-tabs">
            <button
              v-for="tab in dockTabs"
              :key="tab.id"
              class="dock-tab"
              :class="{ active: activeDockTab === tab.id }"
              @click="openDockTab(tab.id)"
            >
              {{ tab.label }}
            </button>
          </div>

          <div v-if="activeDockTab === 'browser'" class="dock-pane">
            <div class="panel-head panel-head--tight">
              <div>
                <p class="eyebrow">SSH 浏览器</p>
                <h2>远程文件</h2>
              </div>
              <div class="actions">
                <button class="ghost" :disabled="!selectedRemotePaths.length" @click="clearRemoteSelection">
                  清空选择
                </button>
                <button class="ghost" :disabled="sftpBusy" @click="goToParentDirectory">上一级</button>
                <button class="ghost" :disabled="sftpBusy || !selectedSessionId" @click="loadSftpDirectory()">
                  刷新
                </button>
              </div>
            </div>

            <div class="sftp-toolbar">
              <label>
                <span>远程目录</span>
                <input v-model="sftpPath" placeholder="/" />
              </label>
              <button class="primary" :disabled="sftpBusy || !selectedSessionId" @click="loadSftpDirectory()">
                加载
              </button>
            </div>

            <div class="browser-grid">
              <div class="tree-panel">
                <div class="section-title">
                  <span>目录树</span>
                  <span>{{ visibleRemoteTreeNodes.length }}</span>
                </div>
                <button
                  v-for="node in visibleRemoteTreeNodes"
                  :key="node.path"
                  class="tree-node"
                  :style="{ paddingLeft: `${12 + node.depth * 16}px` }"
                  @click="openRemoteTreeNode(node)"
                >
                  <span class="tree-toggle" @click.stop="toggleRemoteTreeNode(node)">
                    {{ node.loading ? '…' : node.expanded ? '▾' : '▸' }}
                  </span>
                  <span class="tree-label">{{ node.name }}</span>
                </button>
                <p v-if="!visibleRemoteTreeNodes.length" class="empty-copy">
                  先保存至少一个远程书签，或连接后从 `/` 开始浏览。
                </p>
              </div>

              <div class="sftp-list">
                <div class="section-title">
                  <span>当前目录</span>
                  <span>{{ sftpEntries.length }}</span>
                </div>
                <button
                  v-for="entry in sftpEntries"
                  :key="entry.path"
                  class="sftp-entry"
                  @click="selectRemotePathForMutation(entry)"
                  @dblclick="openRemoteEntry(entry)"
                  @contextmenu="openRemoteContextMenu($event, { path: entry.path, name: entry.name, isDir: entry.is_dir })"
                >
                  <div class="sftp-entry-head">
                    <label class="checkbox-row">
                      <input
                        :checked="isRemoteSelected(entry.path)"
                        type="checkbox"
                        @click.stop
                        @change="toggleRemoteSelection(entry)"
                      />
                      <span>{{ entry.is_dir ? '目录' : '文件' }}</span>
                    </label>
                    <small>{{ formatFileSize(entry.size) }}</small>
                  </div>
                  <strong>{{ entry.name }}</strong>
                  <small>{{ entry.path }}</small>
                </button>
                <p v-if="!sftpEntries.length" class="empty-copy">
                  先连接到会话，右侧 SFTP 面板才会有内容。
                </p>
              </div>
            </div>

            <div class="dock-form-grid">
              <label>
                <span>新目录路径</span>
                <input v-model="sftpCreatePath" placeholder="/tmp/new-folder" />
              </label>
              <label>
                <span>远程路径</span>
                <input v-model="remoteTransferPath" placeholder="/root/example.txt" />
              </label>
              <label>
                <span>重命名目标</span>
                <input v-model="sftpRenameTarget" placeholder="/root/example-renamed.txt" />
              </label>
              <label>
                <span>本地路径</span>
                <input v-model="localTransferPath" placeholder="/root/downloads/example.txt" />
              </label>
            </div>

            <div class="actions dock-actions">
              <button class="ghost" :disabled="sftpBusy" @click="createRemoteDirectory">创建目录</button>
              <button class="ghost" :disabled="sftpBusy || queueRunning" @click="chooseUploadLocalFile">选择文件</button>
              <button class="ghost" :disabled="sftpBusy || queueRunning" @click="chooseDownloadLocalPath">选择保存路径</button>
              <button class="ghost" :disabled="sftpBusy" @click="renameRemotePath">重命名</button>
              <button class="ghost danger" :disabled="sftpBusy" @click="deleteRemotePath">删除</button>
              <button class="ghost" :disabled="sftpBusy || queueRunning" @click="queueDownloadJob">加入下载队列</button>
              <button class="ghost" :disabled="sftpBusy || queueRunning" @click="queueUploadJob">加入上传队列</button>
              <button class="ghost" :disabled="queueRunning || sftpBusy || !selectedSessionId" @click="batchQueueUploads">批量上传</button>
              <button class="ghost" :disabled="!selectedRemotePaths.length || sftpBusy" @click="batchQueueDownloads">批量下载</button>
              <button class="ghost danger" :disabled="!selectedRemotePaths.length || sftpBusy" @click="batchDeleteSelectedRemote">批量删除</button>
              <button class="ghost" :disabled="sftpBusy" @click="downloadSelectedRemoteFile">下载</button>
              <button class="primary" :disabled="sftpBusy" @click="uploadLocalFile">上传</button>
            </div>
          </div>

          <div v-else-if="activeDockTab === 'editor'" class="dock-pane">
            <div class="panel-head panel-head--tight">
              <div>
                <p class="eyebrow">远程编辑器</p>
                <h2>{{ remoteEditorTitle }}</h2>
              </div>
              <div class="actions">
                <button class="ghost" :disabled="remoteEditorLoading || !remoteTransferPath" @click="openRemoteTextFile()">
                  载入
                </button>
                <button class="ghost" :disabled="remoteEditorLoading || !remoteEditorPath" @click="openRemoteTextFile(remoteEditorPath)">
                  重新载入
                </button>
                <button class="primary" :disabled="remoteEditorLoading || !remoteEditorDirty" @click="saveRemoteTextFile">
                  保存到远程
                </button>
              </div>
            </div>

            <div class="dock-form-grid">
              <label>
                <span>远程文本路径</span>
                <input v-model="remoteTransferPath" placeholder="/etc/nginx/nginx.conf" />
              </label>
              <label>
                <span>编辑状态</span>
                <input
                  :value="
                    remoteEditorLoading
                      ? '加载中…'
                      : remoteEditorPath
                        ? remoteEditorDirty
                          ? '已修改'
                          : '已同步'
                        : '尚未载入文件'
                  "
                  disabled
                />
              </label>
            </div>

            <label class="editor-surface">
              <span>UTF-8 文本缓冲区</span>
              <textarea
                v-model="remoteEditorContent"
                rows="20"
                :disabled="remoteEditorLoading || !remoteEditorPath"
                placeholder="在 SSH 浏览器里双击一个远程文件，就会加载到这里。"
              />
            </label>

            <p class="empty-copy">
              这个内联编辑器主要适合配置文件和脚本。非 UTF-8 文件或较大的二进制文件仍然建议通过下载/上传处理。
            </p>
          </div>

          <div v-else-if="activeDockTab === 'queue'" class="dock-pane">
            <div class="panel-head panel-head--tight">
              <div>
                <p class="eyebrow">传输队列</p>
                <h2>顺序任务</h2>
              </div>
              <div class="actions">
                <button class="ghost" :disabled="queueRunning || !transferQueue.length" @click="runTransferQueue">
                  {{ queueRunning ? '运行中...' : '运行队列' }}
                </button>
                <button class="ghost" :disabled="!queueRunning" @click="requestQueueStop">当前任务后停止</button>
                <button class="ghost" :disabled="!transferQueue.length" @click="clearCompletedTransfers">
                  清理已完成
                </button>
              </div>
            </div>

            <div class="settings-grid">
              <label class="checkbox-row">
                <input v-model="autoResumeQueue" type="checkbox" @change="persistAutoResumeQueueSetting" />
                <span>启动时自动恢复</span>
              </label>
              <label class="checkbox-row">
                <input v-model="backgroundOnClose" type="checkbox" @change="persistBackgroundOnCloseSetting" />
                <span>关闭时最小化到托盘</span>
              </label>
              <label class="checkbox-row">
                <input v-model="enableNotifications" type="checkbox" @change="persistNotificationSetting" />
                <span>系统通知</span>
              </label>
              <label class="checkbox-row">
                <input v-model="autoRetryTransfers" type="checkbox" @change="persistAutoRetrySettings" />
                <span>瞬时失败自动重试</span>
              </label>
              <label class="checkbox-row">
                <input v-model="autoRemoveSuccessfulJobs" type="checkbox" @change="persistTransferBehaviorSettings" />
                <span>自动移除成功任务</span>
              </label>
              <label>
                <span>重试次数</span>
                <input v-model="defaultMaxRetries" type="number" min="0" max="9" @change="persistAutoRetrySettings" />
              </label>
              <label>
                <span>基础延迟</span>
                <input v-model="retryBaseDelaySeconds" type="number" min="1" max="60" @change="persistTransferBehaviorSettings" />
              </label>
              <label>
                <span>最大延迟</span>
                <input v-model="retryMaxDelaySeconds" type="number" min="1" max="300" @change="persistTransferBehaviorSettings" />
              </label>
            </div>

            <div v-if="transferQueue.length" class="queue-list">
              <div v-for="job in transferQueue" :key="job.id" class="queue-item">
                <div class="queue-item-head">
                  <strong>{{ transferKindLabel(job.kind) }}</strong>
                  <span class="badge">{{ transferStatusLabel(job.status) }}</span>
                </div>
                <span>{{ job.remotePath }}</span>
                <small>{{ job.localPath }}</small>
                <small>第 {{ job.attemptCount }} / {{ job.maxRetries + 1 }} 次尝试</small>
                <small>{{ job.message }}</small>
                <div v-if="job.status === 'running'" class="queue-progress">
                  <div
                    class="queue-progress-bar"
                    :style="{ width: `${job.total ? Math.min(100, Math.round(((job.transferred ?? 0) / job.total) * 100)) : 15}%` }"
                  ></div>
                </div>
                <div class="actions">
                  <button class="ghost" :disabled="job.status === 'running'" @click="retryTransferJob(job.id)">
                    重试
                  </button>
                  <button class="ghost danger" :disabled="job.status !== 'running'" @click="cancelRunningTransfer(job.id)">
                    取消
                  </button>
                  <button class="ghost danger" :disabled="job.status === 'running'" @click="removeTransferJob(job.id)">
                    移除
                  </button>
                </div>
              </div>
            </div>
            <p v-else class="empty-copy">
              上传和下载任务会在这里按顺序执行，作为右侧停靠的传输面板。
            </p>
          </div>

          <div v-else-if="activeDockTab === 'activity'" class="dock-pane">
            <div class="panel-head panel-head--tight">
              <div>
                <p class="eyebrow">传输活动</p>
                <h2>最近事件</h2>
              </div>
              <div class="actions">
                <button class="ghost" @click="loadTransferEvents">刷新</button>
                <button class="ghost danger" :disabled="!transferEvents.length" @click="clearTransferEvents">
                  清空日志
                </button>
              </div>
            </div>

            <div class="settings-grid">
              <label>
                <span>搜索</span>
                <input v-model="transferEventQuery" placeholder="任务 ID、消息、会话 ID" />
              </label>
              <label>
                <span>级别</span>
                <select v-model="transferEventLevelFilter">
                  <option value="all">全部</option>
                  <option value="info">信息</option>
                  <option value="warning">警告</option>
                  <option value="error">错误</option>
                </select>
              </label>
            </div>

            <div class="known-hosts-list">
              <div v-for="event in visibleTransferEvents" :key="event.id" class="known-host-entry">
                <strong>{{ transferLevelLabel(event.level) }} · {{ event.job_id }}</strong>
                <span>{{ event.message }}</span>
                <small>{{ formatTimestamp(event.created_at) }}</small>
              </div>
              <p v-if="!visibleTransferEvents.length" class="empty-copy">
                还没有传输事件记录。
              </p>
            </div>
          </div>

          <div v-else class="dock-pane">
            <div class="panel-head panel-head--tight">
              <div>
                <p class="eyebrow">主机信任</p>
                <h2>known_hosts</h2>
              </div>
              <div class="actions">
                <button class="ghost" @click="loadKnownHosts">刷新</button>
                <button class="ghost danger" :disabled="!selectedSessionId" @click="forgetSelectedSessionKnownHost">
                  忘记当前主机
                </button>
              </div>
            </div>

            <div class="known-hosts-list">
              <div v-for="entry in knownHosts" :key="entry.line" class="known-host-entry">
                <strong>#{{ entry.line }} · {{ entry.key_type }}</strong>
                <span>{{ entry.hosts }}</span>
                <small>{{ entry.hashed ? '已哈希主机模式' : '明文主机模式' }}</small>
                <div class="actions">
                  <button class="ghost danger" @click="removeKnownHostLine(entry)">删除</button>
                </div>
              </div>
              <p v-if="!knownHosts.length" class="empty-copy">
                当前用户还没有 known_hosts 记录。
              </p>
            </div>
          </div>
        </aside>
      </section>

      <footer class="status-bar">
        <span>{{ statusLine }}</span>
        <span>{{ selectedSession ? `当前会话：${selectedSession.name}` : '尚未选择已保存会话' }}</span>
        <span>{{ runningTransferCount }} 个执行中 · {{ queuedTransferCount }} 个排队中 · {{ failedTransferCount }} 个失败</span>
      </footer>
    </main>

    <div
      v-if="remoteContextMenu"
      style="position:fixed;inset:0;z-index:999"
      @click="closeRemoteContextMenu"
      @contextmenu.prevent="closeRemoteContextMenu"
    >
      <div
        class="panel"
        :style="{
          position: 'fixed',
          left: remoteContextMenu.x + 'px',
          top: remoteContextMenu.y + 'px',
          zIndex: 1000,
          padding: '8px 0',
          minWidth: '180px',
          borderRadius: '14px'
        }"
      >
        <button
          v-if="remoteContextMenu.target.isDir"
          class="ghost"
          style="width:100%;text-align:left;border-radius:0;border:none;"
          @click="loadSftpDirectory(remoteContextMenu.target.path); closeRemoteContextMenu()"
        >打开目录</button>
        <button
          v-if="!remoteContextMenu.target.isDir"
          class="ghost"
          style="width:100%;text-align:left;border-radius:0;border:none;"
          @click="openRemoteTextFile(remoteContextMenu.target.path); closeRemoteContextMenu()"
        >在编辑器中打开</button>
        <button
          v-if="!remoteContextMenu.target.isDir"
          class="ghost"
          style="width:100%;text-align:left;border-radius:0;border:none;"
          @click="remoteTransferPath = remoteContextMenu.target.path; localTransferPath = remoteContextMenu.target.name; activeDockTab = 'browser'; closeRemoteContextMenu()"
        >选为下载源</button>
        <button
          class="ghost"
          style="width:100%;text-align:left;border-radius:0;border:none;"
          @click="sftpRenameTarget = remoteContextMenu.target.path; remoteTransferPath = remoteContextMenu.target.path; closeRemoteContextMenu()"
        >重命名</button>
        <button
          class="ghost danger"
          style="width:100%;text-align:left;border-radius:0;border:none;"
          @click="remoteTransferPath = remoteContextMenu.target.path; remoteTransferIsDir = remoteContextMenu.target.isDir; deleteRemotePath(); closeRemoteContextMenu()"
        >删除</button>
      </div>
    </div>
  </div>
</template>
