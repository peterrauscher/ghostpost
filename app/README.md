# Ghostpost Expo app

Production Expo SDK 57 client for the Ghostpost Axum API. There is no mock or fixture fallback.

## Required environment

```bash
EXPO_PUBLIC_API_URL=http://localhost:8080
```

The app fails at API-module initialization when this variable is missing. For local web auth, keep the frontend callback and API on the same hostname (`localhost`, not a mix of `localhost` and `127.0.0.1`) so the pre-auth and session cookies remain consistent.

Android emulator API URL:

```bash
EXPO_PUBLIC_API_URL=http://10.0.2.2:8080
```

## Run

```bash
npm ci
EXPO_PUBLIC_API_URL=http://localhost:8080 npm run web
```

## Authentication

- Web uses the backend-managed `gp_auth_init` and `gp_session` HttpOnly cookies, plus `/v1/auth/csrf` for mutations.
- Native receives a server-issued one-time `exchangeSecret` from `GET /v1/auth/authorize?client=native`, stores that pending flow in SecureStore, and exchanges it once at `ghostpost://auth/callback`.
- Native stores only the returned Ghostpost `session.token`; WorkOS tokens never reach the app.

## Archive uploads

The document picker supplies an opaque ZIP `File`. The app reserves a signed multipart POST with the API, uploads the file directly to S3/MinIO using `expo/fetch`, and asks the backend to complete and pin the immutable object version. The ZIP is never base64-encoded, hashed, or proxied through the API process.

## Gates

```bash
npm run typecheck
npm test
EXPO_PUBLIC_API_URL=http://localhost:8080 npm run export:web
```
