import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  // App desktop carregado do disco: um bundle único é mais rápido que vários chunks.
  build: { target: 'es2022', chunkSizeWarningLimit: 2000 },
});
