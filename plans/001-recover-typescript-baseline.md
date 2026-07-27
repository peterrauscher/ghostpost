# Plan 001: Recover a green, deterministic TypeScript baseline for `app/`

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 3625ed1..HEAD -- app/package.json app/package-lock.json app/tsconfig.json app/eslint.config.js app/src app/app.json app/README.md`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: tech-debt
- **Planned at**: commit `3625ed1`, 2026-07-23

## Why this matters

Every later Expo cutover plan depends on a trustworthy TypeScript gate. At
`3625ed1`, `app/` has strict TypeScript enabled but no `typecheck` script, no
test runner, and a red `tsc --noEmit` baseline (252 diagnostics across 38 files,
exit 2). Without capturing that baseline and turning it green first,
architecture changes will mix new errors into an already-red tree and nobody
can tell regressions from pre-existing breakage. This plan only restores a
deterministic green baseline and CI-friendly scripts; it must not change product
behavior, API contracts, or runtime gates.

## Current state

### Relevant files

- `app/package.json` — Expo SDK 57 app; scripts are `start` / `android` /
  `ios` / `web` / `lint` / `export:web` only. No `typecheck`, no `test`.
  Uses npm (`package-lock.json` present).
- `app/package-lock.json` — lockfileVersion 3; authority for installs.
- `app/tsconfig.json` — extends `expo/tsconfig.base`, `strict: true`, path
  aliases `@/*` → `./src/*`.
- `app/eslint.config.js` — flat `eslint-config-expo`; ignores `dist/*` only.
- `app/src/domain/types.ts` — domain unions/DTOs used across the app
  (unchanged by this plan except if a type-only fix is required for green
  `tsc`; prefer not to touch).
- `app/src/services/api/http.ts` — uncredentialed HTTP adapter (behavior
  out of scope; do not redesign).
- `app/src/providers/app-state.tsx` — device-local gate state (behavior out
  of scope; do not redesign).
- `app/README.md` — documents `npm install`, `npm run lint`, and web export;
  omits typecheck/test.
- No first-party test files under `app/src/` (only transitive tests inside
  `node_modules`).
- No test-runner devDependencies in `app/package.json` (no Jest, Vitest, or
  similar at plan time).

### Observed package / script baseline (`app/package.json`)

```json
"devDependencies": {
  "@types/react": "~19.2.2",
  "eslint": "^9.39.5",
  "eslint-config-expo": "~57.0.0",
  "typescript": "~6.0.3"
},
"scripts": {
  "start": "expo start",
  "android": "expo start --android",
  "ios": "expo start --ios",
  "web": "expo start --web",
  "lint": "expo lint",
  "export:web": "expo export --platform web"
}
```

Pinned runtime versions at plan time include `expo ~57.0.7`,
`react 19.2.3`, `react-native 0.86.0`, `expo-router ~57.0.7`,
`typescript ~6.0.3` (resolved `6.0.3`), `@types/react` resolved `19.2.17`.

### Observed TypeScript config (`app/tsconfig.json`)

```json
{
  "extends": "expo/tsconfig.base",
  "compilerOptions": {
    "strict": true,
    "paths": {
      "@/*": ["./src/*"],
      "@/assets/*": ["./assets/*"]
    }
  },
  "include": [
    "**/*.ts",
    "**/*.tsx",
    ".expo/types/**/*.ts",
    "expo-env.d.ts"
  ]
}
```

`expo/tsconfig.base` sets `jsx: "react-jsx"`, `moduleResolution: "bundler"`,
`skipLibCheck: true`, `noEmit: true`.

### Observed lint baseline

At plan writing, from `app/`:

```text
npm run lint
# exit 0
```

### Observed TypeScript diagnostics (must re-capture in Step 1)

**Source of truth**: `npx tsc --noEmit` from `app/` after `npm ci`. IDE/LSP
TypeScript diagnostics were **unavailable** at plan writing — do not rely on
editor error counts; use the npm/`tsc` output below.

At plan writing, from `app/`:

```text
npx tsc --noEmit
# exit 2
# 252 lines matching `error TS` (252 diagnostics)
# 38 unique file paths reporting errors
```

Error-code histogram (observed; re-capture full histogram in Step 1):

| Code | Count | Meaning |
|------|------:|---------|
| TS2786 | 114 | JSX component type not a valid element type |
| TS2607 | 113 | JSX element class lacks `props` |
| TS2305 | 13 | Module has no exported member (e.g. `expo-router` `Stack`, `Redirect`, `useRouter`) |
| TS7016 | 7 | Missing declaration file |
| TS2339 | 1 | Property does not exist on type |

Top five codes sum to 248 of 252 total diagnostics; Step 1 must capture any
remaining codes via `grep -oE 'error TS[0-9]+' …`.

*(Inference)* The dominant TS2786/TS2607 pair suggests React 19 / RN 0.86 JSX
typing mismatch rather than application logic bugs in most files.

Representative failures:

```text
src/app/_layout.tsx(1,10): error TS2305: Module '"expo-router"' has no exported member 'Stack'.
src/app/index.tsx(1,10): error TS2305: Module '"expo-router"' has no exported member 'Redirect'.
src/components/primitives/Screen.tsx(70,6): error TS2786: 'View' cannot be used as a JSX component.
src/components/primitives/Screen.tsx(70,5): error TS2607: JSX element class does not support attributes because it does not have a 'props' property.
```

*(Inference)* `Screen.tsx` itself is ordinary RN usage (not app logic bugs):

```tsx
// app/src/components/primitives/Screen.tsx:47-55
<View
  style={[
    styles.inner,
    padded && styles.padded,
    style,
  ]}
  {...props}>
  {children}
</View>
```

### Expo doctor (dependency drift, not the only TS cause)

```text
npx expo-doctor
# 19/20 checks passed; 1 failed
# Minor: react-native-screens expected ~4.26.0, found 4.25.2
# Patch: expo, expo-constants, expo-linking, expo-router,
#        expo-splash-screen, expo-web-browser behind SDK 57 expecteds
# Advice: npx expo install --check
```

### Domain / HTTP / gate excerpts (behavior must stay unchanged)

```ts
// app/src/domain/types.ts:1-36 — RiskLevel, PlatformId, ReviewAction, ScanPhase, AppGate
// app/src/domain/types.ts:101-105 — ScanStatus { phase, progress, message }
```

```ts
// app/src/services/api/http.ts:12-36 — getBaseUrl + request<T> (no auth, always JSON)
// app/src/services/api/http.ts:38-58 — httpApi routes: /me, /dashboard, /review, /posts/...
```

```ts
// app/src/providers/app-state.tsx:49-64 — initialState + deriveGate welcome→onboarding→scan→locked→app
// app/src/providers/app-state.tsx:137-141 — completeOnboarding / completeScan / unlockHome local booleans
```

### Conventions to honor

- Package manager: **npm** in `app/` (lockfile present). Do not introduce pnpm/yarn.
- Do not enable new runtime features, change routes, or alter mock/HTTP selection.
- Prefer Expo’s dependency installer (`npx expo install …`) over hand-pinned
  versions so SDK 57 peer ranges stay aligned.
- Keep `strict: true`. Do not “fix” the baseline by disabling strictness,
  enabling `skipLibCheck` tricks beyond what Expo already sets, or blanket
  `// @ts-nocheck`.
- No product/architecture work from later plans (auth, backend, cutover).

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Install (from `app/`) | `npm ci` | exit 0 |
| Capture baseline diagnostics | `npx tsc --noEmit 2>&1 \| tee /tmp/ghostpost-tsc-before.txt \| tail -5; echo EXIT:$?` | EXIT 2 at plan time; file contains many `error TS` lines |
| Count diagnostics | `grep -c 'error TS' /tmp/ghostpost-tsc-before.txt` | integer ≫ 0 (plan-time: 252) |
| Count affected files | `grep 'error TS' /tmp/ghostpost-tsc-before.txt \| cut -d'(' -f1 \| sort -u \| wc -l` | integer ≫ 0 (plan-time: 38) |
| Expo dependency audit | `npx expo-doctor` | prints check summary; may show mismatches before fix |
| Expo align deps | `npx expo install --fix` then `npx expo install typescript@…` only if doctor/install guides it | exit 0; `package-lock.json` updated |
| Lint | `npm run lint` | exit 0 |
| Typecheck (after script added) | `npm run typecheck` | exit 0, zero `error TS` |
| Typecheck twice (determinism) | `npm run typecheck && npm run typecheck` | both exit 0 |
| Unit tests (after harness added) | `npm test` | exit 0; at least the new smoke test(s) pass |
| Web export smoke | `npm run export:web` | exit 0; `app/dist/` (or Expo export out dir) produced |
| Behavior diff guard | `git diff --stat 3625ed1 -- app/src/services/api app/src/providers app/src/domain app/src/features app/src/app app/src/mocks` | empty **or** only pure type-annotation / import-type fixes with no runtime logic change — see Step 4 |

All commands run with cwd `app/` unless noted.

## Suggested executor toolkit

- If available, use Expo docs for SDK 57 TypeScript setup and
  `npx expo install` behavior rather than guessing package versions.
- Treat `npx tsc --noEmit` (npm script once added) as the TypeScript gate;
  LSP/editor diagnostics are not available in this environment.
- Do **not** run repo-wide formatters or unrelated monorepo gates.

## Scope

**In scope** (the only paths you should modify):

- `app/package.json` — add `typecheck` and `test` scripts; dependency version
  alignment via Expo tooling; add a minimal test runner devDependency if
  missing.
- `app/package-lock.json` — refresh via npm/expo install only.
- `app/tsconfig.json` — only if required for correct project references /
  types inclusion after Expo alignment (keep `strict: true`).
- `app/eslint.config.js` — only if the new test files need an ignore or if
  Expo’s lint flat-config requires a one-line test glob; do not weaken rules
  globally.
- First-party TypeScript/TSX under `app/src/**` **only** as required to clear
  pre-existing diagnostics, with **no intentional runtime/behavior change**.
- New test harness files chosen in Step 5 based on install success (no runner
  preselected at plan time), for example:
  - `app/jest.config.js` or `app/vitest.config.ts` (whichever install path works)
  - `app/src/domain/types.smoke.test.ts` (or equivalent)
- `app/README.md` — document `typecheck` and `test` scripts.
- Capture artifact (committed): `app/scripts/tsc-baseline-3625ed1.txt` **or**
  `plans/artifacts/001-tsc-baseline-3625ed1.txt` if `app/scripts/` is awkward;
  prefer `app/scripts/tsc-baseline-3625ed1.txt` next to existing
  `app/scripts/reset-project.js`.

**Out of scope** (do NOT touch, even if related):

- Any backend (`backend/`), Docker, SST, or infrastructure files.
- Auth, archive upload, scan polling, entitlement, or mock-removal product work
  (Plans 003–007).
- Changing `GhostpostApi`, HTTP paths, AsyncStorage gate semantics, fixtures
  content, or UI copy.
- Removing TanStack query persistence packages (deferred to frontend cutover).
- Renaming `ReviewAction` / adding `delete_local` (later plan).
- Enabling billing, WorkOS, or `EXPO_PUBLIC_*` new env vars.
- Committing secrets; rewriting git history; pushing.

## Git workflow

- Branch: `advisor/001-recover-typescript-baseline` (or
  `plan/001-recover-typescript-baseline` if the operator prefers).
- Commits: small logical units, e.g.
  1. `chore(app): capture tsc baseline at 3625ed1`
  2. `chore(app): align expo sdk dependencies and add typecheck script`
  3. `fix(app): clear pre-existing typescript diagnostics`
  4. `chore(app): add smoke test runner`
- Match short imperative subjects seen in history (`Add pricing paywall…`,
  `Update gitignore…`).
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 0: Drift check

```bash
cd /path/to/ghostpost
git rev-parse --short HEAD   # note SHA
git diff --stat 3625ed1..HEAD -- app/package.json app/package-lock.json app/tsconfig.json app/eslint.config.js app/src app/app.json app/README.md
```

**Verify**: If the diff is non-empty, re-read changed files and compare to
excerpts above. Material behavior or dependency changes → STOP.

### Step 1: Capture the live baseline before any fix

```bash
cd app
npm ci
set -o pipefail
npx tsc --noEmit 2>&1 | tee scripts/tsc-baseline-3625ed1.txt
status=${PIPESTATUS[0]}
echo EXIT:$status
grep -c 'error TS' scripts/tsc-baseline-3625ed1.txt || true
grep -oE 'error TS[0-9]+' scripts/tsc-baseline-3625ed1.txt | sort | uniq -c | sort -rn | head -20 || true
```

Create `app/scripts/` entries if needed and prepend a short header recording the
commit, command, exit status, diagnostic count, and any drift from the planning
evidence.

**Verify**:

- Preserve the command's actual exit status; do not infer it through a trailing
  pipeline stage.
- The plan-time observation was 252 diagnostics across 38 files with exit 2.
  The execution-time clean install at the same SHA produced zero diagnostics and
  exit 0; retain that honest drift note rather than fabricating a red capture.
- Baseline evidence is non-empty and retained with the implementation.

Do **not** fix anything in this step.

### Step 2: Add deterministic `typecheck` script (still red is OK)

Edit `app/package.json` `scripts`:

```json
"typecheck": "tsc --noEmit",
"lint": "expo lint",
"export:web": "expo export --platform web"
```

Keep existing scripts. Do not change runtime code yet.

**Verify**:

```bash
cd app
npm run typecheck ; echo EXIT:$?
```

→ exit 2 (non-zero), same class of errors as Step 1 (count within small noise of
252 diagnostics / 38 files). Script must be invocable as `npm run typecheck`.

### Step 3: Align dependencies with Expo — do not guess versions

```bash
cd app
npx expo-doctor
npx expo install --fix
# If typescript / @types/react still disagree with RN JSX after check:
npx expo install --check
```

Rules:

1. Prefer `npx expo install <pkg>` / `npx expo install --fix` for any Expo
   or React Native ecosystem package.
2. Never hand-edit versions to arbitrary numbers from memory.
3. If `typescript` 6.x is rejected by Expo peer ranges or is the root cause of
   TS2786/TS2607 against `@types/react` + `react-native`, use
   `npx expo install typescript` (or the exact version Expo prints) rather than
   inventing a pin. Record the before/after versions in the commit message.
4. After alignment: `npm ci` (or `npm install` if lockfile rewrite requires it)
   so `package-lock.json` matches.

Common successful outcome for this class of errors on Expo 57:

- Expo packages match `expo-doctor` expected ranges (including
  `react-native-screens`, `expo-router`, etc.).
- `typescript` and `@types/react` are the Expo-supported pair for React 19 /
  RN 0.86 JSX.
- `expo-router` types again export `Stack`, `Redirect`, `useRouter`.

**Verify**:

```bash
cd app
npx expo-doctor
# expect 20/20 checks passed, or only explicitly accepted advisories documented in STOP
npm ls typescript @types/react expo expo-router react react-native --depth=0
```

If `expo install --fix` wants to change `react` / `react-native` majors
beyond SDK 57, STOP.

### Step 4: Clear remaining first-party diagnostics without behavior changes

Re-run:

```bash
cd app
npm run typecheck 2>&1 | tee /tmp/ghostpost-tsc-mid.txt
grep -c 'error TS' /tmp/ghostpost-tsc-mid.txt || true
```

Fix **only** what remains, in this order of preference:

1. Dependency/types alignment (Step 3) — preferred root fix for TS2786/TS2607
   and expo-router TS2305.
2. Local type-only annotations (`type` imports, explicit callback parameter
   types for TS7031, narrowing) with identical emitted JS.
3. Tiny declaration shims under `app/src/types/` **only** if a library ships
   broken types and Expo alignment cannot fix it — document why in the commit.
4. Last resort: minimal code edit that preserves runtime behavior (e.g. split
   a malformed generic). Prefer not to touch UI components if deps fix JSX.

Forbidden “fixes”:

- `// @ts-expect-error` / `@ts-ignore` sprawl across the tree.
- `strict: false`, `noImplicitAny: false`, excluding `src/**` from `include`.
- Casting everything through `as any`.
- Deleting screens or disabling routes to silence errors.
- Changing mock API behavior, gate booleans, or copy.

**Verify after fixes**:

```bash
cd app
npm run typecheck
# exit 0, no error TS lines

npm run typecheck
# exit 0 again (deterministic)

npm run lint
# exit 0

# Behavior guard: no intentional runtime edits
git diff 3625ed1 -- app/src/services/api/http.ts app/src/services/api/mock.ts app/src/providers/app-state.tsx app/src/domain/types.ts
# Prefer empty. If non-empty, the diff must be type-only (imports type, annotations).
# Manually confirm no string/route/async logic changes.
```

If a fix appears to require redesigning `app-state`, HTTP auth, or domain wire
unions for product reasons → STOP (belongs to later plans).

### Step 5: Add a minimal deterministic test harness (no product coverage required)

Goal: `npm test` exists and runs at least one fast smoke test so later plans
have a place to hang unit tests. Do **not** build a full RN Testing Library
suite here.

**Runner choice (no repo default yet)**: `app/package.json` has no `test` script
and no Jest/Vitest devDependencies at plan time. Pick **one** minimal runner
based on what installs cleanly with Expo SDK 57 after you try the commands below
— do not assume Jest or Vitest upfront. Document the chosen runner, install
command, and rationale in the commit message.

1. Try Expo-aligned Jest:

```bash
cd app
npx expo install jest-expo jest @types/jest -- -D
```

If install succeeds, add to `app/package.json`:

```json
"scripts": {
  "test": "jest"
}
```

and `app/jest.config.js`:

```js
module.exports = {
  preset: 'jest-expo',
  testMatch: ['**/*.test.ts', '**/*.test.tsx'],
  moduleNameMapper: {
    '^@/(.*)$': '<rootDir>/src/$1',
  },
};
```

2. If Jest install fails or has unresolved peer conflicts, try Vitest for pure
   TS (node environment):

```bash
cd app
npm install -D vitest
```

If Vitest is chosen: `"test": "vitest run"` in `package.json`, plus a
`app/vitest.config.ts` with `@/` alias matching tsconfig.

Use whichever path completes `npm ci` and runs the smoke test below with exit 0.
If both paths fail after one focused attempt each → STOP.

**Smoke test** `app/src/domain/types.smoke.test.ts`:

```ts
import {
  COMING_UP_OPTIONS,
  CONCERN_OPTIONS,
  PLATFORM_OPTIONS,
} from '@/domain/types';

describe('domain option catalogs', () => {
  it('exposes stable coming-up ids', () => {
    expect(COMING_UP_OPTIONS.map((o) => o.id)).toEqual(
      expect.arrayContaining(['rush', 'college_apps', 'job_interviews']),
    );
  });

  it('exposes five platform preferences', () => {
    expect(PLATFORM_OPTIONS).toHaveLength(5);
    expect(PLATFORM_OPTIONS.map((p) => p.id).sort()).toEqual(
      ['facebook', 'instagram', 'reddit', 'tiktok', 'x'].sort(),
    );
  });

  it('exposes concern ids used by onboarding', () => {
    expect(CONCERN_OPTIONS.some((c) => c.id === 'public_image')).toBe(true);
  });
});
```

This test locks catalog constants without exercising UI or network.

**Verify**:

```bash
cd app
npm test
# exit 0; ≥3 assertions pass

npm run typecheck
# still exit 0
```

### Step 6: Document scripts and prove lint / typecheck / web-export

Update `app/README.md` Scripts section to:

```bash
npm start
npm run lint
npm run typecheck
npm test
npm run export:web
```

Then run the full proof suite from a clean install mindset:

```bash
cd app
npm ci
npm run lint
npm run typecheck
npm test
npm run export:web
```

**Verify**:

| Command | Expected |
|---------|----------|
| `npm run lint` | exit 0 |
| `npm run typecheck` | exit 0; no `error TS` |
| `npm test` | exit 0; smoke tests pass |
| `npm run export:web` | exit 0; export output directory created (typically `dist/` under `app/`) |
| second `npm run typecheck` | exit 0 |

Save a short proof note in the final commit message listing the four commands
and exit codes. Do not invent CI YAML unless already present (none expected).

### Step 7: Final scope hygiene

```bash
cd /path/to/ghostpost
git status
git diff --stat
```

**Verify**:

- Only in-scope files changed.
- `scripts/tsc-baseline-3625ed1.txt` (or chosen baseline path) is tracked.
- No `backend/`, no Docker, no SST, no plan files other than optional README
  status update.
- `app/src/services/api/http.ts` route list still matches pre-plan paths
  (`/me`, `/dashboard`, `/review`, `/scan`, `/home/unlock`, `/demo/reset`, …).
- `deriveGate` still uses local booleans (no server lifecycle yet).

## Test plan

- **New**: `app/src/domain/types.smoke.test.ts` (or equivalent) covering:
  - coming-up catalog contains expected ids
  - five platform preference ids
  - concern catalog includes `public_image`
- **Pattern**: pure unit test of constants; no renderer required.
- **Verification**: `cd app && npm test` → exit 0.
- **Regression proof**: baseline file preserved; `npm run typecheck` green twice.
- Do not add E2E or Detox in this plan.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `app/scripts/tsc-baseline-3625ed1.txt` exists and honestly records the
      execution-time pre-fix result plus the plan-time 252-diagnostic drift note;
      no historical output is fabricated.
- [ ] `app/package.json` defines `"typecheck": "tsc --noEmit"` and `"test": …`.
- [ ] Dependencies aligned via Expo tooling; `npx expo-doctor` is clean or only
      has operator-accepted advisories recorded in the commit message.
- [ ] `cd app && npm ci && npm run lint` exits 0.
- [ ] `cd app && npm run typecheck` exits 0 twice in a row with zero `error TS`.
- [ ] `cd app && npm test` exits 0 with new smoke test(s) passing.
- [ ] `cd app && npm run export:web` exits 0.
- [ ] No intentional runtime/behavior changes to API adapters, app gate,
      fixtures, or screens (type-only diffs only if unavoidable).
- [ ] No files outside the in-scope list are modified (`git status`).
- [ ] `plans/README.md` status row for 001 updated (unless reviewer owns index).

## STOP conditions

Stop and report back (do not improvise) if:

- Drift check shows in-scope files already changed such that excerpts no longer
  match and the fix path is ambiguous.
- `npm ci` fails on the locked dependencies and cannot be repaired without
  unrelated major upgrades.
- `npx expo install --fix` demands a React Native or Expo **major** jump away
  from SDK 57, or removes `package-lock.json` / forces a different package
  manager.
- Clearing TS2786/TS2607 appears to require rewriting application components
  rather than fixing TypeScript / `@types/react` / Expo package alignment —
  after one focused dependency attempt and one minimal annotation attempt both
  fail twice.
- `expo-router` still has no type exports for `Stack` / `Redirect` / `useRouter`
  after dependency alignment; do not reimplement navigation to silence errors.
- A step’s verification fails twice after a reasonable fix attempt.
- The fix appears to require touching out-of-scope backend/infra or product
  cutover behavior (auth, unlock removal, mock deletion, etc.).
- `npm run export:web` fails for reasons unrelated to TypeScript (native module
  crash, missing assets) that cannot be fixed without broad product changes —
  report the export error verbatim.
- You are asked to claim a green baseline without the committed red baseline
  artifact — refuse; capturing the red state is part of done.

## Maintenance notes

- Later plans (002+) must treat `npm run typecheck` as a hard gate for any
  `app/` TypeScript edits. Do not land features while typecheck is red.
- Keep the baseline artifact for archaeology; do not “update” it to green.
  Optional: add `app/scripts/tsc-baseline-after-001.txt` green snapshot, but
  never overwrite the red capture.
- When upgrading Expo SDK next time, rerun `npx expo install --fix` and
  `npm run typecheck` before feature work.
- Reviewers should scrutinize: (1) any non-lockfile diff under `app/src/app` or
  providers for accidental behavior changes, (2) whether versions came from
  Expo install vs hand-editing, (3) that `strict` remained true.
- Deferred: product type changes (`ScanStatus.id/status`, `delete_local`,
  optional engagement fields), removal of query-persist packages, and real API
  auth headers — all later plans.
