// Stockage sur l'appareil (IndexedDB) : bibliothèque, ROM, sauvegardes batterie, états.
// Rien n'est jamais envoyé ailleurs. Une sauvegarde écrasée garde la précédente (`ancienne`).

const NOM = "gbemu";
const VERSION = 1;
let base = null;

function ouvrir() {
  if (base) return base;
  base = new Promise((ok, ko) => {
    const r = indexedDB.open(NOM, VERSION);
    r.onupgradeneeded = () => {
      const db = r.result;
      // jeux : { id, nom, titre, console, taille, ajoute, joue }
      db.createObjectStore("jeux", { keyPath: "id" });
      // roms : octets de la ROM, clé = id du jeu
      db.createObjectStore("roms");
      // sauvegardes : { id, data, date, ancienne }
      db.createObjectStore("sauvegardes", { keyPath: "id" });
      // etats : { id, data, date }
      db.createObjectStore("etats", { keyPath: "id" });
    };
    r.onsuccess = () => ok(r.result);
    r.onerror = () => ko(r.error);
  });
  return base;
}

// Exécute `f(magasins)` dans une transaction et attend sa fin (écriture durable).
async function transaction(noms, mode, f) {
  const db = await ouvrir();
  return new Promise((ok, ko) => {
    const t = db.transaction(noms, mode, { durability: "strict" });
    const m = Object.fromEntries(noms.map((n) => [n, t.objectStore(n)]));
    let resultat;
    Promise.resolve(f(m)).then((v) => (resultat = v));
    t.oncomplete = () => ok(resultat);
    t.onerror = () => ko(t.error);
    t.onabort = () => ko(t.error || new Error("transaction annulée"));
  });
}

function requete(r) {
  return new Promise((ok, ko) => {
    r.onsuccess = () => ok(r.result);
    r.onerror = () => ko(r.error);
  });
}

export const listerJeux = () =>
  transaction(["jeux"], "readonly", (m) => requete(m.jeux.getAll()));

export const lireJeu = (id) =>
  transaction(["jeux"], "readonly", (m) => requete(m.jeux.get(id)));

export async function ajouterJeu(jeu, octets) {
  return transaction(["jeux", "roms"], "readwrite", (m) => {
    m.jeux.put(jeu);
    m.roms.put(octets, jeu.id);
  });
}

export const majJeu = (jeu) =>
  transaction(["jeux"], "readwrite", (m) => {
    m.jeux.put(jeu);
  });

// La sauvegarde batterie et l'état sont gardés : réajouter la ROM les retrouve.
export const supprimerJeu = (id) =>
  transaction(["jeux", "roms"], "readwrite", (m) => {
    m.jeux.delete(id);
    m.roms.delete(id);
  });

export const lireRom = (id) =>
  transaction(["roms"], "readonly", (m) => requete(m.roms.get(id)));

export const lireSauvegarde = (id) =>
  transaction(["sauvegardes"], "readonly", (m) => requete(m.sauvegardes.get(id)));

export const listerSauvegardes = () =>
  transaction(["sauvegardes"], "readonly", (m) => requete(m.sauvegardes.getAll()));

export const ecrireSauvegarde = (id, data) =>
  transaction(["sauvegardes"], "readwrite", async (m) => {
    const avant = await requete(m.sauvegardes.get(id));
    m.sauvegardes.put({ id, data, date: Date.now(), ancienne: avant ? avant.data : null });
  });

export const lireEtat = (id) =>
  transaction(["etats"], "readonly", (m) => requete(m.etats.get(id)));

export const ecrireEtat = (id, data) =>
  transaction(["etats"], "readwrite", (m) => {
    m.etats.put({ id, data, date: Date.now() });
  });

// Demande au navigateur de ne pas effacer les données quand l'espace manque.
export function demanderPersistance() {
  if (navigator.storage && navigator.storage.persist) navigator.storage.persist().catch(() => {});
}
