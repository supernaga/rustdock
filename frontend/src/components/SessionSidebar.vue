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
    <div class="brand-block">
      <p class="eyebrow">中文工作台</p>
      <h1>RustDock</h1>
      <p class="subcopy">
        左侧管理会话书签，中间是终端主工作区，右侧是可停靠的 SSH 文件浏览与编辑面板。
      </p>
    </div>

    <div class="rail-actions">
      <button class="primary" @click="props.onStartNewSession">新建会话</button>
      <button class="ghost" :disabled="props.loading" @click="props.onLoadSessions">刷新列表</button>
    </div>

    <label class="search-card">
      <span>会话搜索</span>
      <input v-model="props.state.sessionFilter.value" placeholder="主机、标签、用户名" />
    </label>

    <div class="session-directory">
      <div class="section-title">
        <span>已保存会话</span>
        <span>{{ props.filteredSessions.length }}</span>
      </div>

      <button
        v-for="session in props.filteredSessions"
        :key="session.id"
        class="session-card"
        :class="{
          selected: session.id === props.selectedSessionId,
          live: session.id === props.activeTerminalId
        }"
        @click="props.onSelectSession(session)"
        @dblclick="props.onConnectSessionFromList(session)"
      >
        <div class="session-card-head">
          <strong>{{ session.name }}</strong>
          <span class="badge">{{ session.id === props.activeTerminalId ? '在线' : props.syncLabel(session.sync_state) }}</span>
        </div>
        <span>{{ session.username }}@{{ session.host }}:{{ session.port }}</span>
        <small>
          {{
            session.tags.length
              ? session.tags.join(' · ')
              : `上次连接 ${props.formatTimestamp(session.last_connected_at)}`
          }}
        </small>
      </button>

      <p v-if="!props.filteredSessions.length" class="empty-copy">
        还没有已保存会话。先在下方填写草稿并保存，然后双击即可连接。
      </p>
    </div>

    <section class="editor-card">
      <div class="panel-head panel-head--tight">
        <div>
          <p class="eyebrow">会话草稿</p>
          <h2>{{ props.state.form.value.id ? '编辑配置' : '快速连接草稿' }}</h2>
        </div>
        <div class="actions">
          <button class="ghost danger" :disabled="props.busy || !props.selectedSessionId" @click="props.onDeleteSession">
            删除
          </button>
          <button class="primary" :disabled="props.busy" @click="props.onSaveSession">保存</button>
        </div>
      </div>

      <div class="editor-grid">
        <label>
          <span>名称</span>
          <input v-model="props.state.form.value.name" placeholder="生产堡垒机" />
        </label>
        <label>
          <span>主机</span>
          <input v-model="props.state.form.value.host" placeholder="bastion.example.com" />
        </label>
        <label>
          <span>端口</span>
          <input v-model="props.state.form.value.port" inputmode="numeric" />
        </label>
        <label>
          <span>用户名</span>
          <input v-model="props.state.form.value.username" placeholder="root" />
        </label>
        <label>
          <span>认证方式</span>
          <select v-model="props.state.form.value.authType">
            <option value="private-key">私钥</option>
            <option value="agent">SSH 代理</option>
            <option value="password">密码</option>
          </select>
        </label>
        <label v-if="props.state.form.value.authType === 'private-key'">
          <span>密钥路径</span>
          <input v-model="props.state.form.value.keyPath" placeholder="~/.ssh/id_ed25519" />
        </label>
        <label v-else-if="props.state.form.value.authType === 'agent'">
          <span>说明</span>
          <input :value="'通过 SSH 代理认证'" disabled />
        </label>
        <label v-else>
          <span>连接密码</span>
          <input
            v-model="props.state.connectSecret.value"
            type="password"
            autocomplete="off"
            placeholder="输入 SSH 密码"
          />
        </label>
      </div>

      <div v-if="props.state.form.value.authType === 'password'" class="draft-secret-panel">
        <label class="checkbox-row">
          <input v-model="props.state.rememberSecret.value" type="checkbox" />
          <span>连接成功前，将密码保存到系统钥匙串</span>
        </label>
        <p class="empty-copy">
          现在可以直接在左侧输入密码，然后点击下方“{{ props.editorConnectLabel }}”。不需要先去中间区域输入。
        </p>
      </div>

      <div class="stack compact-stack">
        <label>
          <span>标签</span>
          <input v-model="props.state.form.value.tagsInput" placeholder="prod, ssh, eu-west" />
        </label>
        <label>
          <span>远程书签</span>
          <textarea v-model="props.state.form.value.remoteRootsInput" rows="4" placeholder="/home/root&#10;/var/www" />
        </label>
        <label>
          <span>本地书签</span>
          <textarea v-model="props.state.form.value.localRootsInput" rows="3" placeholder="/srv/app&#10;/var/log" />
        </label>
        <label>
          <span>备注</span>
          <textarea v-model="props.state.form.value.notes" rows="3" placeholder="跳板机、仅生产环境、密钥按月轮换" />
        </label>
      </div>

      <div class="actions actions--spread">
        <button class="ghost" :disabled="props.busy" @click="props.onSaveSession">
          仅保存
        </button>
        <button class="primary" :disabled="props.busy" @click="props.onSaveAndConnectSession">
          {{ props.editorConnectLabel }}
        </button>
      </div>
    </section>
  </aside>
</template>
