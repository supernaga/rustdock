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
    <div class="terminal-toolbar">
      <div class="terminal-toolbar-left">
        <span class="status-pill" :data-state="props.terminalStatus.toLowerCase()">
          {{ props.terminalStatusLabel(props.terminalStatus) }}
        </span>
        <strong class="terminal-title">
          {{ props.activeTerminalName || props.selectedSessionName || '未连接' }}
        </strong>
        <span class="muted-inline">{{ props.selectedSessionSummary }}</span>
      </div>

      <div class="terminal-toolbar-right">
        <input
          class="secret-inline"
          v-model="props.credentials.connectSecret.value"
          type="password"
          autocomplete="off"
          :placeholder="props.secretPrompt"
          :title="props.secretPrompt"
        />
        <label class="checkbox-row compact-check">
          <input v-model="props.credentials.rememberSecret.value" type="checkbox" />
          <span>记住</span>
        </label>
        <button
          class="ghost tight"
          :disabled="!props.selectedSessionId || !props.credentials.connectSecret.value.trim()"
          title="保存凭据到钥匙串"
          @click="props.onSaveCurrentSecret"
        >
          存密
        </button>
        <button
          class="primary tight"
          :disabled="props.busy || !props.selectedSessionId"
          @click="props.onConnectTerminal"
        >
          连接
        </button>
      </div>
    </div>

    <div
      v-if="props.workspaceMismatch && props.selectedSessionName && props.activeWorkspaceName"
      class="context-banner compact-banner"
    >
      左侧编辑 <strong>{{ props.selectedSessionName }}</strong>，文件区绑定
      <strong>{{ props.activeWorkspaceName }}</strong>
    </div>

    <div class="terminal-stack">
      <div
        :ref="props.bindTerminalHost"
        class="terminal-host"
        :class="{ 'terminal-host--idle': !props.activeTerminalId }"
      ></div>

      <div v-if="!props.activeTerminalId" class="terminal-overlay">
        <div class="terminal-empty">
          <h3>{{ props.selectedSessionName || '选择左侧会话' }}</h3>
          <p>双击会话连接，或点上方「连接」。中间为终端，右侧为文件/传输。</p>
          <div class="actions">
            <button class="primary" :disabled="props.busy || !props.selectedSessionId" @click="props.onConnectTerminal">
              连接
            </button>
            <button class="ghost" :disabled="props.busy" @click="props.onSaveAndConnectSession">
              保存并连接
            </button>
          </div>
        </div>
      </div>
    </div>
  </section>
</template>
