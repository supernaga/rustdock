import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'node:path'

export default defineConfig({
  root: resolve(__dirname, 'frontend'),
  plugins: [vue()],
  server: {
    host: '0.0.0.0',
    port: 1420,
    strictPort: true
  },
  build: {
    outDir: resolve(__dirname, 'frontend/dist'),
    emptyOutDir: true,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes('node_modules')) {
            return
          }
          if (id.includes('/vue/') || id.includes('/@vue/')) {
            return 'vendor-vue'
          }
          if (
            id.includes('@tauri-apps/api') ||
            id.includes('@tauri-apps/plugin-dialog') ||
            id.includes('@tauri-apps/plugin-notification')
          ) {
            return 'vendor-tauri'
          }
          if (id.includes('/xterm') || id.includes('@xterm/')) {
            return 'vendor-terminal'
          }
        }
      }
    }
  }
})
