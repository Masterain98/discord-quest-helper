import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'path'

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [vue()],
  
  // Vite options tailored for Tauri development
  clearScreen: false,
  
  // Tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Rust builds replace executables under `target` while Vite is running.
      // Watching those files on Windows can raise EBUSY and kill the dev server,
      // leaving the still-open Tauri window on a blank page.
      ignored: [
        '**/target/**',
        '**/src-tauri/**',
        '**/src-runner/**',
        '**/src-cdp-launcher/**',
        '**/crates/**',
      ]
    }
  },
  
  resolve: {
    alias: {
      '@': resolve(__dirname, 'src')
    }
  },
  
  // Env variables starting with VITE_ will be exposed to your frontend source code
  envPrefix: ['VITE_']
})
