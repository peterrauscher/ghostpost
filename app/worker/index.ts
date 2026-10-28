// Serves the Expo static web export (app/dist, post-processed by
// scripts/prepare-web-dist.mjs). Static routes and the 404 page are plain
// assets; this Worker only runs for dynamic routes, which Expo exports as one
// HTML shell per route that the client router fills in from the URL.
interface Env {
  ASSETS: { fetch(request: Request): Promise<Response> };
}

// Keep in lockstep with DYNAMIC in scripts/prepare-web-dist.mjs.
const DYNAMIC: Array<[RegExp, string]> = [[/^\/review\/[^/]+\/?$/, "/_dynamic/review-id"]];

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const shell = DYNAMIC.find(([pattern]) => pattern.test(url.pathname))?.[1];
    return env.ASSETS.fetch(shell ? new Request(new URL(shell, url), request) : request);
  },
};
