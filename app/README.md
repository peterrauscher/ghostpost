# Ghostpost

Expo SDK 57 app for cleaning up social media posts before college, rush, or job applications.

## Stack

- Expo Router (file-based navigation + protected route gates)
- Ghostpost design system components (from Pencil)
- TanStack Query + AsyncStorage persistence for server-state hydration
- Mock API adapter by default (`EXPO_PUBLIC_API_URL` switches to HTTP)

## Getting started

```bash
npm install
npx expo start
```

Press `i` / `a` / `w` for iOS, Android, or web.

## App flow

1. Welcome carousel
2. Onboarding (coming up → concerns → platforms → how it helps)
3. Scan progress
4. Locked home → unlock
5. Home / Scan / Profile tabs
6. Review flagged content → flag detail

## Data layer

- Domain types: `src/domain`
- Mock fixtures: `src/mocks`
- API adapters: `src/services/api` (`mock` by default, `http` when `EXPO_PUBLIC_API_URL` is set)
- Feature hooks: `src/features/hooks.ts`
- App gate persistence: `src/providers/app-state.tsx`
- Query cache persistence: `src/providers/query-provider.tsx`

## Scripts

```bash
npm start
npm run lint
npm run typecheck
npm test
npm run export:web
```
