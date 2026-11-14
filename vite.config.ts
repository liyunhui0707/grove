import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  envDir: false,
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: { host: '127.0.0.1', port: 1420, strictPort: true },
  build: { outDir: 'dist' },
  test: { include: ['apps/desktop/src/**/*.test.ts'], environment: 'node' },
});
