import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// 开发态：Vite 在 5173 提供 UI，API/WS 经代理转发到 Rust 服务（默认 8080）。
// 生产态：由 `oaip serve` 直接托管 `ui/dist`，同源无需代理。
export default defineConfig({
  plugins: [react()],
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://localhost:8080',
        changeOrigin: true,
        ws: true,
      },
    },
  },
})
