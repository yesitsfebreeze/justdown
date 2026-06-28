# Editing an Onshape CAD document — runnable example

Two layered paths to change one parameter (an extrude depth) and confirm it
persisted. **Recommendation: REST API for the geometric edit; Playwright only
for auth, navigation, UI-only edits, and visual verification.**

The Onshape 3D model lives in a **WebGL canvas that is not in the DOM** — only
the feature tree and dialogs are. So UI selectors are brittle (Angular SPA,
generated class names), while the REST `update feature` call is deterministic,
transactional, CI-friendly, and never hits a 2FA/bot wall.

## 1. REST API — `onshape_edit.py` (primary)

HMAC-SHA256 signed, no SDK dependency (the official `onshape-client` is archived
since 2021). Finds the first `extrude` feature, sets its `depth` expression,
POSTs the full feature back wrapped in `BTFeatureDefinitionCall-1406`, then
re-GETs features + mass properties to confirm.

```bash
# Proof of run — no credentials (in-process mock of the features API):
python3 onshape_edit.py --self-test

# Live edit:
export ONSHAPE_ACCESS_KEY=...   # from dev-portal.onshape.com -> API keys
export ONSHAPE_SECRET_KEY=...   # shown once at creation
python3 onshape_edit.py "https://cad.onshape.com/documents/<did>/w/<wid>/e/<eid>" "50 mm"
```

The `<did>/w/<wid>/e/<eid>` triple is read straight from the document URL.
POSTs must target a **workspace** (`/w/`), never a version (`/v/`).

## 2. Endpoint conformance — `verify_endpoints.py` (the jd library is real)

The `.jd/library/dev/onshape/` knowledge tree documents every REST group. This
test proves each endpoint those jdfiles cite actually exists in Onshape's live
**OpenAPI spec** (`https://cad.onshape.com/api/openapi` — public, no auth), so the
documented surface can't silently drift from reality.

```bash
python3 verify_endpoints.py            # fetches the spec on first run, caches it
python3 verify_endpoints.py --refresh  # re-pull the spec, then check
```

It normalizes the `w|v|m` workspace/version/microversion coordinate to the spec's
`{wvm}` template and asserts 116/116 cited endpoints conform.

## 3. Playwright UI — `tests/` (fallback + verification)

Onshape enforces cookie-based sessions and (optionally) 2FA, which blocks
scripted headless login. The robust pattern is **capture the session once,
interactively, then reuse it**:

```bash
npm install
npx playwright install chromium          # --with-deps needs sudo; binary alone is enough

npm run auth                             # headed: solve password + 2FA + "remember device"
                                         # -> writes auth.json (cookies/localStorage)

# Mechanics proof — headless, no account (local mock dialog):
npm run test:mock

# Real UI edit (skips unless ONSHAPE_DOC_URL is set and auth.json exists):
ONSHAPE_DOC_URL="https://cad.onshape.com/documents/<did>/w/<wid>/e/<eid>" \
ONSHAPE_FEATURE="Extrude 1" ONSHAPE_DEPTH="50 mm" npm test
```

Accepting a feature dialog (green check / `Enter`) **auto-commits to the
workspace** — there is no per-edit Save button; Versions are manual snapshots.

> The selectors in `tests/onshape-ui.spec.ts` are **unverified guesses** —
> Onshape publishes no `data-test`/aria contract. Regenerate against a live
> document with `npm run auth` / `playwright codegen` before relying on them.

## Files

## Run everything

```bash
npm run verify   # self-test + endpoint conformance + headless mock UI test
```

## Files

| File | Role |
|------|------|
| `onshape_edit.py` | REST edit + verify; `--self-test` runs creds-free |
| `verify_endpoints.py` | asserts every jd-cited endpoint exists in the live OpenAPI spec |
| `tests/mock-editor.{html,spec.ts}` | headless proof of the dialog mechanics |
| `tests/onshape-ui.spec.ts` | real UI edit (gated on URL + auth.json) |
| `playwright.config.ts` | headless Chromium, storageState reuse |
| `captures/openapi.json` | cached OpenAPI spec (gitignored; auto-fetched) |
