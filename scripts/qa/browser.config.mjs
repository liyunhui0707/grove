// Copied with the existing locked project test libraries into an isolated QA bundle.
import { defineConfig } from '@playwright/test';
const baseURL = process.env.MG_QA_URL;
if (!baseURL || new URL(baseURL).hostname !== '127.0.0.1')
  throw new Error('A private loopback test server is required');
export default defineConfig({
  testDir: './tests',
  workers: 1,
  fullyParallel: false,
  retries: 0,
  timeout: 30000,
  reporter: [['json', { outputFile: '../results/playwright.json' }], ['line']],
  outputDir: '../results/artifacts',
  use: {
    browserName: 'chromium',
    launchOptions: { executablePath: process.env.MG_QA_CHROME },
    headless: true,
    baseURL,
    viewport: { width: 1440, height: 960 },
    reducedMotion: 'reduce',
    trace: 'retain-on-failure',
  },
});
