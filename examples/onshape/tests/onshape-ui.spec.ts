import { test, expect } from '@playwright/test';

// Edit an extrude depth through the real Onshape web UI. Gated: runs only when a
// document URL is provided AND a session was captured via `npm run auth`
// (storageState in auth.json). Otherwise it skips — keeping `npm test` green
// without credentials.
//
// SELECTOR WARNING: Onshape is an Angular SPA with generated class names and a
// WebGL canvas (the 3D model is NOT in the DOM). The locators below target the
// feature tree + dialog (which ARE DOM) but are UNVERIFIED guesses — regenerate
// them against a live document with `npm run auth` / `playwright codegen`.
//
// Prefer the REST API (../onshape_edit.py) for the geometric edit; use this UI
// path only for edits the API can't express, or for visual verification.

const DOC = process.env.ONSHAPE_DOC_URL;
const FEATURE = process.env.ONSHAPE_FEATURE ?? 'Extrude 1';
const NEW_DEPTH = process.env.ONSHAPE_DEPTH ?? '50 mm';

test.skip(!DOC, 'set ONSHAPE_DOC_URL and capture auth.json (npm run auth) to run');

test('edit extrude depth via Onshape UI', async ({ page }) => {
  await page.goto(DOC!);

  // Canvas is WebGL — wait on a DOM anchor (the feature tree), not the model.
  const featureRow = page.getByText(FEATURE, { exact: true }); // GUESS
  await expect(featureRow).toBeVisible({ timeout: 30_000 });

  await featureRow.dblclick(); // GUESS — fallback: right-click -> "Edit"

  const depth = page.getByRole('textbox', { name: /depth|distance/i }); // GUESS
  await expect(depth).toBeVisible();
  await depth.fill(NEW_DEPTH); // expression input accepts "50 mm"

  // Green checkmark accepts and auto-commits to the workspace (no Save button).
  const confirm = page.getByRole('button', { name: /ok|confirm|accept/i }); // GUESS
  if (await confirm.isVisible().catch(() => false)) await confirm.click();
  else await page.keyboard.press('Enter'); // Enter accepts a feature dialog

  // Verify persistence: reopen and read the value back.
  await featureRow.dblclick();
  await expect(depth).toHaveValue(new RegExp(NEW_DEPTH.replace(/\s+/g, '\\s*'), 'i'));
  await page.keyboard.press('Escape');
});
