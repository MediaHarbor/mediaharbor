import { defineConfig, configDefaults } from 'vitest/config';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    tsconfigPaths: true,
  },
  base: './',
  build: {
    outDir: 'dist-react',
    emptyOutDir: true,
    chunkSizeWarningLimit: 600,
    rolldownOptions: {
      output: {
        manualChunks(id) {
          if (id.includes('node_modules/framer-motion') || id.includes('node_modules/@radix-ui')) {
            return 'vendor-ui';
          }
          if (id.includes('node_modules/lucide-react')) {
            return 'vendor-icons';
          }
          if (id.includes('node_modules/@tanstack') || id.includes('node_modules/zustand')) {
            return 'vendor-query';
          }
          if (
            id.includes('node_modules/react/') ||
            id.includes('node_modules/react-dom/') ||
            id.includes('node_modules/react-router-dom/') ||
            id.includes('node_modules/@remix-run/')
          ) {
            return 'vendor-react';
          }
        },
      },
    },
  },
  server: {
    port: 5173,
    strictPort: true,
    host: 'localhost',
    watch: {
      ignored: [
        '**/.flatpak/**',
        '**/target/**',
        '**/dist-react/**',
        '**/_build/**',
        '**/gamdl/**',
        '**/votify/**',
        '**/src-tauri/target/**',
      ],
      followSymlinks: false,
    },
    fs: {
      strict: false,
    },
  },
  clearScreen: false,
  envPrefix: ['VITE_', 'TAURI_'],
  test: {
    include: ['src/**/*.{test,spec}.?(c|m)[jt]s?(x)'],
    exclude: [
      ...configDefaults.exclude,
      '**/.flatpak/**',
      '**/target/**',
      '**/dist-react/**',
      '**/_build/**',
      '**/gamdl/**',
      '**/votify/**',
      '**/src-tauri/target/**',
    ],
    passWithNoTests: true,
  },
});
