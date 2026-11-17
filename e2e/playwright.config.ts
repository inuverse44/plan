// UI tests: headless Chromium against `plan serve` on a fresh SQLite with fixture epics (see serve.sh).
// "Today" is fixed in each test (page.clock), so nothing depends on the real clock.
import { defineConfig } from '@playwright/test';

const port = Number(process.env.PORT_E2E || 47391);

export default defineConfig({
  testDir: './tests',
  // One worker: the cases share one server, and a change anywhere makes open pages reload
  workers: 1,
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : 'list',
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    timezoneId: 'Asia/Tokyo',
    locale: 'en-US',
    viewport: { width: 1280, height: 800 },
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  webServer: {
    command: './serve.sh',
    url: `http://127.0.0.1:${port}/api/version`,
    reuseExistingServer: false,
    env: { PORT_E2E: String(port) },
    stdout: 'ignore',
  },
});
