import { test, expect } from '@playwright/test';
import { pathToFileURL } from 'node:url';
import path from 'node:path';

// Proves the edit-dialog mechanics the real Onshape spec relies on — double-click
// a feature row, fill the depth textbox, confirm with the green check, reopen and
// assert the value persisted — run fully headless against a local mock page.
// No Onshape account or storageState needed.
test.use({ storageState: { cookies: [], origins: [] } });

const MOCK = pathToFileURL(path.join(__dirname, 'mock-editor.html')).href;

test('edit dialog: fill depth, confirm, verify persistence', async ({ page }) => {
  await page.goto(MOCK);

  const featureRow = page.getByRole('treeitem', { name: 'Extrude 1' });
  await expect(featureRow).toBeVisible();

  await featureRow.dblclick();
  const depth = page.getByRole('textbox', { name: /depth/i });
  await expect(depth).toHaveValue('25 mm');

  await depth.fill('50 mm');
  await page.getByRole('button', { name: /ok/i }).click();

  // reopen — the persisted (committed) value should now read back as the edit
  await featureRow.dblclick();
  await expect(depth).toHaveValue('50 mm');
});
