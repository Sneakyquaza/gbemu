// Service worker : l'appli entière en cache, utilisable hors ligne. VERSION est remplacée par
// scripts/web.sh (empreinte des fichiers) : une nouvelle version vide l'ancien cache.
// Les ROM et sauvegardes ne passent jamais par ici (IndexedDB).

const VERSION = "dev";
const CACHE = "gbemu-" + VERSION;
const FICHIERS = [
  "./",
  "index.html",
  "manifest.webmanifest",
  "css/app.css",
  "js/app.js",
  "js/audio.js",
  "js/commandes.js",
  "js/son-worklet.js",
  "js/stockage.js",
  "js/zip.js",
  "pkg/gb_web.js",
  "pkg/gb_web_bg.wasm",
  "fonts/Rubik-Regular.ttf",
  "fonts/Rubik-Medium.ttf",
  "fonts/Rubik-Bold.ttf",
  "icons/icone-192.png",
  "icons/icone-512.png",
  "icons/icone-masquable-512.png",
  "icons/apple-touch-icon.png",
];

self.addEventListener("install", (e) => {
  e.waitUntil(
    caches
      .open(CACHE)
      .then((c) => c.addAll(FICHIERS.map((f) => new Request(f, { cache: "reload" }))))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (e) => {
  e.waitUntil(
    caches
      .keys()
      .then((cles) => Promise.all(cles.filter((c) => c.startsWith("gbemu-") && c !== CACHE).map((c) => caches.delete(c))))
      .then(() => self.clients.claim()),
  );
});

// Cache d'abord (hors ligne), réseau en secours.
self.addEventListener("fetch", (e) => {
  if (e.request.method !== "GET" || new URL(e.request.url).origin !== location.origin) return;
  e.respondWith(
    caches.match(e.request, { ignoreSearch: true }).then((r) => r || fetch(e.request)),
  );
});
