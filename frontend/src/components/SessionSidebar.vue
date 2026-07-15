<script setup lang="ts">
import type { Ref } from 'vue'
import type { SessionFormState, SessionProfile } from '../types'

type ActionResult = void | Promise<unknown>

interface SessionSidebarState {
  sessionFilter: Ref<string>
  form: Ref<SessionFormState>
  connectSecret: Ref<string>
  rememberSecret: Ref<boolean>
}

interface Props {
  loading: boolean
  busy: boolean
  selectedSessionId: string | null
  activeTerminalId: string | null
  state: SessionSidebarState
  filteredSessions: SessionProfile[]
  editorConnectLabel: string

  onStartNewSession: () => ActionResult
  onLoadSessions: () => ActionResult
  onSelectSession: (session: SessionProfile) => ActionResult
  onConnectSessionFromList: (session: SessionProfile) => ActionResult
  onDeleteSession: () => ActionResult
  onSaveSession: () => ActionResult
  onSaveAndConnectSession: () => ActionResult

  formatTimestamp: (timestamp: number | null) => string
  syncLabel: (value: SessionProfile['sync_state']) => string
}

const props = defineProps<Props>()
</script>

<template>
  <aside class="left-rail">
    <div class="rail-header">
      <div class="brand-compact">
        <strong>RustDock</strong>
        <span>会话</span>
      </div>
      <div class="rail-actions">
        <button class="ghost tight" title="新建会话" @click="props.onStartNewSession">+</button>
        <button class="ghost tight" title="刷新" :disabled="props.loading" @click="props.onLoadSessions">↻</button>
      </div>
    </div>

    <input
      class="search-input"
      v-model="props.state.sessionFilter.value"
      placeholder="搜索主机 / 用户 / 标签"
    />

    <div class="session-directory">
      <button
        v-for="session in props.filteredSessions"
        :key="session.id"
        class="session-card"
        :class="{
          selected: session.id === props.selectedSessionId,
          live: session.id === props.activeTerminalId
        }"
        :title="`${session.username}@${session.host}:${session.port}`"
        @click="props.onSelectSession(session)"
        @dblclick="props.onConnectSessionFromList(session)"
      >
        <div class="session-card-head">
          <strong>{{ session.name }}</strong>
          <span v-if="session.id === props.activeTerminalId" class="dot live-dot" title="在线"></span>
        </div>
        <span class="session-endpoint">{{ session.username }}@{{ session.host }}:{{ session.port }}</span>
      </button>

      <p v-if="!props.filteredSessions.length" class="empty-copy">
        暂无会话。填写下方表单后保存。
      </p>
    </div>

    <section class="editor-card">
      <div class="panel-head panel-head--tight">
        <strong class="editor-title">{{ props.state.form.value.id ? '编辑会话' : '新建会话' }}</strong>
        <div class="actions">
          <button class="ghost tight danger" :disabled="props.busy || !props.selectedSessionId" @click="props.onDeleteSession">
            删
          </button>
          <button class="ghost tight" :disabled="props.busy" @click="props.onSaveSession">存</button>
        </div>
      </div>

      <div class="editor-grid">
        <label>
          <span>名称</span>
          <input v-model="props.state.form.value.name" placeholder="生产机" />
        </label>
        <label>
          <span>主机</span>
          <input v-model="props.state.form.value.host" placeholder="1.2.3.4" />
        </label>
        <label>
          <span>端口</span>
          <input v-model="props.state.form.value.port" inputmode="numeric" />
        </label>
        <label>
          <span>用户</span>
          <input v-model="props.state.form.value.username" placeholder="root" />
        </label>
        <label>
          <span>认证</span>
          <select v-model="props.state.form.value.authType">
            <option value="password">密码</option>
            <option value="private-key">私钥</option>
            <option value="agent">Agent</option>
          </select>
        </label>
        <label v-if="props.state.form.value.authType === 'private-key'">
          <span>密钥</span>
          <input v-model="props.state.form.value.keyPath" placeholder="C:\Users\...\.ssh\id_ed25519" />
        </label>
        <label v-else-if="props.state.form.value.authType === 'password'">
          <span>密码</span>
          <input
            v-model="props.state.connectSecret.value"
            type="password"
            autocomplete="off"
            placeholder="SSH 密码"
          />
        </label>
        <label v-else>
          <span>Agent</span>
          <input value="系统 SSH Agent / Pageant" disabled />
        </label>
      </div>

      <div v-if="props.state.form.value.authType === 'password'" class="draft-secret-panel">
        <label class="checkbox-row">
          <input v-model="props.state.rememberSecret.value" type="checkbox" />
          <span>记住密码</span>
        </label>
      </div>

      <details class="advanced-fold">
        <summary>高级选项</summary>
        <div class="stack compact-stack">
          <label>
            <span>标签</span>
            <input v-model="props.state.form.value.tagsInput" placeholder="prod, web" />
          </label>
          <label>
            <span>远程目录</span>
            <textarea v-model="props.state.form.value.remoteRootsInput" rows="2" placeholder="/home&#10;/var/www" />
          </label>
          <label>
            <span>本地目录</span>
            <textarea v-model="props.state.form.value.localRootsInput" rows="2" placeholder="D:\work" />
          </label>
          <label>
            <span>备注</span>
            <textarea v-model="props.state.form.value.notes" rows="2" placeholder="可选" />
          </label>
        </div>
      </details>

      <div class="actions actions--spread">
        <button class="ghost" :disabled="props.busy" @click="props.onSaveSession">仅保存</button>
        <button class="primary" :disabled="props.busy" @click="props.onSaveAndConnectSession">
          {{ props.editorConnectLabel }}
        </button>
      </div>
    </section>
  </aside>
</template>
