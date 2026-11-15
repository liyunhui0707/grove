import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: '../tests/website',
  // App UI tests clear their own test-results directory at startup.
  outputDir: '../.local/website-test-results',
  workers: 1,
  retries: 0,
  use: {
    channel: 'chrome',
    baseURL: 'http://127.0.0.1:1430',
    reducedMotion: 'reduce',
    trace: 'retain-on-failure',
  },
  webServer: {
    command:
      'node website/build.mjs && python3 -m http.server 1430 --bind 127.0.0.1 --directory website/dist',
    cwd: '..',
    url: 'http://127.0.0.1:1430',
    reuseExistingServer: false,
  },
});
