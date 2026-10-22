import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// During development (`npm run dev`) the page comes from Vite and the API
// from the laptimer server (`laptimer serve`, port 5800).
export default defineConfig({
  plugins: [react()],
  server: {
    host: true, // also reachable from phones in the network
    proxy: {
      '/api': 'http://localhost:5800',
      '/ws': { target: 'ws://localhost:5800', ws: true },
    },
  },
})
