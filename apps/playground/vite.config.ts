import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  base: '/playground/',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
  server: {
    proxy: {
      '/agents': 'http://localhost:8080',
      '/health': 'http://localhost:8080',
      '/playground/agents': 'http://localhost:8080',
    },
  },
});
