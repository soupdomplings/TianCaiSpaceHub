import { defineConfig } from 'vitest/config';

export default defineConfig({
  base: '/remote/',
  build: {
    outDir: '../assets/remote-web',
    emptyOutDir: true,
    sourcemap: false,
  },
  test: {
    environment: 'jsdom',
    include: ['tests/**/*.test.{ts,tsx}'],
    restoreMocks: true,
    clearMocks: true,
  },
});
