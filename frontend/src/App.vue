<script setup lang="ts">
import { Channel, invoke } from '@tauri-apps/api/core'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import type { FitAddon as XTermFitAddon } from '@xterm/addon-fit'
import type { Terminal as XTermTerminal } from '@xterm/xterm'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import type { ComponentPublicInstance } from 'vue'
import SessionSidebar from './components/SessionSidebar.vue'
import TerminalPanel from './components/TerminalPanel.vue'
import WorkspaceDock from './components/WorkspaceDock.vue'
import type {
  AuthMethod,
  DockTab,
  DockTabOption,
  DraftAuthType,
  KnownHostEntry,
  LocalPathInspection,
  MutationResult,
  RemoteContextTarget,
  RemoteDirectoryListing,
  RemoteDirEntry,
  RemoteTextFilePayload,
  RemoteTreeNode,
  SessionDraftPayload,
  SessionFormState,
  SessionProfile,
  SessionStatus,
  SessionSyncState,
  SessionWorkspaceState,
  TerminalConnection,
  TerminalStreamMessage,
  TerminalTab,
  TransferChannelMessage,
  TransferEventLevel,
  TransferEventRecordPayload,
  TransferJob,
  TransferJobKind,
  TransferJobRecordPayload,
  TransferJobStatus,
  TransferResult
} from './types'

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
const remoteDropZone = ref<HTMLElement | null>(null)
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
const remoteDropActive = ref(false)
const remoteDropCandidateCount = ref(0)
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
let remoteDropUnlisten: (() => void) | null = null
let sftpLoadRequestToken = 0
let remoteEditorRequestToken = 0
let transferJobSequence = 0

function bindTerminalHost(element: Element | ComponentPublicInstance | null) {
  terminalHost.value = element instanceof HTMLElement ? element : null
}

function bindRemoteDropZone(element: Element | ComponentPublicInstance | null) {
  remoteDropZone.value = element instanceof HTMLElement ? element : null
}

const sessionSidebarState = {
  sessionFilter,
  form,
  connectSecret,
  rememberSecret
}

const terminalCredentialState = {
  connectSecret,
  rememberSecret
}

const workspaceDockState = {
  sftpPath,
  remoteTransferPath,
  sftpCreatePath,
  sftpRenameTarget,
  localTransferPath,
  remoteEditorContent,
  autoResumeQueue,
  backgroundOnClose,
  enableNotifications,
  autoRetryTransfers,
  autoRemoveSuccessfulJobs,
  defaultMaxRetries,
  retryBaseDelaySeconds,
  retryMaxDelaySeconds,
  transferEventQuery,
  transferEventLevelFilter
}

const dockTabs: DockTabOption[] = [
  { id: 'browser', label: 'SSH 浏览器' },
  { id: 'editor', label: '远程编辑器' },
  { id: 'queue', label: '传输队列' },
  { id: 'activity', label: '活动日志' },
  { id: 'hosts', label: 'known_hosts' }
]

const selectedSession = computed(() =>
  sessions.value.find((session) => session.id === selectedSessionId.value) ?? null
)
const activeWorkspaceSession = computed(() =>
  sessions.value.find((session) => session.id === activeTerminalId.value) ?? null
)
const terminalContextSession = computed(() => activeWorkspaceSession.value ?? selectedSession.value)
const workspaceMismatch = computed(
  () =>
    Boolean(
      activeWorkspaceSession.value &&
        selectedSession.value &&
        activeWorkspaceSession.value.id !== selectedSession.value.id
    )
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
const remoteContextSelectionEntries = computed(() => {
  const target = remoteContextMenu.value?.target
  if (!target) {
    return []
  }

  const selectedPaths = selectedRemotePaths.value.includes(target.path)
    ? selectedRemotePaths.value
    : [target.path]
  return sftpEntries.value.filter((entry) => selectedPaths.includes(entry.path))
})
const remoteContextSelectionCount = computed(() => remoteContextSelectionEntries.value.length)
const remoteContextSelectionFileCount = computed(() =>
  remoteContextSelectionEntries.value.filter((entry) => !entry.is_dir).length
)
const remoteContextSelectionLabel = computed(() =>
  remoteContextSelectionCount.value > 1
    ? `已选 ${remoteContextSelectionCount.value} 项`
    : remoteContextSelectionEntries.value[0]?.name ?? remoteContextMenu.value?.target.name ?? '当前项'
)
const remoteContextSelectionMeta = computed(() => {
  if (remoteContextSelectionCount.value > 1) {
    const directoryCount =
      remoteContextSelectionCount.value - remoteContextSelectionFileCount.value
    return `${remoteContextSelectionFileCount.value} 文件 · ${directoryCount} 目录`
  }
  return remoteContextMenu.value?.target.path ?? ''
})
const remoteContextQueueDownloadLabel = computed(() =>
  remoteContextSelectionCount.value > 1
    ? `将 ${remoteContextSelectionCount.value} 项加入下载队列`
    : remoteContextMenu.value?.target.isDir
      ? '将目录加入下载队列'
      : '加入下载队列'
)
const remoteContextDeleteLabel = computed(() =>
  remoteContextSelectionCount.value > 1
    ? `删除已选 ${remoteContextSelectionCount.value} 项`
    : '删除'
)
const remoteDropSummary = computed(() =>
  remoteDropCandidateCount.value > 0
    ? `拖放 ${remoteDropCandidateCount.value} 项`
    : '拖放本地文件或目录'
)
const remoteDropHint = computed(() => `释放以上传到 ${sftpPath.value || '/'} 并加入队列`)

function sessionSummary(session: SessionProfile | null, fallback: string): string {
  if (!session) {
    return fallback
  }

  return `${session.username}@${session.host}:${session.port}`
}

function sessionPrimaryRemoteRoot(session: SessionProfile | null): string {
  return session?.remote_roots[0]?.trim() || '/'
}

function sessionAuthLabel(session: SessionProfile | null): string {
  if (!session) {
    return '未选择认证方式'
  }
  if (session.auth_method === 'Password') {
    return '密码认证'
  }
  if (session.auth_method === 'Agent') {
    return 'SSH 代理'
  }
  return `密钥 ${basename(session.auth_method.PrivateKey.path)}`
}

function sessionTagSummary(session: SessionProfile | null): string {
  if (!session?.tags.length) {
    return '无标签'
  }
  return session.tags.join(' · ')
}

const selectedSessionSummary = computed(() => {
  return sessionSummary(selectedSession.value, '先在左侧选择一个已保存会话，或先新建一个草稿。')
})
const selectedSessionRemoteRoot = computed(() => sessionPrimaryRemoteRoot(selectedSession.value))
const selectedSessionAuthLabel = computed(() => sessionAuthLabel(selectedSession.value))
const activeWorkspaceSummary = computed(() =>
  sessionSummary(activeWorkspaceSession.value, '右侧远端工作区未连接。')
)
const terminalContextRemoteRoot = computed(() => sessionPrimaryRemoteRoot(terminalContextSession.value))
const terminalContextAuthLabel = computed(() => sessionAuthLabel(terminalContextSession.value))
const terminalContextTagSummary = computed(() => sessionTagSummary(terminalContextSession.value))
const knownHostTargetSession = computed(() => activeWorkspaceSession.value ?? selectedSession.value)
const hasKnownHostTarget = computed(() => Boolean(knownHostTargetSession.value))
const knownHostTargetName = computed(() => knownHostTargetSession.value?.name ?? null)
const knownHostTargetScopeLabel = computed(() => {
  if (activeWorkspaceSession.value) {
    return '活动工作区'
  }
  if (selectedSession.value) {
    return '左侧草稿'
  }
  return '未选择目标'
})
const remoteEditorDirty = computed(
  () => remoteEditorPath.value.length > 0 && remoteEditorContent.value !== remoteEditorOriginalContent.value
)
const remoteEditorTitle = computed(() =>
  remoteEditorPath.value ? basename(remoteEditorPath.value) : '未打开文件'
)
const terminalContextTagSummaryText = computed(() => terminalContextTagSummary.value)
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
  persistSelectedSessionSecret()
  if (!activeTerminalId.value) {
    persistVisibleWorkspace()
  }
  selectedSessionId.value = null
  form.value = newDraft()
  connectSecret.value = ''
  rememberSecret.value = false
  if (!activeTerminalId.value) {
    clearVisibleWorkspace()
  }
  statusLine.value = activeTerminalId.value
    ? '已打开新的会话草稿，当前远端工作区保持不变。'
    : '已重置草稿。'
}

async function selectSession(session: SessionProfile) {
  if (selectedSessionId.value && selectedSessionId.value !== session.id) {
    persistSelectedSessionSecret(selectedSessionId.value)
    if (!activeTerminalId.value) {
      persistVisibleWorkspace(selectedSessionId.value)
    }
  }
  selectedSessionId.value = session.id
  form.value = draftFromSession(session)
  if (!activeTerminalId.value || activeTerminalId.value === session.id) {
    applyWorkspaceState(session)
  }
  await ensureSelectedSessionSecret(session)
  statusLine.value =
    activeTerminalId.value && activeTerminalId.value !== session.id
      ? `已切换左侧草稿到 ${session.name}，右侧仍绑定到 ${activeWorkspaceSession.value?.name ?? '当前工作区'}。`
      : `已选择 ${session.name}。`
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

function clearVisibleWorkspace() {
  invalidateVisibleWorkspaceOperations()
  sftpPath.value = '/'
  sftpEntries.value = []
  remoteTreeRoots.value = []
  remoteTransferPath.value = ''
  localTransferPath.value = ''
  remoteTransferIsDir.value = false
  selectedRemotePaths.value = []
  sftpCreatePath.value = ''
  sftpRenameTarget.value = ''
  clearRemoteEditorBuffer()
  activeDockTab.value = 'browser'
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

function currentVisibleWorkspaceId(): string | null {
  return activeTerminalId.value ?? selectedSessionId.value
}

function persistSelectedSessionSecret(sessionId = selectedSessionId.value) {
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
      sftpPath: existing?.sftpPath ?? session.remote_roots[0] ?? '/',
      sftpEntries: cloneRemoteEntries(existing?.sftpEntries ?? []),
      remoteTreeRoots: cloneRemoteTreeNodes(existing?.remoteTreeRoots ?? buildRemoteTreeRoots(session)),
      remoteTransferPath: existing?.remoteTransferPath ?? '',
      localTransferPath: existing?.localTransferPath ?? '',
      remoteTransferIsDir: existing?.remoteTransferIsDir ?? false,
      selectedRemotePaths: [...(existing?.selectedRemotePaths ?? [])],
      sftpCreatePath: existing?.sftpCreatePath ?? '',
      sftpRenameTarget: existing?.sftpRenameTarget ?? '',
      remoteEditorPath: existing?.remoteEditorPath ?? '',
      remoteEditorContent: existing?.remoteEditorContent ?? '',
      remoteEditorOriginalContent: existing?.remoteEditorOriginalContent ?? '',
      activeDockTab: existing?.activeDockTab ?? 'browser'
    }
  }
}

function persistVisibleWorkspace(sessionId = currentVisibleWorkspaceId()) {
  if (!sessionId) {
    return
  }

  const session = sessions.value.find((entry) => entry.id === sessionId)
  if (!session) {
    return
  }
  const existing = getWorkspaceState(session)

  sessionWorkspace.value = {
    ...sessionWorkspace.value,
    [sessionId]: {
      connectSecret: existing.connectSecret,
      rememberSecret: existing.rememberSecret,
      secretHydrated: existing.secretHydrated,
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
  invalidateVisibleWorkspaceOperations()
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
  activeDockTab.value = state.activeDockTab
}

function invalidateVisibleWorkspaceOperations() {
  sftpLoadRequestToken += 1
  remoteEditorRequestToken += 1
  sftpBusy.value = false
  remoteEditorLoading.value = false
}

function clearRemoteEditorBuffer() {
  remoteEditorPath.value = ''
  remoteEditorContent.value = ''
  remoteEditorOriginalContent.value = ''
}

function matchesRemotePathTarget(targetPath: string, candidatePath: string, candidateIsDir: boolean): boolean {
  if (targetPath === candidatePath) {
    return true
  }

  const normalizedCandidate = candidatePath.replace(/\/$/, '')
  return candidateIsDir && targetPath.startsWith(`${normalizedCandidate}/`)
}

function pruneDeletedRemoteState(entries: Array<{ path: string; isDir: boolean }>) {
  const isDeletedPath = (targetPath: string) =>
    entries.some((entry) => matchesRemotePathTarget(targetPath, entry.path, entry.isDir))

  if (remoteEditorPath.value && isDeletedPath(remoteEditorPath.value)) {
    clearRemoteEditorBuffer()
  }

  if (remoteTransferPath.value && isDeletedPath(remoteTransferPath.value)) {
    remoteTransferPath.value = ''
    remoteTransferIsDir.value = false
  }

  if (sftpRenameTarget.value && isDeletedPath(sftpRenameTarget.value)) {
    sftpRenameTarget.value = ''
  }

  selectedRemotePaths.value = selectedRemotePaths.value.filter((path) => !isDeletedPath(path))
}

function isActiveWorkspaceSession(sessionId: string): boolean {
  return activeTerminalId.value === sessionId
}

function canUpdateVisibleTerminalState(sessionId: string): boolean {
  return !activeTerminalId.value || activeTerminalId.value === sessionId
}

function canSendTerminalCommand(sessionId: string): boolean {
  const tab = findTerminalTab(sessionId)
  return tab?.status === 'Connected' || tab?.status === 'Connecting'
}

function handleTerminalCommandFailure(sessionId: string, error: unknown) {
  const message = renderError(error)
  if (
    message.includes('Terminal session is not connected') ||
    message.includes('session command channel is closed')
  ) {
    setTerminalTabStatus(sessionId, 'Disconnected')
    statusLine.value = '终端连接已结束，输入与窗口同步已停止。'
    return
  }

  statusLine.value = message
}

function hasTrackedTransferJob(jobId: string): boolean {
  return transferQueue.value.some((job) => job.id === jobId)
}

function nextTransferJobId(kind: TransferJobKind, hint = ''): string {
  transferJobSequence += 1
  const normalizedHint = hint
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 48)

  const suffix = normalizedHint ? `-${normalizedHint}` : ''
  return `${Date.now()}-${transferJobSequence}-${kind}${suffix}`
}

function purgeTransfersForSession(sessionId: string) {
  const removedJobIds = new Set(
    transferQueue.value.filter((job) => job.sessionId === sessionId).map((job) => job.id)
  )

  if (removedJobIds.size === 0) {
    transferEvents.value = transferEvents.value.filter((event) => event.session_id !== sessionId)
    return
  }

  transferQueue.value = transferQueue.value.filter((job) => job.sessionId !== sessionId)
  transferEvents.value = transferEvents.value.filter(
    (event) => event.session_id !== sessionId && !removedJobIds.has(event.job_id)
  )
}

async function refreshActiveWorkspaceDirectory(sessionId: string, path: string): Promise<boolean> {
  if (!isActiveWorkspaceSession(sessionId)) {
    return false
  }

  return loadSftpDirectory(path)
}

async function ensureSelectedSessionSecret(session: SessionProfile) {
  const state = getWorkspaceState(session)
  if (state.secretHydrated) {
    if (selectedSessionId.value === session.id) {
      connectSecret.value = state.connectSecret
      rememberSecret.value = state.rememberSecret
    }
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
    if (selectedSessionId.value === session.id) {
      connectSecret.value = state.connectSecret
      rememberSecret.value = state.rememberSecret
    }
  } catch (error) {
    if (selectedSessionId.value === session.id) {
      statusLine.value = renderError(error)
    }
  }
}

function requireActiveWorkspace(actionLabel: string): SessionProfile | null {
  if (!activeWorkspaceSession.value) {
    statusLine.value = `${actionLabel}前请先连接一个活动工作区。`
    return null
  }

  return activeWorkspaceSession.value
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
  const previousVisibleContextId = currentVisibleWorkspaceId()
  if (previousVisibleContextId && previousVisibleContextId !== sessionId) {
    persistVisibleWorkspace(previousVisibleContextId)
  }
  if (selectedSessionId.value && selectedSessionId.value !== sessionId) {
    persistSelectedSessionSecret(selectedSessionId.value)
  }
  activeTerminalId.value = tab.sessionId
  activeTerminalName.value = tab.sessionName
  terminalStatus.value = tab.status
  tab.unread = 0
  selectedSessionId.value = tab.sessionId
  const session = sessions.value.find((entry) => entry.id === tab.sessionId)
  if (session) {
    form.value = draftFromSession(session)
    applyWorkspaceState(session)
    await ensureSelectedSessionSecret(session)
  }
  syncTerminalViewport()
}

function ensureTerminalTab(sessionId: string, sessionName: string): TerminalTab {
  const existing = findTerminalTab(sessionId)
  if (existing) {
    existing.sessionName = sessionName
    return existing
  }

  const tab: TerminalTab = {
    sessionId,
    sessionName,
    status: 'Connecting',
    buffer: '',
    unread: 0,
    connectedAt: Date.now()
  }
  terminalTabs.value = [tab, ...terminalTabs.value]
  return tab
}

function registerTerminalTab(connection: TerminalConnection): TerminalTab {
  const tab = ensureTerminalTab(connection.session_id, connection.session_name)
  tab.connectedAt = Date.now()
  tab.unread = 0
  return tab
}

async function removeTerminalTab(
  sessionId: string,
  options: {
    activateFallback?: boolean
  } = {}
) {
  const activateFallback = options.activateFallback ?? true
  terminalTabs.value = terminalTabs.value.filter((tab) => tab.sessionId !== sessionId)
  if (activeTerminalId.value === sessionId) {
    const nextTab = activateFallback ? terminalTabs.value[0] ?? null : null
    if (nextTab) {
      await activateTerminalTab(nextTab.sessionId)
    } else {
      activeTerminalId.value = null
      activeTerminalName.value = ''
      terminalStatus.value = 'Disconnected'
      syncTerminalViewport()
      if (selectedSession.value) {
        applyWorkspaceState(selectedSession.value)
      } else {
        clearVisibleWorkspace()
      }
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
    persistSelectedSessionSecret(saved.id)
    if (!activeTerminalId.value || activeTerminalId.value === saved.id) {
      applyWorkspaceState(saved)
      persistVisibleWorkspace(saved.id)
    }
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
    purgeTransfersForSession(deletedId)
    await removeTerminalTab(deletedId)
    await loadSessions()
    await updateTrayQueueStatus()
    if (activeTerminalId.value) {
      const activeSession = sessions.value.find((session) => session.id === activeTerminalId.value)
      if (activeSession) {
        selectedSessionId.value = activeSession.id
        form.value = draftFromSession(activeSession)
        await ensureSelectedSessionSecret(activeSession)
      }
    } else {
      selectedSessionId.value = null
      form.value = newDraft()
      connectSecret.value = ''
      rememberSecret.value = false
      clearVisibleWorkspace()
    }
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

  const sessionId = selectedSessionId.value
  const fallbackSessionName = selectedSession.value?.name ?? sessionId
  const updatesVisibleTerminal = canUpdateVisibleTerminalState(sessionId)

  fitAddon.fit()
  const cols = terminal.cols || 120
  const rows = terminal.rows || 32

  const existingTab = findTerminalTab(sessionId)
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
    await removeTerminalTab(existingTab.sessionId, { activateFallback: false })
  }

  busy.value = true
  if (updatesVisibleTerminal) {
    terminalStatus.value = 'Connecting'
  }
  statusLine.value = updatesVisibleTerminal
    ? `正在连接 ${fallbackSessionName}...`
    : `正在连接 ${fallbackSessionName}，当前终端保持为 ${activeTerminalName.value || '现有工作区'}。`

  // Register the tab before invoke so early Channel messages (Connected / banner
  // output) are not dropped while the backend is still waiting on readiness.
  const pendingTab = ensureTerminalTab(sessionId, fallbackSessionName)
  if (pendingTab.status === 'Idle' || pendingTab.status === 'Disconnected' || pendingTab.status === 'Failed') {
    pendingTab.status = 'Connecting'
  }
  if (pendingTab.buffer.length === 0) {
    pendingTab.buffer = trimTerminalBuffer(
      `\x1b[1;34m[workspace]\x1b[0m Opening ${fallbackSessionName}\r\n`
    )
  }
  if (updatesVisibleTerminal) {
    terminalStatus.value = pendingTab.status
    syncTerminalViewport()
  }

  try {
    if (rememberSecret.value && connectSecret.value.trim()) {
      await invoke('save_session_secret', {
        sessionId,
        secret: connectSecret.value.trim()
      })
    }

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

    // Refresh name only — never force status back to Connecting (Channel may
    // already have delivered Connected before invoke resolved).
    const tab = ensureTerminalTab(connection.session_id, connection.session_name)
    tab.connectedAt = Date.now()
    tab.unread = 0
    await activateTerminalTab(connection.session_id)
    activeDockTab.value = 'browser'
    const sftpLoaded = await loadSftpDirectory(selectedSessionRemoteRoot.value)
    const sftpStatus = statusLine.value
    terminal.focus()
    await loadSessions()
    await loadKnownHosts()
    statusLine.value = sftpLoaded ? `终端已连接到 ${connection.session_name}。` : sftpStatus
  } catch (error) {
    const tab = findTerminalTab(sessionId)
    if (tab && (tab.status === 'Connecting' || tab.status === 'Failed' || tab.status === 'Idle')) {
      await removeTerminalTab(sessionId, { activateFallback: updatesVisibleTerminal })
    } else if (updatesVisibleTerminal) {
      terminalStatus.value = 'Failed'
    }
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

  const sessionId = activeTerminalId.value
  try {
    await invoke('disconnect_terminal', { sessionId })
    await removeTerminalTab(sessionId)
    statusLine.value = '终端已断开。'
  } catch (error) {
    const message = renderError(error)
    if (message.includes('Terminal session is not connected')) {
      await removeTerminalTab(sessionId)
      statusLine.value = '终端已结束，本地标签已清理。'
      return
    }
    statusLine.value = message
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

  await removeTerminalTab(sessionId)
}

async function closeTerminalState() {
  if (!activeTerminalId.value) {
    terminalStatus.value = 'Disconnected'
    return
  }
  await removeTerminalTab(activeTerminalId.value)
}

async function loadSftpDirectory(path?: string): Promise<boolean> {
  const workspace = requireActiveWorkspace('加载远程目录')
  if (!workspace) {
    return false
  }

  const requestToken = ++sftpLoadRequestToken
  sftpBusy.value = true
  try {
    const secret = await resolveSecretForSession(workspace.id)
    const listing = await invoke<RemoteDirectoryListing>('list_sftp_dir', {
      request: {
        session_id: workspace.id,
        path: path ?? sftpPath.value,
        secret
      }
    })
    if (requestToken !== sftpLoadRequestToken || !isActiveWorkspaceSession(workspace.id)) {
      return false
    }
    sftpPath.value = listing.directory
    sftpEntries.value = listing.entries
    selectedRemotePaths.value = selectedRemotePaths.value.filter((selectedPath) =>
      listing.entries.some((entry) => entry.path === selectedPath)
    )
    statusLine.value = `已加载 ${listing.directory} 下的 ${listing.entries.length} 个条目。`
    return true
  } catch (error) {
    if (requestToken === sftpLoadRequestToken) {
      statusLine.value = renderError(error)
    }
    return false
  } finally {
    if (requestToken === sftpLoadRequestToken) {
      sftpBusy.value = false
    }
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
  const workspace = requireActiveWorkspace('展开远程目录')
  if (!workspace || node.loading) {
    return
  }

  node.loading = true
  try {
    const secret = await resolveSecretForSession(workspace.id)
    const listing = await invoke<RemoteDirectoryListing>('list_sftp_dir', {
      request: {
        session_id: workspace.id,
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
  setRemoteMutationTarget(entry.path, entry.is_dir)
  if (!localTransferPath.value) {
    localTransferPath.value = entry.name
  }
  activeDockTab.value = 'editor'
  void openRemoteTextFile(entry.path)
}

function setRemoteMutationTarget(path: string, isDir: boolean) {
  remoteTransferPath.value = path
  remoteTransferIsDir.value = isDir
  sftpRenameTarget.value = path
}

function selectRemotePathForMutation(entry: RemoteDirEntry) {
  setRemoteMutationTarget(entry.path, entry.is_dir)
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

function selectAllRemoteEntries() {
  const nextSelection = sftpEntries.value.map((entry) => entry.path)
  selectedRemotePaths.value = nextSelection
  if (nextSelection.length) {
    statusLine.value = `已选中当前目录下 ${nextSelection.length} 项。`
  }
}

function clearRemoteDropState() {
  remoteDropActive.value = false
  remoteDropCandidateCount.value = 0
}

function canAcceptRemoteDrop(): boolean {
  return (
    activeDockTab.value === 'browser' &&
    Boolean(activeWorkspaceSession.value) &&
    Boolean(remoteDropZone.value) &&
    !queueRunning.value &&
    !sftpBusy.value
  )
}

function toCssDropPosition(position: { x: number; y: number }) {
  const scale = window.devicePixelRatio || 1
  return {
    x: position.x / scale,
    y: position.y / scale
  }
}

function isPositionInsideRemoteDropZone(position: { x: number; y: number }): boolean {
  if (!canAcceptRemoteDrop() || !remoteDropZone.value) {
    return false
  }

  const rect = remoteDropZone.value.getBoundingClientRect()
  const cssPosition = toCssDropPosition(position)
  return (
    cssPosition.x >= rect.left &&
    cssPosition.x <= rect.right &&
    cssPosition.y >= rect.top &&
    cssPosition.y <= rect.bottom
  )
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
  const workspace = requireActiveWorkspace('下载远程文件')
  if (!workspace) {
    return
  }
  if (remoteTransferIsDir.value) {
    statusLine.value = '目录下载请使用“加入下载队列”或“批量下载”。'
    return
  }
  if (!remoteTransferPath.value.trim() || !localTransferPath.value.trim()) {
    statusLine.value = '下载前请先填写远程路径和本地路径。'
    return
  }

  sftpBusy.value = true
  try {
    const secret = await resolveSecretForSession(workspace.id)
    const result = await invoke<TransferResult>('download_sftp_file', {
      request: {
        session_id: workspace.id,
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
  const workspace = requireActiveWorkspace('上传本地文件')
  if (!workspace) {
    return
  }
  if (!remoteTransferPath.value.trim() || !localTransferPath.value.trim()) {
    statusLine.value = '上传前请先填写本地路径和远程路径。'
    return
  }

  let localInfo: LocalPathInspection
  try {
    localInfo = await inspectLocalPath(localTransferPath.value.trim())
  } catch (error) {
    statusLine.value = renderError(error)
    return
  }
  if (localInfo.is_dir) {
    statusLine.value = '目录上传请使用“加入上传队列”。'
    return
  }

  sftpBusy.value = true
  try {
    const secret = await resolveSecretForSession(workspace.id)
    const result = await invoke<TransferResult>('upload_sftp_file', {
      request: {
        session_id: workspace.id,
        local_path: localTransferPath.value.trim(),
        remote_path: remoteTransferPath.value.trim(),
        secret
      }
    })
    statusLine.value = `已上传 ${result.bytes} 字节到 ${result.path}。`
    await refreshActiveWorkspaceDirectory(workspace.id, sftpPath.value)
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
  remoteTransferIsDir.value = false
  if (!remoteTransferPath.value.trim()) {
    remoteTransferPath.value = joinRemotePath(sftpPath.value, basename(selected))
  }
}

async function chooseUploadLocalDirectory() {
  const selected = await dialogOpen({
    multiple: false,
    directory: true
  })

  if (!selected || Array.isArray(selected)) {
    return
  }

  localTransferPath.value = selected
  remoteTransferPath.value = joinRemotePath(sftpPath.value, basename(selected))
  remoteTransferIsDir.value = true
}

async function chooseDownloadLocalPath() {
  if (remoteTransferIsDir.value && remoteTransferPath.value.trim()) {
    const selected = await dialogOpen({
      multiple: false,
      directory: true
    })

    if (!selected || Array.isArray(selected)) {
      return
    }

    localTransferPath.value = `${selected.replace(/[\\/]$/, '')}/${basename(remoteTransferPath.value.trim())}`
    return
  }

  const selected = await dialogSave({
    defaultPath: localTransferPath.value || basename(remoteTransferPath.value) || 'download.txt'
  })

  if (!selected) {
    return
  }

  localTransferPath.value = selected
}

async function queueDownloadJob() {
  const workspace = requireActiveWorkspace('加入下载队列')
  if (!workspace) {
    return
  }
  if (!remoteTransferPath.value.trim() || !localTransferPath.value.trim()) {
    statusLine.value = '加入下载队列前请先填写远程路径和本地路径。'
    return
  }

  const kind: TransferJobKind = remoteTransferIsDir.value ? 'download-dir' : 'download'
  const job: TransferJob = {
    id: nextTransferJobId(kind, basename(remoteTransferPath.value.trim())),
    kind,
    sessionId: workspace.id,
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
  const workspace = requireActiveWorkspace('加入上传队列')
  if (!workspace) {
    return
  }
  if (!remoteTransferPath.value.trim() || !localTransferPath.value.trim()) {
    statusLine.value = '加入上传队列前请先填写本地路径和远程路径。'
    return
  }

  let localInfo: LocalPathInspection
  try {
    localInfo = await inspectLocalPath(localTransferPath.value.trim())
  } catch (error) {
    statusLine.value = renderError(error)
    return
  }
  if (!localInfo.is_file && !localInfo.is_dir) {
    statusLine.value = '暂不支持该本地路径类型。'
    return
  }

  const kind: TransferJobKind = localInfo.is_dir ? 'upload-dir' : 'upload'
  const job: TransferJob = {
    id: nextTransferJobId(kind, basename(localTransferPath.value.trim())),
    kind,
    sessionId: workspace.id,
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

async function enqueueUploadPathsToCurrentDirectory(
  localPaths: string[],
  sourceLabel: string
): Promise<{ queuedCount: number; skippedCount: number }> {
  const workspace = requireActiveWorkspace(sourceLabel)
  if (!workspace) {
    return { queuedCount: 0, skippedCount: 0 }
  }
  if (queueRunning.value) {
    statusLine.value = `传输队列正在运行，当前不接受新的${sourceLabel}任务。`
    return { queuedCount: 0, skippedCount: 0 }
  }
  if (sftpBusy.value) {
    statusLine.value = '当前远端工作区正忙，稍后再试。'
    return { queuedCount: 0, skippedCount: 0 }
  }

  const normalizedPaths = [...new Set(localPaths.map((path) => path.trim()).filter(Boolean))]
  const targetDirectory = sftpPath.value || '/'
  let queuedCount = 0
  let skippedCount = 0
  let firstJob: TransferJob | null = null

  for (const localPath of normalizedPaths) {
    let localInfo: LocalPathInspection
    try {
      localInfo = await inspectLocalPath(localPath)
    } catch {
      skippedCount += 1
      continue
    }

    if (!localInfo.is_file && !localInfo.is_dir) {
      skippedCount += 1
      continue
    }

    const kind: TransferJobKind = localInfo.is_dir ? 'upload-dir' : 'upload'
    const job: TransferJob = {
      id: nextTransferJobId(kind, basename(localPath)),
      kind,
      sessionId: workspace.id,
      remotePath: joinRemotePath(targetDirectory, basename(localPath)),
      localPath,
      status: 'queued',
      message: '等待中',
      attemptCount: 0,
      maxRetries: defaultMaxRetries.value,
      createdAt: nowEpoch(),
      updatedAt: nowEpoch()
    }

    if (!firstJob) {
      firstJob = job
    }
    transferQueue.value.push(job)
    await persistTransferJob(job)
    await appendTransferEvent(job, 'info', `${sourceLabel}：已加入上传队列：${job.localPath}`)
    queuedCount += 1
  }

  if (firstJob) {
    localTransferPath.value = firstJob.localPath
    remoteTransferPath.value = firstJob.remotePath
    remoteTransferIsDir.value = firstJob.kind === 'upload-dir'
  }

  await updateTrayQueueStatus()
  return { queuedCount, skippedCount }
}

async function queueDraggedUploads(paths: string[]) {
  const { queuedCount, skippedCount } = await enqueueUploadPathsToCurrentDirectory(paths, '拖拽上传')
  if (queuedCount === 0) {
    if (skippedCount > 0) {
      statusLine.value = '拖拽的本地路径不可用，未加入上传队列。'
    }
    return
  }

  activeDockTab.value = 'queue'
  statusLine.value =
    skippedCount > 0
      ? `已将 ${queuedCount} 项拖放到 ${sftpPath.value || '/'}，加入上传队列，另有 ${skippedCount} 项已跳过。`
      : `已将 ${queuedCount} 项拖放到 ${sftpPath.value || '/'}，加入上传队列。`
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
    if (activeWorkspaceSession.value) {
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
  await updateTrayQueueStatus()
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
  let secret: string | null
  try {
    secret = await resolveSecretForSession(job.sessionId)
  } catch (error) {
    await finalizeTransferFailure(job, renderError(error))
    return
  }

  await new Promise<void>((resolve) => {
    const channel = new Channel<TransferChannelMessage>()
    channel.onmessage = (message) => {
      if (!hasTrackedTransferJob(job.id)) {
        resolve()
        return
      }
      if (message.job_id !== job.id) {
        return
      }

      if (message.kind === 'started') {
        job.message = '已连接'
      } else if (message.kind === 'progress') {
        job.transferred = message.transferred
        job.total = message.total
        job.message = renderTransferProgress(
          message.transferred,
          message.total,
          message.current_path,
          message.completed_files,
          message.total_files
        )
      } else if (message.kind === 'completed') {
        job.status = 'success'
        job.bytes = message.bytes
        job.transferred = message.bytes
        job.total = message.bytes
        job.updatedAt = nowEpoch()
        job.message = transferSuccessMessage(job.kind, message.bytes)
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
      if (!hasTrackedTransferJob(job.id)) {
        resolve()
        return
      }
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
  const workspace = requireActiveWorkspace('创建远程目录')
  if (!workspace) {
    return
  }
  if (!sftpCreatePath.value.trim()) {
    statusLine.value = '请输入要创建的远程目录路径。'
    return
  }

  sftpBusy.value = true
  try {
    const secret = await resolveSecretForSession(workspace.id)
    const result = await invoke<MutationResult>('create_sftp_dir', {
      request: {
        session_id: workspace.id,
        remote_path: sftpCreatePath.value.trim(),
        secret
      }
    })
    statusLine.value = `已创建目录 ${result.path}。`
    sftpCreatePath.value = ''
    await refreshActiveWorkspaceDirectory(workspace.id, sftpPath.value)
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    sftpBusy.value = false
  }
}

async function renameRemotePath() {
  const workspace = requireActiveWorkspace('重命名远程路径')
  if (!workspace) {
    return
  }
  if (!remoteTransferPath.value.trim() || !sftpRenameTarget.value.trim()) {
    statusLine.value = '重命名前请先填写源路径和目标路径。'
    return
  }

  sftpBusy.value = true
  try {
    const secret = await resolveSecretForSession(workspace.id)
    const result = await invoke<MutationResult>('rename_sftp_path', {
      request: {
        session_id: workspace.id,
        source_path: remoteTransferPath.value.trim(),
        target_path: sftpRenameTarget.value.trim(),
        secret
      }
    })
    remoteTransferPath.value = result.path
    sftpRenameTarget.value = result.path
    statusLine.value = `已将远程路径重命名为 ${result.path}。`
    await refreshActiveWorkspaceDirectory(workspace.id, sftpPath.value)
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    sftpBusy.value = false
  }
}

async function deleteRemotePath() {
  const workspace = requireActiveWorkspace('删除远程路径')
  if (!workspace) {
    return
  }
  if (!remoteTransferPath.value.trim()) {
    statusLine.value = '删除前请先选择远程路径。'
    return
  }

  const accepted = await dialogConfirm(
    remoteTransferIsDir.value
      ? `确认递归删除目录“${remoteTransferPath.value.trim()}”吗？`
      : `确认删除文件“${remoteTransferPath.value.trim()}”吗？`,
    { title: '删除远程路径', kind: 'warning' }
  )
  if (!accepted) {
    statusLine.value = '已取消删除远程路径。'
    return
  }

  sftpBusy.value = true
  try {
    const isDirectory = remoteTransferIsDir.value
    const secret = await resolveSecretForSession(workspace.id)
    const result = await invoke<MutationResult>('delete_sftp_path', {
      request: {
        session_id: workspace.id,
        remote_path: remoteTransferPath.value.trim(),
        is_dir: isDirectory,
        secret
      }
    })
    statusLine.value = remoteTransferIsDir.value
      ? `已递归删除目录 ${result.path}。`
      : `已删除文件 ${result.path}。`
    pruneDeletedRemoteState([{ path: result.path, isDir: isDirectory }])
    await refreshActiveWorkspaceDirectory(workspace.id, sftpPath.value)
  } catch (error) {
    statusLine.value = renderError(error)
  } finally {
    sftpBusy.value = false
  }
}

async function batchQueueDownloads() {
  const workspace = requireActiveWorkspace('批量加入下载队列')
  if (!workspace) {
    return
  }

  const selectedEntries = sftpEntries.value.filter((entry) => selectedRemotePaths.value.includes(entry.path))
  if (!selectedEntries.length) {
    statusLine.value = '请至少选择一个远程路径加入下载队列。'
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
    const kind: TransferJobKind = entry.is_dir ? 'download-dir' : 'download'
    const job: TransferJob = {
      id: nextTransferJobId(kind, entry.name),
      kind,
      sessionId: workspace.id,
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
  await updateTrayQueueStatus()
}

async function batchQueueUploads() {
  const selected = await dialogOpen({
    multiple: true,
    directory: false
  })
  if (!selected) {
    return
  }

  const files = Array.isArray(selected) ? selected : [selected]
  const { queuedCount, skippedCount } = await enqueueUploadPathsToCurrentDirectory(files, '批量上传')
  if (queuedCount === 0) {
    if (skippedCount > 0) {
      statusLine.value = '没有可加入批量上传队列的本地文件。'
    }
    return
  }

  statusLine.value =
    skippedCount > 0
      ? `已加入 ${queuedCount} 个上传任务到 ${sftpPath.value || '/'}，另有 ${skippedCount} 项已跳过。`
      : `已加入 ${queuedCount} 个上传任务到 ${sftpPath.value || '/'}。`
}

async function batchDeleteSelectedRemote() {
  const workspace = requireActiveWorkspace('批量删除远程路径')
  if (!workspace) {
    return
  }

  const selectedEntries = sftpEntries.value.filter((entry) => selectedRemotePaths.value.includes(entry.path))
  if (!selectedEntries.length) {
    statusLine.value = '请至少选择一个远程路径再删除。'
    return
  }
  const includesDirectory = selectedEntries.some((entry) => entry.is_dir)

  const accepted = await dialogConfirm(
    includesDirectory
      ? `确认删除 ${sftpPath.value} 下选中的 ${selectedEntries.length} 个远程路径吗？其中目录会递归删除。`
      : `确认删除 ${sftpPath.value} 下选中的 ${selectedEntries.length} 个远程路径吗？`,
    { title: '批量删除远程路径', kind: 'warning' }
  )
  if (!accepted) {
    statusLine.value = '已取消批量删除。'
    return
  }

  sftpBusy.value = true
  try {
    const secret = await resolveSecretForSession(workspace.id)
    for (const entry of selectedEntries) {
      await invoke('delete_sftp_path', {
        request: {
          session_id: workspace.id,
          remote_path: entry.path,
          is_dir: entry.is_dir,
          secret
        }
      })
    }
    pruneDeletedRemoteState(selectedEntries.map((entry) => ({ path: entry.path, isDir: entry.is_dir })))
    statusLine.value = includesDirectory
      ? `已删除 ${selectedEntries.length} 个远程路径，其中目录已递归清理。`
      : `已删除 ${selectedEntries.length} 个远程路径。`
    await refreshActiveWorkspaceDirectory(workspace.id, sftpPath.value)
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
  switch (value) {
    case 'upload-dir':
      return '上传目录'
    case 'download-dir':
      return '下载目录'
    case 'upload':
      return '上传'
    default:
      return '下载'
  }
}

function transferSuccessMessage(kind: TransferJobKind, bytes: number): string {
  switch (kind) {
    case 'upload-dir':
      return `目录上传完成，共 ${bytes} 字节`
    case 'download-dir':
      return `目录下载完成，共 ${bytes} 字节`
    case 'upload':
      return `已上传 ${bytes} 字节`
    default:
      return `已下载 ${bytes} 字节`
  }
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

function renderTransferProgress(
  transferred: number,
  total: number | null,
  currentPath: string | null,
  completedFiles: number | null,
  totalFiles: number | null
): string {
  const segments: string[] = []

  if (typeof totalFiles === 'number' && totalFiles > 0) {
    const completed = typeof completedFiles === 'number' ? completedFiles : 0
    segments.push(`${completed}/${totalFiles} 已完成`)
  }

  if (currentPath) {
    segments.push(basename(currentPath))
  }

  if (!total || total <= 0) {
    segments.push(`${transferred} 字节`)
    return segments.join(' · ')
  }

  const percent = Math.min(100, Math.round((transferred / total) * 100))
  segments.push(`${percent}% · ${transferred}/${total} 字节`)
  return segments.join(' · ')
}

function computeRetryDelaySeconds(attemptCount: number): number {
  const exponent = Math.max(0, attemptCount - 1)
  return Math.min(retryMaxDelaySeconds.value, retryBaseDelaySeconds.value * 2 ** exponent)
}

async function delay(ms: number) {
  await new Promise((resolve) => window.setTimeout(resolve, ms))
}

async function finalizeTransferFailure(job: TransferJob, errorMessage: string) {
  if (!hasTrackedTransferJob(job.id)) {
    return
  }

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
  try {
    await invoke('save_session_secret', {
      sessionId: selectedSessionId.value,
      secret: connectSecret.value.trim()
    })
    rememberSecret.value = true
    statusLine.value = selectedSession.value
      ? `已为 ${selectedSession.value.name} 保存凭据到系统钥匙串。`
      : '凭据已保存到系统钥匙串。'
  } catch (error) {
    statusLine.value = renderError(error)
  }
}

async function forgetSavedSecret() {
  if (!selectedSessionId.value) {
    statusLine.value = '请先选择一个已保存的会话。'
    return
  }
  try {
    await invoke('delete_session_secret', { sessionId: selectedSessionId.value })
    connectSecret.value = ''
    rememberSecret.value = false
    statusLine.value = selectedSession.value
      ? `已移除 ${selectedSession.value.name} 的已保存凭据。`
      : '已从系统钥匙串移除保存的凭据。'
  } catch (error) {
    statusLine.value = renderError(error)
  }
}

async function forgetKnownHostTarget() {
  const target = knownHostTargetSession.value
  if (!target) {
    statusLine.value = '请先选择一个会话，或连接一个活动工作区。'
    return
  }
  const scopeLabel = activeWorkspaceSession.value ? '活动工作区' : '左侧草稿'

  const accepted = await dialogConfirm(
    `确认忘记${scopeLabel} “${target.host}” 的 known_hosts 记录吗？`,
    { title: '忘记已知主机', kind: 'warning' }
  )
  if (!accepted) {
    statusLine.value = '已取消移除 known_hosts 记录。'
    return
  }

  try {
    const removed = await invoke<number>('forget_session_known_host', {
      sessionId: target.id
    })
    await loadKnownHosts()
    statusLine.value =
      removed > 0
        ? `已为${scopeLabel} ${target.name} 移除 ${removed} 条 known_hosts 记录。`
        : `${scopeLabel} ${target.name} 没有匹配的 known_hosts 记录。`
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

  const cached = sessionWorkspace.value[sessionId]?.connectSecret.trim()
  if (cached) {
    return cached
  }

  const secret = await invoke<string | null>('load_session_secret', { sessionId })
  const session = sessions.value.find((entry) => entry.id === sessionId)
  if (session) {
    const existing = getWorkspaceState(session)
    sessionWorkspace.value = {
      ...sessionWorkspace.value,
      [sessionId]: {
        ...existing,
        connectSecret: secret || existing.connectSecret,
        rememberSecret: Boolean(secret),
        secretHydrated: true
      }
    }
  }
  if (selectedSessionId.value === sessionId) {
    connectSecret.value = secret || ''
    rememberSecret.value = Boolean(secret)
  }
  return secret
}

async function openRemoteTextFile(path = remoteTransferPath.value.trim()) {
  const workspace = requireActiveWorkspace('打开远程文件')
  if (!workspace) {
    return
  }
  if (!path) {
    statusLine.value = '请先选择一个远程文件。'
    return
  }

  const requestToken = ++remoteEditorRequestToken
  remoteEditorLoading.value = true
  try {
    const secret = await resolveSecretForSession(workspace.id)
    const file = await invoke<RemoteTextFilePayload>('read_remote_text', {
      request: {
        session_id: workspace.id,
        remote_path: path,
        secret
      }
    })
    if (requestToken !== remoteEditorRequestToken || !isActiveWorkspaceSession(workspace.id)) {
      return
    }
    remoteEditorPath.value = file.path
    remoteEditorContent.value = file.content
    remoteEditorOriginalContent.value = file.content
    remoteTransferPath.value = file.path
    activeDockTab.value = 'editor'
    statusLine.value = `已从 ${file.path} 读取 ${file.bytes} 字节。`
  } catch (error) {
    if (requestToken === remoteEditorRequestToken) {
      statusLine.value = renderError(error)
    }
  } finally {
    if (requestToken === remoteEditorRequestToken) {
      remoteEditorLoading.value = false
    }
  }
}

async function saveRemoteTextFile() {
  const workspace = requireActiveWorkspace('保存远程文件')
  if (!workspace || !remoteEditorPath.value) {
    statusLine.value = '请先打开一个远程文本文件。'
    return
  }

  const requestToken = ++remoteEditorRequestToken
  const editorPath = remoteEditorPath.value
  const editorContent = remoteEditorContent.value
  remoteEditorLoading.value = true
  try {
    const secret = await resolveSecretForSession(workspace.id)
    await invoke('write_remote_text', {
      request: {
        session_id: workspace.id,
        remote_path: editorPath,
        content: editorContent,
        secret
      }
    })
    if (requestToken !== remoteEditorRequestToken || !isActiveWorkspaceSession(workspace.id)) {
      return
    }
    remoteEditorOriginalContent.value = editorContent
    statusLine.value = `已保存 ${editorPath}。`
    await refreshActiveWorkspaceDirectory(workspace.id, sftpPath.value)
  } catch (error) {
    if (requestToken === remoteEditorRequestToken) {
      statusLine.value = renderError(error)
    }
  } finally {
    if (requestToken === remoteEditorRequestToken) {
      remoteEditorLoading.value = false
    }
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
    message:
      record.status === 'queued' &&
      record.message === 'Recovered after restart' &&
      record.attempt_count > 0
        ? '上次运行中断，已恢复到队列'
        : record.message,
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
  if ((tab === 'browser' || tab === 'editor') && !activeWorkspaceSession.value) {
    statusLine.value = '右侧远端工作区未连接，先建立一个活动终端会话。'
  }
  activeDockTab.value = tab
  if (tab === 'editor' && activeWorkspaceSession.value && !remoteEditorPath.value && remoteTransferPath.value.trim()) {
    void openRemoteTextFile()
  }
}

watch(
  [connectSecret, rememberSecret],
  () => {
    persistSelectedSessionSecret()
  }
)

watch(
  [
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
    persistVisibleWorkspace()
  },
  { deep: true }
)

watch([activeDockTab, activeTerminalId, queueRunning, sftpBusy], () => {
  if (!canAcceptRemoteDrop()) {
    clearRemoteDropState()
  }
})

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

function primeRemoteContextTarget(target: RemoteContextTarget) {
  setRemoteMutationTarget(target.path, target.isDir)
  if (!selectedRemotePaths.value.includes(target.path)) {
    selectedRemotePaths.value = [target.path]
  }
}

function openRemoteContextMenu(event: MouseEvent, target: RemoteContextTarget) {
  event.preventDefault()
  primeRemoteContextTarget(target)
  remoteContextMenu.value = {
    x: event.clientX,
    y: event.clientY,
    target
  }
}

function openContextMenuDirectory() {
  const target = remoteContextMenu.value?.target
  if (!target) {
    return
  }
  closeRemoteContextMenu()
  void loadSftpDirectory(target.path)
}

function openContextMenuEditor() {
  const target = remoteContextMenu.value?.target
  if (!target) {
    return
  }
  closeRemoteContextMenu()
  void openRemoteTextFile(target.path)
}

function prepareContextMenuDownloadSource() {
  const target = remoteContextMenu.value?.target
  if (!target) {
    return
  }
  setRemoteMutationTarget(target.path, target.isDir)
  localTransferPath.value = target.name
  activeDockTab.value = 'browser'
  closeRemoteContextMenu()
}

function prepareContextMenuRename() {
  const target = remoteContextMenu.value?.target
  if (!target) {
    return
  }
  setRemoteMutationTarget(target.path, target.isDir)
  activeDockTab.value = 'browser'
  closeRemoteContextMenu()
}

async function queueContextMenuDownloads() {
  closeRemoteContextMenu()
  await batchQueueDownloads()
}

async function deleteContextMenuSelection() {
  const selectionCount = remoteContextSelectionCount.value
  closeRemoteContextMenu()
  if (selectionCount > 1) {
    await batchDeleteSelectedRemote()
    return
  }
  await deleteRemotePath()
}

function clearRemoteSelectionFromContextMenu() {
  clearRemoteSelection()
  closeRemoteContextMenu()
}

async function installRemoteDropListener() {
  remoteDropUnlisten = await getCurrentWebview().onDragDropEvent(async (event) => {
    const payload = event.payload

    if (payload.type === 'leave') {
      clearRemoteDropState()
      return
    }

    if (payload.type === 'enter') {
      remoteDropCandidateCount.value = payload.paths.length
      remoteDropActive.value = isPositionInsideRemoteDropZone(payload.position)
      return
    }

    if (payload.type === 'over') {
      remoteDropActive.value = isPositionInsideRemoteDropZone(payload.position)
      return
    }

    const droppedPaths = payload.paths
    const droppedInsideRemoteZone = isPositionInsideRemoteDropZone(payload.position)
    clearRemoteDropState()
    if (!droppedInsideRemoteZone || droppedPaths.length === 0) {
      return
    }

    await queueDraggedUploads(droppedPaths)
  })
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

function sessionNameForId(sessionId: string): string {
  return sessions.value.find((session) => session.id === sessionId)?.name ?? sessionId
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

async function inspectLocalPath(path: string) {
  return invoke<LocalPathInspection>('inspect_local_path', { path })
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
    if (!activeTerminalId.value || !canSendTerminalCommand(activeTerminalId.value)) {
      return
    }
    const sessionId = activeTerminalId.value
    invoke('send_terminal_input', {
      sessionId,
      input: data
    }).catch((error) => {
      handleTerminalCommandFailure(sessionId, error)
    })
  })

  terminal.onResize(({ cols, rows }) => {
    if (!activeTerminalId.value || !canSendTerminalCommand(activeTerminalId.value)) {
      return
    }
    const sessionId = activeTerminalId.value
    invoke('resize_terminal', {
      sessionId,
      cols,
      rows
    }).catch((error) => {
      handleTerminalCommandFailure(sessionId, error)
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
  await installRemoteDropListener()
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
  remoteDropUnlisten?.()
  clearRemoteDropState()
  for (const tab of terminalTabs.value) {
    invoke('disconnect_terminal', { sessionId: tab.sessionId }).catch(() => undefined)
  }
  terminal?.dispose()
})
</script>

<template>
  <div class="app-shell">
    <SessionSidebar
      :loading="loading"
      :busy="busy"
      :selected-session-id="selectedSessionId"
      :active-terminal-id="activeTerminalId"
      :state="sessionSidebarState"
      :filtered-sessions="filteredSessions"
      :editor-connect-label="editorConnectLabel"
      :on-start-new-session="startNewSession"
      :on-load-sessions="loadSessions"
      :on-select-session="selectSession"
      :on-connect-session-from-list="connectSessionFromList"
      :on-delete-session="deleteSession"
      :on-save-session="saveSession"
      :on-save-and-connect-session="saveAndConnectSession"
      :format-timestamp="formatTimestamp"
      :sync-label="syncLabel"
    />

    <main class="workspace-shell">
      <header class="topbar">
        <div class="workspace-tabs">
          <button
            v-for="tab in orderedTerminalTabs"
            :key="tab.sessionId"
            class="workspace-tab"
            :class="{
              active: tab.sessionId === activeTerminalId,
              connected: tab.status === 'Connected' || tab.status === 'Connecting',
              disconnected: tab.status === 'Disconnected' || tab.status === 'Failed'
            }"
            @click="activateTerminalTab(tab.sessionId)"
          >
            <span>{{ tab.sessionName }}</span>
            <small v-if="tab.unread">{{ tab.unread }}</small>
            <span class="workspace-tab-close" @click.stop="closeTerminalTab(tab.sessionId)">×</span>
          </button>
          <span v-if="!orderedTerminalTabs.length" class="workspace-caption">终端标签会在连接后出现</span>
        </div>

        <div class="actions toolbar-actions">
          <button class="ghost tight" :disabled="busy || !selectedSessionId" @click="connectTerminal">连接</button>
          <button class="ghost tight danger" :disabled="!activeTerminalId" @click="disconnectTerminal">断开</button>
          <button class="ghost tight" :disabled="!activeWorkspaceSession" @click="openDockTab('browser')">文件</button>
          <button class="ghost tight" :disabled="!activeWorkspaceSession" @click="openDockTab('editor')">编辑</button>
          <button class="ghost tight" @click="openDockTab('queue')">队列</button>
          <button class="ghost tight" @click="openDockTab('activity')">活动</button>
          <button class="ghost tight" @click="openDockTab('hosts')">主机</button>
        </div>
      </header>

      <section class="workspace-grid">
        <TerminalPanel
          :active-terminal-id="activeTerminalId"
          :active-terminal-name="activeTerminalName"
          :selected-session-id="selectedSessionId"
          :selected-session-name="selectedSession?.name ?? null"
          :selected-session-auth-label="selectedSessionAuthLabel"
          :active-workspace-name="activeWorkspaceSession?.name ?? null"
          :busy="busy"
          :terminal-status="terminalStatus"
          :credentials="terminalCredentialState"
          :secret-prompt="secretPrompt"
          :workspace-mismatch="workspaceMismatch"
          :terminal-context-auth-label="terminalContextAuthLabel"
          :terminal-context-tag-summary="terminalContextTagSummaryText"
          :selected-session-summary="selectedSessionSummary"
          :active-workspace-summary="activeWorkspaceSummary"
          :selected-session-remote-root="selectedSessionRemoteRoot"
          :terminal-context-remote-root="terminalContextRemoteRoot"
          :sessions-count="sessions.length"
          :queued-transfer-count="queuedTransferCount"
          :known-hosts-count="knownHosts.length"
          :running-transfer-count="runningTransferCount"
          :failed-transfer-count="failedTransferCount"
          :bind-terminal-host="bindTerminalHost"
          :on-connect-terminal="connectTerminal"
          :on-save-and-connect-session="saveAndConnectSession"
          :on-open-dock-tab="openDockTab"
          :on-save-current-secret="saveCurrentSecret"
          :on-forget-saved-secret="forgetSavedSecret"
          :terminal-status-label="terminalStatusLabel"
        />

        <WorkspaceDock
          :active-dock-tab="activeDockTab"
          :dock-tabs="dockTabs"
          :active-workspace-session="activeWorkspaceSession"
          :active-workspace-summary="activeWorkspaceSummary"
          :workspace-mismatch="workspaceMismatch"
          :selected-session-name="selectedSession?.name ?? null"
          :has-known-host-target="hasKnownHostTarget"
          :known-host-target-name="knownHostTargetName"
          :known-host-target-scope-label="knownHostTargetScopeLabel"
          :state="workspaceDockState"
          :visible-remote-tree-nodes="visibleRemoteTreeNodes"
          :selected-remote-paths="selectedRemotePaths"
          :sftp-busy="sftpBusy"
          :sftp-entries="sftpEntries"
          :remote-drop-active="remoteDropActive"
          :remote-drop-summary="remoteDropSummary"
          :remote-drop-hint="remoteDropHint"
          :remote-editor-title="remoteEditorTitle"
          :remote-editor-loading="remoteEditorLoading"
          :remote-editor-path="remoteEditorPath"
          :remote-editor-dirty="remoteEditorDirty"
          :transfer-queue="transferQueue"
          :queue-running="queueRunning"
          :transfer-events="transferEvents"
          :visible-transfer-events="visibleTransferEvents"
          :known-hosts="knownHosts"
          :on-open-dock-tab="openDockTab"
          :bind-remote-drop-zone="bindRemoteDropZone"
          :on-select-all-remote-entries="selectAllRemoteEntries"
          :on-clear-remote-selection="clearRemoteSelection"
          :on-go-to-parent-directory="goToParentDirectory"
          :on-load-sftp-directory="loadSftpDirectory"
          :on-open-remote-tree-node="openRemoteTreeNode"
          :on-toggle-remote-tree-node="toggleRemoteTreeNode"
          :on-select-remote-path-for-mutation="selectRemotePathForMutation"
          :on-open-remote-entry="openRemoteEntry"
          :on-open-remote-context-menu="openRemoteContextMenu"
          :on-toggle-remote-selection="toggleRemoteSelection"
          :on-create-remote-directory="createRemoteDirectory"
          :on-choose-upload-local-file="chooseUploadLocalFile"
          :on-choose-upload-local-directory="chooseUploadLocalDirectory"
          :on-choose-download-local-path="chooseDownloadLocalPath"
          :on-rename-remote-path="renameRemotePath"
          :on-delete-remote-path="deleteRemotePath"
          :on-queue-download-job="queueDownloadJob"
          :on-queue-upload-job="queueUploadJob"
          :on-batch-queue-uploads="batchQueueUploads"
          :on-batch-queue-downloads="batchQueueDownloads"
          :on-batch-delete-selected-remote="batchDeleteSelectedRemote"
          :on-download-selected-remote-file="downloadSelectedRemoteFile"
          :on-upload-local-file="uploadLocalFile"
          :on-open-remote-text-file="openRemoteTextFile"
          :on-save-remote-text-file="saveRemoteTextFile"
          :on-run-transfer-queue="runTransferQueue"
          :on-request-queue-stop="requestQueueStop"
          :on-clear-completed-transfers="clearCompletedTransfers"
          :on-persist-auto-resume-queue-setting="persistAutoResumeQueueSetting"
          :on-persist-background-on-close-setting="persistBackgroundOnCloseSetting"
          :on-persist-notification-setting="persistNotificationSetting"
          :on-persist-auto-retry-settings="persistAutoRetrySettings"
          :on-persist-transfer-behavior-settings="persistTransferBehaviorSettings"
          :on-retry-transfer-job="retryTransferJob"
          :on-cancel-running-transfer="cancelRunningTransfer"
          :on-remove-transfer-job="removeTransferJob"
          :on-load-transfer-events="loadTransferEvents"
          :on-clear-transfer-events="clearTransferEvents"
          :on-load-known-hosts="loadKnownHosts"
          :on-forget-known-host-target="forgetKnownHostTarget"
          :on-remove-known-host-line="removeKnownHostLine"
          :format-file-size="formatFileSize"
          :session-name-for-id="sessionNameForId"
          :transfer-kind-label="transferKindLabel"
          :transfer-status-label="transferStatusLabel"
          :transfer-level-label="transferLevelLabel"
          :format-timestamp="formatTimestamp"
          :is-remote-selected="isRemoteSelected"
        />
      </section>

      <footer class="status-bar">
        <span class="status-main">{{ statusLine }}</span>
        <span v-if="activeWorkspaceSession">{{ activeWorkspaceSession.name }}</span>
        <span>{{ runningTransferCount }} 传输 / {{ queuedTransferCount }} 排队 / {{ failedTransferCount }} 失败</span>
      </footer>
    </main>

    <div
      v-if="remoteContextMenu"
      class="remote-context-layer"
      @click="closeRemoteContextMenu"
      @contextmenu.prevent="closeRemoteContextMenu"
    >
      <div
        class="panel remote-context-menu"
        :style="{
          left: remoteContextMenu.x + 'px',
          top: remoteContextMenu.y + 'px'
        }"
        @click.stop
      >
        <div class="remote-context-head">
          <strong>{{ remoteContextSelectionLabel }}</strong>
          <small>{{ remoteContextSelectionMeta }}</small>
        </div>
        <button
          v-if="remoteContextMenu.target.isDir"
          class="ghost remote-context-item"
          @click="openContextMenuDirectory"
        >打开目录</button>
        <button
          v-if="!remoteContextMenu.target.isDir"
          class="ghost remote-context-item"
          @click="openContextMenuEditor"
        >在编辑器中打开</button>
        <button
          v-if="!remoteContextMenu.target.isDir"
          class="ghost remote-context-item"
          @click="prepareContextMenuDownloadSource"
        >选为下载源</button>
        <button
          v-if="remoteContextSelectionCount > 0"
          class="ghost remote-context-item"
          @click="queueContextMenuDownloads"
        >{{ remoteContextQueueDownloadLabel }}</button>
        <button
          class="ghost remote-context-item"
          @click="prepareContextMenuRename"
        >重命名</button>
        <button
          class="ghost danger remote-context-item"
          @click="deleteContextMenuSelection"
        >{{ remoteContextDeleteLabel }}</button>
        <button
          v-if="selectedRemotePaths.length"
          class="ghost remote-context-item"
          @click="clearRemoteSelectionFromContextMenu"
        >清空选择</button>
      </div>
    </div>
  </div>
</template>
