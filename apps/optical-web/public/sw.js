const CACHE_NAME = "drpa-optical-v3";
const APP_SHELL = new URL("./", self.registration.scope).href;

self.addEventListener("install", (event) => {
  event.waitUntil((async () => {
    const cache = await caches.open(CACHE_NAME);
    const response = await fetch(APP_SHELL, { cache: "no-cache" });
    const html = await response.clone().text();
    await cache.put(APP_SHELL, response);
    const assets = [...html.matchAll(/(?:src|href)="([^"]+)"/g)]
      .map((match) => new URL(match[1], APP_SHELL).href)
      .filter((url) => new URL(url).origin === self.location.origin);
    const manifestUrl = new URL("precache-manifest.json", APP_SHELL).href;
    const manifest = await fetch(manifestUrl, { cache: "no-cache" }).then((result) => result.json());
    const generated = Array.isArray(manifest.files)
      ? manifest.files.map((file) => new URL(file, APP_SHELL).href)
      : [];
    await cache.addAll([...new Set([...assets, manifestUrl, ...generated])]);
  })());
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(caches.keys().then((keys) => Promise.all(keys.filter((key) => key !== CACHE_NAME).map((key) => caches.delete(key)))));
  self.clients.claim();
});

self.addEventListener("fetch", (event) => {
  if (event.request.method !== "GET") return;
  const url = new URL(event.request.url);
  if (url.origin !== self.location.origin) return;
  event.respondWith(fetch(event.request).then((response) => {
    if (response.ok) {
      const copy = response.clone();
      void caches.open(CACHE_NAME).then((cache) => cache.put(event.request, copy));
    }
    return response;
  }).catch(async () => {
    const cached = await caches.match(event.request);
    if (cached) return cached;
    if (event.request.mode === "navigate") return caches.match(APP_SHELL);
    return Response.error();
  }));
});
