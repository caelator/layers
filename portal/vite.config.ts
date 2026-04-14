import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    proxy: {
      '/api': 'http://127.0.0.1:18791',
      '/ws': { target: 'ws://127.0.0.1:18791', ws: true },
      '/health': 'http://127.0.0.1:18791',
    },
  },
  build: {
    outDir: 'dist',
  },
})
