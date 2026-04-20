<script setup lang="ts">
import type { ComponentPublicInstance, Ref } from 'vue'
import type {
  DockTab,
  DockTabOption,
  KnownHostEntry,
  RemoteContextTarget,
  RemoteDirEntry,
  RemoteTreeNode,
  SessionProfile,
  TransferEventLevel,
  TransferEventRecordPayload,
  TransferJob,
  TransferJobKind,
  TransferJobStatus
} from '../types'

type ActionResult = void | Promise<unknown>
type TreeAction = (node: RemoteTreeNode) => ActionResult
type EntryAction = (entry: RemoteDirEntry) => ActionResult
type HostAction = (entry: KnownHostEntry) => ActionResult

interface WorkspaceDockState {
  sftpPath: Ref<string>
  remoteTransferPath: Ref<string>
  sftpCreatePath: Ref<string>
  sftpRenameTarget: Ref<string>
  localTransferPath: Ref<string>
  remoteEditorContent: Ref<string>
  autoResumeQueue: Ref<boolean>
  backgroundOnClose: Ref<boolean>
  enableNotifications: Ref<boolean>
  autoRetryTransfers: Ref<boolean>
  autoRemoveSuccessfulJobs: Ref<boolean>
  defaultMaxRetries: Ref<number>
  retryBaseDelaySeconds: Ref<number>
  retryMaxDelaySeconds: Ref<number>
  transferEventQuery: Ref<string>
  transferEventLevelFilter: Ref<'all' | TransferEventLevel>
}

interface Props {
  activeDockTab: DockTab
  dockTabs: DockTabOption[]
  activeWorkspaceSession: SessionProfile | null
  activeWorkspaceSummary: string
  workspaceMismatch: boolean
  selectedSessionName: string | null
  hasKnownHostTarget: boolean
  knownHostTargetName: string | null
  knownHostTargetScopeLabel: string
  state: WorkspaceDockState

  visibleRemoteTreeNodes: RemoteTreeNode[]
  selectedRemotePaths: string[]
  sftpBusy: boolean
  sftpEntries: RemoteDirEntry[]
  remoteDropActive: boolean
  remoteDropSummary: string
  remoteDropHint: string

  remoteEditorTitle: string
  remoteEditorLoading: boolean
  remoteEditorPath: string
  remoteEditorDirty: boolean

  transferQueue: TransferJob[]
  queueRunning: boolean

  transferEvents: TransferEventRecordPayload[]
  visibleTransferEvents: TransferEventRecordPayload[]

  knownHosts: KnownHostEntry[]

  onOpenDockTab: (tab: DockTab) => ActionResult
  bindRemoteDropZone: (element: Element | ComponentPublicInstance | null) => void
  onSelectAllRemoteEntries: () => ActionResult
  onClearRemoteSelection: () => ActionResult
  onGoToParentDirectory: () => ActionResult
  onLoadSftpDirectory: (path?: string) => ActionResult
  onOpenRemoteTreeNode: TreeAction
  onToggleRemoteTreeNode: TreeAction
  onSelectRemotePathForMutation: EntryAction
  onOpenRemoteEntry: EntryAction
  onOpenRemoteContextMenu: (event: MouseEvent, target: RemoteContextTarget) => ActionResult
  onToggleRemoteSelection: EntryAction
  onCreateRemoteDirectory: () => ActionResult
  onChooseUploadLocalFile: () => ActionResult
  onChooseUploadLocalDirectory: () => ActionResult
  onChooseDownloadLocalPath: () => ActionResult
  onRenameRemotePath: () => ActionResult
  onDeleteRemotePath: () => ActionResult
  onQueueDownloadJob: () => ActionResult
  onQueueUploadJob: () => ActionResult
  onBatchQueueUploads: () => ActionResult
  onBatchQueueDownloads: () => ActionResult
  onBatchDeleteSelectedRemote: () => ActionResult
  onDownloadSelectedRemoteFile: () => ActionResult
  onUploadLocalFile: () => ActionResult
  onOpenRemoteTextFile: (path?: string) => ActionResult
  onSaveRemoteTextFile: () => ActionResult

  onRunTransferQueue: () => ActionResult
  onRequestQueueStop: () => ActionResult
  onClearCompletedTransfers: () => ActionResult
  onPersistAutoResumeQueueSetting: () => ActionResult
  onPersistBackgroundOnCloseSetting: () => ActionResult
  onPersistNotificationSetting: () => ActionResult
  onPersistAutoRetrySettings: () => ActionResult
  onPersistTransferBehaviorSettings: () => ActionResult
  onRetryTransferJob: (jobId: string) => ActionResult
  onCancelRunningTransfer: (jobId: string) => ActionResult
  onRemoveTransferJob: (jobId: string) => ActionResult

  onLoadTransferEvents: () => ActionResult
  onClearTransferEvents: () => ActionResult

  onLoadKnownHosts: () => ActionResult
  onForgetKnownHostTarget: () => ActionResult
  onRemoveKnownHostLine: HostAction

  formatFileSize: (bytes: number | null | undefined) => string
  sessionNameForId: (sessionId: string) => string
  transferKindLabel: (value: TransferJobKind) => string
  transferStatusLabel: (value: TransferJobStatus) => string
  transferLevelLabel: (value: TransferEventLevel) => string
  formatTimestamp: (timestamp: number | null) => string
  isRemoteSelected: (path: string) => boolean
}

const props = defineProps<Props>()
</script>

<template>
  <aside class="panel dock-panel">
    <div class="workspace-context-card">
      <div class="workspace-context-head">
        <div>
          <p class="eyebrow">右侧操作目标</p>
          <strong>{{ props.activeWorkspaceSession?.name ?? '未连接远端工作区' }}</strong>
          <p class="subcopy">{{ props.activeWorkspaceSummary }}</p>
        </div>
        <span class="status-pill" :data-state="props.activeWorkspaceSession ? 'connected' : 'idle'">
          {{ props.activeWorkspaceSession ? '已绑定' : '未连接' }}
        </span>
      </div>
      <p v-if="props.workspaceMismatch" class="workspace-context-note">
        左侧仍在编辑 {{ props.selectedSessionName }}，右侧所有远端操作固定绑定
        {{ props.activeWorkspaceSession?.name }}。
      </p>
      <p v-else-if="!props.activeWorkspaceSession" class="workspace-context-note">
        右侧文件浏览、编辑和传输只跟随活动终端，不再跟随左侧选中项。
      </p>
    </div>

    <div class="dock-tabs">
      <button
        v-for="tab in props.dockTabs"
        :key="tab.id"
        class="dock-tab"
        :class="{ active: props.activeDockTab === tab.id }"
        @click="props.onOpenDockTab(tab.id)"
      >
        {{ tab.label }}
      </button>
    </div>

    <div v-if="props.activeDockTab === 'browser'" class="dock-pane">
      <template v-if="props.activeWorkspaceSession">
        <div class="panel-head panel-head--tight">
          <div>
            <p class="eyebrow">SSH 浏览器</p>
            <h2>远程文件</h2>
          </div>
          <div class="actions">
            <button
              class="ghost"
              :disabled="!props.sftpEntries.length || props.selectedRemotePaths.length === props.sftpEntries.length"
              @click="props.onSelectAllRemoteEntries"
            >
              全选当前页
            </button>
            <button class="ghost" :disabled="!props.selectedRemotePaths.length" @click="props.onClearRemoteSelection">
              清空选择
            </button>
            <button class="ghost" :disabled="props.sftpBusy" @click="props.onGoToParentDirectory">上一级</button>
            <button class="ghost" :disabled="props.sftpBusy" @click="props.onLoadSftpDirectory()">
              刷新
            </button>
          </div>
        </div>

        <div class="sftp-toolbar">
          <label>
            <span>远程目录</span>
            <input v-model="props.state.sftpPath.value" placeholder="/" />
          </label>
          <button class="primary" :disabled="props.sftpBusy" @click="props.onLoadSftpDirectory()">
            加载
          </button>
        </div>

        <div class="browser-grid">
          <div class="tree-panel">
            <div class="section-title">
              <span>目录树</span>
              <span>{{ props.visibleRemoteTreeNodes.length }}</span>
            </div>
            <button
              v-for="node in props.visibleRemoteTreeNodes"
              :key="node.path"
              class="tree-node"
              :style="{ paddingLeft: `${12 + node.depth * 16}px` }"
              @click="props.onOpenRemoteTreeNode(node)"
            >
              <span class="tree-toggle" @click.stop="props.onToggleRemoteTreeNode(node)">
                {{ node.loading ? '…' : node.expanded ? '▾' : '▸' }}
              </span>
              <span class="tree-label">{{ node.name }}</span>
            </button>
            <p v-if="!props.visibleRemoteTreeNodes.length" class="empty-copy">
              先保存至少一个远程书签，或连接后从 `/` 开始浏览。
            </p>
          </div>

          <div :ref="props.bindRemoteDropZone" class="sftp-list" :class="{ 'sftp-list--drop-active': props.remoteDropActive }">
            <div class="section-title">
              <span>当前目录</span>
              <span>
                {{
                  props.selectedRemotePaths.length
                    ? `${props.sftpEntries.length} · 已选 ${props.selectedRemotePaths.length}`
                    : props.sftpEntries.length
                }}
              </span>
            </div>
            <div v-if="props.remoteDropActive" class="remote-drop-overlay">
              <strong>{{ props.remoteDropSummary }}</strong>
              <span>{{ props.remoteDropHint }}</span>
            </div>
            <button
              v-for="entry in props.sftpEntries"
              :key="entry.path"
              class="sftp-entry"
              @click="props.onSelectRemotePathForMutation(entry)"
              @dblclick="props.onOpenRemoteEntry(entry)"
              @contextmenu="props.onOpenRemoteContextMenu($event, { path: entry.path, name: entry.name, isDir: entry.is_dir })"
            >
              <div class="sftp-entry-head">
                <label class="checkbox-row">
                  <input
                    :checked="props.isRemoteSelected(entry.path)"
                    type="checkbox"
                    @click.stop
                    @change="props.onToggleRemoteSelection(entry)"
                  />
                  <span>{{ entry.is_dir ? '目录' : '文件' }}</span>
                </label>
                <small>{{ props.formatFileSize(entry.size) }}</small>
              </div>
              <strong>{{ entry.name }}</strong>
              <small>{{ entry.path }}</small>
            </button>
            <p v-if="!props.sftpEntries.length" class="empty-copy">
              先连接到会话，右侧 SFTP 面板才会有内容。
            </p>
          </div>
        </div>

        <div class="dock-form-grid">
          <label>
            <span>新目录路径</span>
            <input v-model="props.state.sftpCreatePath.value" placeholder="/tmp/new-folder" />
          </label>
          <label>
            <span>远程路径</span>
            <input v-model="props.state.remoteTransferPath.value" placeholder="/root/example.txt" />
          </label>
          <label>
            <span>重命名目标</span>
            <input v-model="props.state.sftpRenameTarget.value" placeholder="/root/example-renamed.txt" />
          </label>
          <label>
            <span>本地路径</span>
            <input v-model="props.state.localTransferPath.value" placeholder="/root/downloads/example.txt" />
          </label>
        </div>

        <div class="actions dock-actions">
          <button class="ghost" :disabled="props.sftpBusy" @click="props.onCreateRemoteDirectory">创建目录</button>
          <button class="ghost" :disabled="props.sftpBusy || props.queueRunning" @click="props.onChooseUploadLocalFile">
            选择文件
          </button>
          <button
            class="ghost"
            :disabled="props.sftpBusy || props.queueRunning"
            @click="props.onChooseUploadLocalDirectory"
          >
            选择目录
          </button>
          <button class="ghost" :disabled="props.sftpBusy || props.queueRunning" @click="props.onChooseDownloadLocalPath">
            选择保存路径
          </button>
          <button class="ghost" :disabled="props.sftpBusy" @click="props.onRenameRemotePath">重命名</button>
          <button class="ghost danger" :disabled="props.sftpBusy" @click="props.onDeleteRemotePath">删除</button>
          <button class="ghost" :disabled="props.sftpBusy || props.queueRunning" @click="props.onQueueDownloadJob">
            加入下载队列
          </button>
          <button class="ghost" :disabled="props.sftpBusy || props.queueRunning" @click="props.onQueueUploadJob">
            加入上传队列
          </button>
          <button class="ghost" :disabled="props.queueRunning || props.sftpBusy" @click="props.onBatchQueueUploads">
            批量上传
          </button>
          <button class="ghost" :disabled="!props.selectedRemotePaths.length || props.sftpBusy" @click="props.onBatchQueueDownloads">
            批量下载
          </button>
          <button class="ghost danger" :disabled="!props.selectedRemotePaths.length || props.sftpBusy" @click="props.onBatchDeleteSelectedRemote">
            批量删除
          </button>
          <button class="ghost" :disabled="props.sftpBusy" @click="props.onDownloadSelectedRemoteFile">下载</button>
          <button class="primary" :disabled="props.sftpBusy" @click="props.onUploadLocalFile">上传</button>
        </div>
      </template>
      <div v-else class="workspace-blocker">
        <p class="eyebrow">SSH 浏览器</p>
        <h2>先连接一个工作区</h2>
        <p>
          右侧远端浏览只跟随活动终端。先在左侧选择会话并连接，或者切换到一个已经打开的终端标签。
        </p>
      </div>
    </div>

    <div v-else-if="props.activeDockTab === 'editor'" class="dock-pane">
      <template v-if="props.activeWorkspaceSession">
        <div class="panel-head panel-head--tight">
          <div>
            <p class="eyebrow">远程编辑器</p>
            <h2>{{ props.remoteEditorTitle }}</h2>
          </div>
          <div class="actions">
            <button class="ghost" :disabled="props.remoteEditorLoading || !props.state.remoteTransferPath.value" @click="props.onOpenRemoteTextFile()">
              载入
            </button>
            <button class="ghost" :disabled="props.remoteEditorLoading || !props.remoteEditorPath" @click="props.onOpenRemoteTextFile(props.remoteEditorPath)">
              重新载入
            </button>
            <button class="primary" :disabled="props.remoteEditorLoading || !props.remoteEditorDirty" @click="props.onSaveRemoteTextFile">
              保存到远程
            </button>
          </div>
        </div>

        <div class="dock-form-grid">
          <label>
            <span>远程文本路径</span>
            <input v-model="props.state.remoteTransferPath.value" placeholder="/etc/nginx/nginx.conf" />
          </label>
          <label>
            <span>编辑状态</span>
            <input
              :value="
                props.remoteEditorLoading
                  ? '加载中…'
                  : props.remoteEditorPath
                    ? props.remoteEditorDirty
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
            v-model="props.state.remoteEditorContent.value"
            rows="20"
            :disabled="props.remoteEditorLoading || !props.remoteEditorPath"
            placeholder="在 SSH 浏览器里双击一个远程文件，就会加载到这里。"
          />
        </label>

        <p class="empty-copy">
          这个内联编辑器主要适合配置文件和脚本。非 UTF-8 文件或较大的二进制文件仍然建议通过下载/上传处理。
        </p>
      </template>
      <div v-else class="workspace-blocker">
        <p class="eyebrow">远程编辑器</p>
        <h2>还没有活动远端文件</h2>
        <p>
          先连接一个工作区，然后在 SSH 浏览器里双击远程文件。编辑器现在只会对活动工作区生效。
        </p>
      </div>
    </div>

    <div v-else-if="props.activeDockTab === 'queue'" class="dock-pane">
      <div class="panel-head panel-head--tight">
        <div>
          <p class="eyebrow">传输队列</p>
          <h2>顺序任务</h2>
        </div>
        <div class="actions">
          <button class="ghost" :disabled="props.queueRunning || !props.transferQueue.length" @click="props.onRunTransferQueue">
            {{ props.queueRunning ? '运行中...' : '运行队列' }}
          </button>
          <button class="ghost" :disabled="!props.queueRunning" @click="props.onRequestQueueStop">当前任务后停止</button>
          <button class="ghost" :disabled="!props.transferQueue.length" @click="props.onClearCompletedTransfers">
            清理已完成
          </button>
        </div>
      </div>

      <div class="settings-grid">
        <label class="checkbox-row">
          <input v-model="props.state.autoResumeQueue.value" type="checkbox" @change="props.onPersistAutoResumeQueueSetting" />
          <span>启动时自动恢复</span>
        </label>
        <label class="checkbox-row">
          <input v-model="props.state.backgroundOnClose.value" type="checkbox" @change="props.onPersistBackgroundOnCloseSetting" />
          <span>关闭时最小化到托盘</span>
        </label>
        <label class="checkbox-row">
          <input v-model="props.state.enableNotifications.value" type="checkbox" @change="props.onPersistNotificationSetting" />
          <span>系统通知</span>
        </label>
        <label class="checkbox-row">
          <input v-model="props.state.autoRetryTransfers.value" type="checkbox" @change="props.onPersistAutoRetrySettings" />
          <span>瞬时失败自动重试</span>
        </label>
        <label class="checkbox-row">
          <input v-model="props.state.autoRemoveSuccessfulJobs.value" type="checkbox" @change="props.onPersistTransferBehaviorSettings" />
          <span>自动移除成功任务</span>
        </label>
        <label>
          <span>重试次数</span>
          <input v-model="props.state.defaultMaxRetries.value" type="number" min="0" max="9" @change="props.onPersistAutoRetrySettings" />
        </label>
        <label>
          <span>基础延迟</span>
          <input v-model="props.state.retryBaseDelaySeconds.value" type="number" min="1" max="60" @change="props.onPersistTransferBehaviorSettings" />
        </label>
        <label>
          <span>最大延迟</span>
          <input v-model="props.state.retryMaxDelaySeconds.value" type="number" min="1" max="300" @change="props.onPersistTransferBehaviorSettings" />
        </label>
      </div>

      <div v-if="props.transferQueue.length" class="queue-list">
        <div v-for="job in props.transferQueue" :key="job.id" class="queue-item">
          <div class="queue-item-head">
            <strong>{{ props.transferKindLabel(job.kind) }}</strong>
            <span class="badge">{{ props.transferStatusLabel(job.status) }}</span>
          </div>
          <small>会话 {{ props.sessionNameForId(job.sessionId) }}</small>
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
            <button class="ghost" :disabled="job.status === 'running'" @click="props.onRetryTransferJob(job.id)">
              重试
            </button>
            <button class="ghost danger" :disabled="job.status !== 'running'" @click="props.onCancelRunningTransfer(job.id)">
              取消
            </button>
            <button class="ghost danger" :disabled="job.status === 'running'" @click="props.onRemoveTransferJob(job.id)">
              移除
            </button>
          </div>
        </div>
      </div>
      <p v-else class="empty-copy">
        上传和下载任务会在这里按顺序执行，作为右侧停靠的传输面板。
      </p>
    </div>

    <div v-else-if="props.activeDockTab === 'activity'" class="dock-pane">
      <div class="panel-head panel-head--tight">
        <div>
          <p class="eyebrow">传输活动</p>
          <h2>最近事件</h2>
        </div>
        <div class="actions">
          <button class="ghost" @click="props.onLoadTransferEvents">刷新</button>
          <button class="ghost danger" :disabled="!props.transferEvents.length" @click="props.onClearTransferEvents">
            清空日志
          </button>
        </div>
      </div>

      <div class="settings-grid">
        <label>
          <span>搜索</span>
          <input v-model="props.state.transferEventQuery.value" placeholder="任务 ID、消息、会话 ID" />
        </label>
        <label>
          <span>级别</span>
          <select v-model="props.state.transferEventLevelFilter.value">
            <option value="all">全部</option>
            <option value="info">信息</option>
            <option value="warning">警告</option>
            <option value="error">错误</option>
          </select>
        </label>
      </div>

      <div class="known-hosts-list">
        <div v-for="event in props.visibleTransferEvents" :key="event.id" class="known-host-entry">
          <strong>{{ props.transferLevelLabel(event.level) }} · {{ event.job_id }}</strong>
          <small>会话 {{ props.sessionNameForId(event.session_id) }}</small>
          <span>{{ event.message }}</span>
          <small>{{ props.formatTimestamp(event.created_at) }}</small>
        </div>
        <p v-if="!props.visibleTransferEvents.length" class="empty-copy">
          还没有传输事件记录。
        </p>
      </div>
    </div>

    <div v-else class="dock-pane">
      <div class="panel-head panel-head--tight">
        <div>
          <p class="eyebrow">主机信任</p>
          <h2>known_hosts</h2>
          <p class="subcopy">
            {{
              props.hasKnownHostTarget
                ? `当前目标：${props.knownHostTargetScopeLabel} ${props.knownHostTargetName}`
                : '先选择左侧会话，或连接一个活动工作区。'
            }}
          </p>
        </div>
        <div class="actions">
          <button class="ghost" @click="props.onLoadKnownHosts">刷新</button>
          <button class="ghost danger" :disabled="!props.hasKnownHostTarget" @click="props.onForgetKnownHostTarget">
            忘记当前目标主机
          </button>
        </div>
      </div>

      <div class="known-hosts-list">
        <div v-for="entry in props.knownHosts" :key="entry.line" class="known-host-entry">
          <strong>#{{ entry.line }} · {{ entry.key_type }}</strong>
          <span>{{ entry.hosts }}</span>
          <small>{{ entry.hashed ? '已哈希主机模式' : '明文主机模式' }}</small>
          <div class="actions">
            <button class="ghost danger" @click="props.onRemoveKnownHostLine(entry)">删除</button>
          </div>
        </div>
        <p v-if="!props.knownHosts.length" class="empty-copy">
          当前用户还没有 known_hosts 记录。
        </p>
      </div>
    </div>
  </aside>
</template>
