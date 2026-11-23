// Service worker : l'appli entière en cache, utilisable hors ligne. VERSION est remplacée par
// scripts/web.sh (empreinte des fichiers) : une nouvelle version vide l'ancien cache.
// Les ROM et sauvegardes du joueur ne passent jamais par ici (IndexedDB) ; seuls les 2 jeux
// libres servis par le site (jeux-libres/) sont en cache, pour les ajouter hors ligne.
// L'appli Windows (telecharger/) passe toujours par le réseau, jamais par le cache.

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
  "icones/icone-192.png",
  "icones/icone-512.png",
  "icones/icone-masquable-512.png",
  "icones/apple-touch-icon.png",
  "jeux-libres/libbet.gb",
  "jeux-libres/ucity.gbc",
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

// Version demandée par la page (Réglages > Mise à jour), réponse sur le port fourni.
self.addEventListener("message", (e) => {
  if (e.data === "version" && e.ports[0]) e.ports[0].postMessage(VERSION);
});

// Cache d'abord (hors ligne), réseau en secours.
self.addEventListener("fetch", (e) => {
  const url = new URL(e.request.url);
  if (e.request.method !== "GET" || url.origin !== location.origin) return;
  if (url.pathname.includes("/telecharger/")) return;
  e.respondWith(
    caches.match(e.request, { ignoreSearch: true }).then((r) => r || fetch(e.request)),
  );
});
