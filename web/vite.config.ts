import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Dev proxies /api + /auth to the local automl-server so `pnpm dev` works
// against a real backend out of the box (v1 had no proxy at all).
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/api': 'http://127.0.0.1:8030',
      '/auth': 'http://127.0.0.1:8030',
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
});
