<script setup lang="ts">
import type { ComponentPublicInstance, Ref } from 'vue'
import type { DockTab, SessionStatus } from '../types'

type ActionResult = void | Promise<unknown>

interface TerminalCredentialState {
  connectSecret: Ref<string>
  rememberSecret: Ref<boolean>
}

interface Props {
  activeTerminalId: string | null
  activeTerminalName: string
  selectedSessionId: string | null
  selectedSessionName: string | null
  selectedSessionAuthLabel: string
  activeWorkspaceName: string | null
  busy: boolean
  terminalStatus: SessionStatus
  credentials: TerminalCredentialState
  secretPrompt: string
  workspaceMismatch: boolean
  terminalContextAuthLabel: string
  terminalContextTagSummary: string
  selectedSessionSummary: string
  activeWorkspaceSummary: string
  selectedSessionRemoteRoot: string
  terminalContextRemoteRoot: string
  sessionsCount: number
  queuedTransferCount: number
  knownHostsCount: number
  runningTransferCount: number
  failedTransferCount: number
  bindTerminalHost: (element: Element | ComponentPublicInstance | null) => void

  onConnectTerminal: () => ActionResult
  onSaveAndConnectSession: () => ActionResult
  onOpenDockTab: (tab: DockTab) => ActionResult
  onSaveCurrentSecret: () => ActionResult
  onForgetSavedSecret: () => ActionResult

  terminalStatusLabel: (value: SessionStatus) => string
}

const props = defineProps<Props>()
</script>

<template>
  <section class="panel terminal-panel shell-panel">
    <div class="panel-head">
      <div>
        <p class="eyebrow">终端工作区</p>
        <h2>{{ props.activeTerminalName || props.selectedSessionName || '等待连接' }}</h2>
        <p class="subcopy">
          {{
            props.activeTerminalId
              ? `当前已连接到 ${props.activeTerminalName}`
              : '选择一个书签并连接后，就会在这里打开实时终端。'
          }}
        </p>
      </div>

      <div class="terminal-meta">
        <span class="status-pill" :data-state="props.terminalStatus.toLowerCase()">
          {{ props.terminalStatusLabel(props.terminalStatus) }}
        </span>
        <span class="badge">工作区 · {{ props.terminalContextAuthLabel }}</span>
        <span v-if="props.terminalContextTagSummary" class="badge">{{ props.terminalContextTagSummary }}</span>
        <span v-if="props.workspaceMismatch && props.activeWorkspaceName" class="badge">
          右侧锁定 {{ props.activeWorkspaceName }}
        </span>
      </div>
    </div>

    <div class="terminal-auth terminal-auth--toolbar">
      <label>
        <span>{{
          props.selectedSessionName ? `${props.selectedSessionName} 的${props.secretPrompt}` : props.secretPrompt
        }}</span>
        <input
          v-model="props.credentials.connectSecret.value"
          type="password"
          autocomplete="off"
          :placeholder="props.secretPrompt"
        />
      </label>

      <div class="terminal-auth-side">
        <div class="credential-context-card">
          <strong>{{ props.selectedSessionName ?? '未选择凭据目标' }}</strong>
          <small>{{ props.selectedSessionSummary }}</small>
          <small>认证方式：{{ props.selectedSessionAuthLabel }}</small>
          <small v-if="props.workspaceMismatch && props.selectedSessionName && props.activeWorkspaceName">
            终端状态显示 {{ props.activeWorkspaceName }}，这里编辑的是 {{ props.selectedSessionName }} 的连接凭据。
          </small>
        </div>

        <div class="actions">
          <label class="checkbox-row">
            <input v-model="props.credentials.rememberSecret.value" type="checkbox" />
            <span>保存到系统钥匙串</span>
          </label>
          <button
            class="ghost"
            :disabled="!props.selectedSessionId || !props.credentials.connectSecret.value.trim()"
            @click="props.onSaveCurrentSecret"
          >
            保存当前草稿凭据
          </button>
          <button class="ghost danger" :disabled="!props.selectedSessionId" @click="props.onForgetSavedSecret">
            忘记当前草稿凭据
          </button>
        </div>
      </div>
    </div>

    <div v-if="props.workspaceMismatch && props.selectedSessionName && props.activeWorkspaceName" class="context-banner">
      左侧正在编辑 <strong>{{ props.selectedSessionName }}</strong>，但右侧远端浏览、编辑和传输仍固定绑定
      <strong>{{ props.activeWorkspaceName }}</strong>。
    </div>

    <div class="terminal-stack">
      <div :ref="props.bindTerminalHost" class="terminal-host" :class="{ 'terminal-host--idle': !props.activeTerminalId }"></div>

      <div v-if="!props.activeTerminalId" class="terminal-overlay">
        <div class="terminal-empty">
          <p class="eyebrow">连接中心</p>
          <h3>{{ props.selectedSessionName || '尚未选择会话' }}</h3>
          <p>
            当前界面按终端优先的工作流组织：左侧选会话，点击连接，中间跑终端，右侧处理文件浏览和编辑。
          </p>

          <div class="actions">
            <button class="primary" :disabled="props.busy || !props.selectedSessionId" @click="props.onConnectTerminal">
              连接当前会话
            </button>
            <button class="ghost" :disabled="props.busy" @click="props.onSaveAndConnectSession">保存并连接</button>
            <button class="ghost" @click="props.onOpenDockTab('browser')">打开 SSH 浏览器</button>
          </div>

          <div class="summary-grid">
            <div class="summary-card">
              <strong>{{ props.sessionsCount }}</strong>
              <span>已保存会话</span>
            </div>
            <div class="summary-card">
              <strong>{{ props.queuedTransferCount }}</strong>
              <span>排队任务</span>
            </div>
            <div class="summary-card">
              <strong>{{ props.knownHostsCount }}</strong>
              <span>known_hosts 记录</span>
            </div>
            <div class="summary-card">
              <strong>{{ props.selectedSessionRemoteRoot }}</strong>
              <span>主远程目录</span>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div class="terminal-footer terminal-footer--cards">
      <div class="terminal-foot-card">
        <strong>左侧草稿</strong>
        <p>{{ props.selectedSessionSummary }}</p>
      </div>
      <div class="terminal-foot-card">
        <strong>活动工作区</strong>
        <p>{{ props.activeWorkspaceSummary }}</p>
      </div>
      <div class="terminal-foot-card">
        <strong>远程根目录</strong>
        <p>{{ props.terminalContextRemoteRoot }}</p>
      </div>
      <div class="terminal-foot-card">
        <strong>队列状态</strong>
        <p>{{ props.runningTransferCount }} 个执行中 · {{ props.queuedTransferCount }} 个排队中 · {{ props.failedTransferCount }} 个失败</p>
      </div>
    </div>
  </section>
</template>
