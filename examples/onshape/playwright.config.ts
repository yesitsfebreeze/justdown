import { defineConfig, devices } from '@playwright/test';

// Headless Chromium for WSL. The real Onshape spec reuses a session captured
// once via `npm run auth` (storageState in auth.json); the mock spec needs none.
export default defineConfig({
  testDir: './tests',
  fullyParallel: true,
  reporter: [['list']],
  use: {
    storageState: process.env.ONSHAPE_AUTH ?? 'auth.json',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    launchOptions: { args: ['--disable-blink-features=AutomationControlled'] },
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
  ],
});
