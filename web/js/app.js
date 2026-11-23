// gbemu web : bibliothèque (IndexedDB), partie (cadence, écran, son, commandes), menus.

import init, { Emu, analyser_rom, ouvrir_zip } from "../pkg/gb_web.js";
import * as S from "./stockage.js";
import * as son from "./audio.js";
import { Commandes } from "./commandes.js";
import { zipper } from "./zip.js";

const $ = (id) => document.getElementById(id);
const CONSOLES = { gb: "Game Boy", gbc: "Game Boy Color", gba: "Game Boy Advance" };
const COURT = { gb: "GB", gbc: "GBC", gba: "GBA" };

let wasm = null;
// Après un piège WebAssembly, la mémoire du module n'est plus sûre : rechargement au prochain jeu.
let moduleCasse = false;
const pret = init().then((w) => (wasm = w));

const ios = /iPad|iPhone|iPod/.test(navigator.userAgent) ||
  (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);
const installe = () =>
  matchMedia("(display-mode: fullscreen), (display-mode: standalone)").matches ||
  navigator.standalone === true;
const tactile = matchMedia("(pointer: coarse)").matches;

// ---------- Réglages (confort par appareil) ----------

const reglages = { volume: 80, coupe: false, astuce: false };
try {
  Object.assign(reglages, JSON.parse(localStorage.getItem("gbemu-reglages") || "{}"));
} catch {}
function garderReglages() {
  try {
    localStorage.setItem("gbemu-reglages", JSON.stringify(reglages));
  } catch {}
}
function appliquerSon() {
  son.reglerVolume(reglages.volume / 100, reglages.coupe);
  $("m-son").textContent = reglages.coupe ? "Remettre le son" : "Couper le son";
}
appliquerSon();

// ---------- Petits utilitaires ----------

let minuteurToast = 0;
function toast(texte) {
  const t = $("toast");
  t.textContent = texte;
  t.hidden = false;
  t.classList.remove("parti");
  clearTimeout(minuteurToast);
  minuteurToast = setTimeout(() => t.classList.add("parti"), 2600);
}
$("toast").addEventListener("transitionend", (e) => {
  if (e.target.classList.contains("parti")) e.target.hidden = true;
});

const sansExt = (nom) => nom.replace(/\.[^.]+$/, "");
const extension = (nom) => (nom.match(/\.([^.]+)$/) || ["", ""])[1].toLowerCase();

// « Pokemon - Version Rouge (France) [!].gb » → « Pokemon - Version Rouge ».
function nomAffiche(jeu) {
  const n = sansExt(jeu.nom).replace(/\s*[([][^)\]]*[)\]]/g, "").replace(/_/g, " ").trim();
  return n || jeu.titre || jeu.nom;
}

const normaliser = (s) => s.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();

function quand(t) {
  if (!t) return "Jamais lancé";
  const jour = (d) => Math.floor((d - new Date(d).getTimezoneOffset() * 60000) / 86400000);
  const n = jour(Date.now()) - jour(t);
  if (n <= 0) return "Joué aujourd'hui";
  if (n === 1) return "Joué hier";
  if (n < 30) return `Joué il y a ${n} jours`;
  return "Joué le " + new Date(t).toLocaleDateString("fr-FR", { day: "numeric", month: "long" });
}

const hex = (crc) => (crc >>> 0).toString(16).padStart(8, "0");

function afficherVue(id) {
  for (const v of document.querySelectorAll(".vue")) v.hidden = v.id !== id;
}

function ouvrirVoile(el) {
  el.hidden = false;
  const b = el.querySelector("button");
  if (b) b.focus({ preventScroll: true });
}

// ---------- Bibliothèque ----------

let jeux = [];

async function chargerBiblio() {
  try {
    jeux = await S.listerJeux();
  } catch (e) {
    toast("Stockage indisponible : " + e.message);
    jeux = [];
  }
  dessinerBiblio();
}

function carte(jeu) {
  const li = document.createElement("li");
  li.className = "element";
  const b = document.createElement("button");
  b.type = "button";
  b.className = "carte";
  b.dataset.id = jeu.id;
  const c = document.createElement("span");
  c.className = "cartouche " + jeu.console;
  c.setAttribute("aria-hidden", "true");
  // Teinte d'étiquette tirée de l'empreinte : chaque jeu garde la sienne.
  c.style.setProperty("--teinte", (parseInt(jeu.id, 16) % 360) + "deg");
  const et = document.createElement("span");
  et.className = "etiquette";
  et.textContent = nomAffiche(jeu).slice(0, 28);
  c.append(et);
  const titre = document.createElement("span");
  titre.className = "titre";
  titre.textContent = nomAffiche(jeu);
  const meta = document.createElement("span");
  meta.className = "meta";
  const puce = document.createElement("span");
  puce.className = "puce " + jeu.console;
  puce.textContent = COURT[jeu.console];
  puce.title = CONSOLES[jeu.console];
  meta.append(puce, " ", quand(jeu.joue));
  b.append(c, titre, meta);
  const s = document.createElement("button");
  s.type = "button";
  s.className = "suppr";
  s.dataset.suppr = jeu.id;
  s.setAttribute("aria-label", "Supprimer " + nomAffiche(jeu));
  s.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"/></svg>';
  li.append(b, s);
  return li;
}

function dessinerBiblio() {
  const q = normaliser($("recherche").value.trim());
  const liste = jeux
    .filter((j) => !q || normaliser(nomAffiche(j) + " " + j.titre + " " + COURT[j.console]).includes(q))
    .sort((a, b) => (b.joue || 0) - (a.joue || 0) || nomAffiche(a).localeCompare(nomAffiche(b), "fr"));
  $("liste").replaceChildren(...liste.map(carte));
  $("vide").hidden = jeux.length > 0;
  $("aucun").hidden = jeux.length === 0 || liste.length > 0;
  $("recherche").parentElement.classList.toggle("sans-jeux", jeux.length === 0);
}

$("recherche").addEventListener("input", dessinerBiblio);
$("b-ajouter").addEventListener("click", () => $("f-roms").click());

$("liste").addEventListener("click", (e) => {
  const s = e.target.closest("[data-suppr]");
  if (s) return demanderSuppression(s.dataset.suppr);
  const c = e.target.closest(".carte");
  if (c) lancer(c.dataset.id);
});

function demanderSuppression(id) {
  const jeu = jeux.find((j) => j.id === id);
  if (!jeu) return;
  $("confirmer-titre").textContent = `Supprimer « ${nomAffiche(jeu)} » ?`;
  $("confirmer-texte").textContent =
    "Le jeu est retiré de cet appareil. Sa sauvegarde est gardée : elle reviendra si tu l'ajoutes à nouveau.";
  const v = $("confirmer");
  ouvrirVoile(v);
  $("c-oui").onclick = async () => {
    v.hidden = true;
    try {
      await S.supprimerJeu(id);
      toast("Jeu supprimé");
    } catch (e) {
      toast("Suppression impossible : " + e.message);
    }
    chargerBiblio();
  };
}
$("c-non").addEventListener("click", () => ($("confirmer").hidden = true));

// Ajout : ROM en vrac, .zip, et .sav au passage.
$("f-roms").addEventListener("change", async (e) => {
  const fichiers = [...e.target.files];
  e.target.value = "";
  if (!fichiers.length) return;
  const etat = $("etat-ajout");
  etat.hidden = false;
  etat.textContent = "Lecture des fichiers…";
  await pret;
  S.demanderPersistance();
  let ajoutes = 0;
  let deja = 0;
  let ignores = 0;
  let sav = 0;
  const connus = new Set(jeux.map((j) => j.id));
  const sauvegardes = [];
  for (const f of fichiers) {
    let entrees;
    try {
      const octets = new Uint8Array(await f.arrayBuffer());
      entrees =
        extension(f.name) === "zip"
          ? ouvrir_zip(octets).map((x) => ({ nom: x.nom, octets: x.octets }))
          : [{ nom: f.name, octets }];
    } catch {
      ignores++;
      continue;
    }
    for (const x of entrees) {
      if (extension(x.nom) === "sav") {
        sauvegardes.push(x);
        continue;
      }
      let info;
      try {
        info = analyser_rom(x.nom, x.octets);
      } catch {
        ignores++;
        continue;
      }
      const id = hex(info.crc);
      if (connus.has(id)) {
        deja++;
        continue;
      }
      const jeu = { id, nom: x.nom, titre: info.titre, console: info.console, taille: x.octets.length, ajoute: Date.now(), joue: 0 };
      info.free();
      try {
        await S.ajouterJeu(jeu, x.octets);
        connus.add(id);
        jeux.push(jeu);
        ajoutes++;
        etat.textContent = `${ajoutes} jeu${ajoutes > 1 ? "x" : ""} ajouté${ajoutes > 1 ? "s" : ""}…`;
      } catch (err) {
        toast("Espace insuffisant ou stockage refusé : " + err.message);
        ignores++;
      }
    }
  }
  if (sauvegardes.length) sav = (await importerSauvegardes(sauvegardes)).importees;
  const morceaux = [];
  if (ajoutes) morceaux.push(`${ajoutes} jeu${ajoutes > 1 ? "x" : ""} ajouté${ajoutes > 1 ? "s" : ""}`);
  if (deja) morceaux.push(`${deja} déjà dans la bibliothèque`);
  if (sav) morceaux.push(`${sav} sauvegarde${sav > 1 ? "s" : ""} importée${sav > 1 ? "s" : ""}`);
  if (ignores) morceaux.push(`${ignores} fichier${ignores > 1 ? "s" : ""} ignoré${ignores > 1 ? "s" : ""} (pas une ROM GB, GBC ou GBA)`);
  etat.textContent = morceaux.join(", ") + ".";
  setTimeout(() => (etat.hidden = true), 6000);
  dessinerBiblio();
});

// ---------- Plein écran, installation ----------

let invite = null;
const pleinEcranPossible = () => document.fullscreenEnabled && !installe();

function basculerPleinEcran() {
  if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
  else document.documentElement.requestFullscreen({ navigationUI: "hide" }).catch(() => {});
}
$("b-plein-ecran").hidden = !pleinEcranPossible();
$("b-plein-ecran").addEventListener("click", basculerPleinEcran);

function majAstuce() {
  const a = $("astuce");
  if (installe() || reglages.astuce || !tactile) {
    a.hidden = true;
    return;
  }
  $("astuce-texte").textContent = ios
    ? "Pour jouer en plein écran, sans la barre de Safari : touche Partager, puis « Sur l'écran d'accueil »."
    : "Pour jouer en plein écran, sans la barre du navigateur : installe l'appli, ou menu ⋮ puis « Ajouter à l'écran d'accueil ».";
  $("b-installer").hidden = !invite;
  a.hidden = false;
}
window.addEventListener("beforeinstallprompt", (e) => {
  e.preventDefault();
  invite = e;
  majAstuce();
});
$("b-installer").addEventListener("click", async () => {
  if (!invite) return;
  invite.prompt();
  await invite.userChoice.catch(() => {});
  invite = null;
  majAstuce();
});
$("b-astuce-fermer").addEventListener("click", () => {
  reglages.astuce = true;
  garderReglages();
  majAstuce();
});
majAstuce();

// ---------- Partie ----------

let partie = null;
const commandes = new Commandes($("jeu"), () => basculerPause());
const toile = $("ecran");
const ctx2d = toile.getContext("2d", { alpha: false });

async function lancer(id) {
  if (partie) return;
  if (moduleCasse) {
    location.reload();
    return;
  }
  // Dans le geste : son (iOS) et plein écran (navigateur Android).
  son.demarrer();
  if (tactile && pleinEcranPossible() && !document.fullscreenElement) {
    document.documentElement.requestFullscreen({ navigationUI: "hide" }).catch(() => {});
  }
  let jeu, rom, sav, emu;
  try {
    await pret;
    [jeu, rom, sav] = await Promise.all([S.lireJeu(id), S.lireRom(id), S.lireSauvegarde(id)]);
    if (!jeu || !rom) throw new Error("ROM absente du stockage");
    emu = new Emu(jeu.console, rom);
  } catch (e) {
    toast("Impossible de lancer le jeu : " + e.message);
    return;
  }
  if (sav) emu.charger_sauvegarde(sav.data);
  const w = emu.largeur;
  const h = emu.hauteur;
  toile.width = w;
  toile.height = h;
  partie = {
    id,
    jeu,
    emu,
    hz: emu.hz,
    freq: 0,
    image: new ImageData(w, h),
    acc: 0,
    dernier: performance.now(),
    pause: false,
    raf: 0,
    // Dernière sauvegarde écrite, pour n'écrire que ce qui change.
    sav: sav ? sav.data : emu.sauvegarde(),
    ecriture: Promise.resolve(),
    minuteur: setInterval(sauverBatterie, 2000),
    verrou: null,
  };
  const vue = $("jeu");
  vue.dataset.console = jeu.console;
  vue.classList.remove("allume");
  $("pause-jeu").textContent = nomAffiche(jeu);
  $("menu-pause").hidden = true;
  afficherVue("jeu");
  dimensionner();
  requestAnimationFrame(() => {
    vue.classList.add("allume");
    commandes.mesurer();
  });
  commandes.actif = true;
  garderEveil();
  jeu.joue = Date.now();
  S.majJeu(jeu).catch(() => {});
  partie.raf = requestAnimationFrame(boucle);
}

// Cadence de la console indépendante de l'écran : accumulateur, au plus 3 images par
// rafraîchissement, retard au-delà abandonné. Asservie (±0,5 %) au niveau de la file son.
function boucle(t) {
  const p = partie;
  if (!p) return;
  p.raf = requestAnimationFrame(boucle);
  let dt = t - p.dernier;
  p.dernier = t;
  if (p.pause) return;
  if (dt > 250) dt = 250;
  p.acc += (dt / 1000) * p.hz * son.correction();
  try {
    const f = son.frequence();
    if (f && f !== p.freq) {
      p.freq = f;
      p.emu.set_frequence(f);
      p.emu.echantillons();
      son.vider();
    }
    let n = 0;
    while (p.acc >= 1 && n < 3) {
      p.emu.set_boutons(commandes.lire());
      p.emu.run_frame();
      son.pousser(p.emu.echantillons());
      p.acc -= 1;
      n++;
    }
    if (p.acc > 2) p.acc = 0;
    if (n) dessiner(p);
  } catch (e) {
    planter(e);
  }
}

function dessiner(p) {
  const n = p.image.data.length;
  const ptr = p.emu.ecran();
  p.image.data.set(new Uint8ClampedArray(wasm.memory.buffer, ptr, n));
  ctx2d.putImageData(p.image, 0, 0);
}

// Écran à l'échelle entière en pixels physiques quand c'est possible.
function dimensionner() {
  const p = partie;
  if (!p) return;
  const vue = $("jeu");
  const cs = getComputedStyle(vue);
  const larg = vue.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
  const haut = vue.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
  const paysage = larg > haut;
  const gba = p.jeu.console === "gba";
  // Place laissée aux commandes : bas de l'écran en portrait, côtés en paysage.
  // (20 px : cadre autour de l'écran)
  const dispoL = paysage ? larg - 2 * (gba ? 150 : 170) - 20 : larg - 24;
  const dispoH = paysage ? haut - 24 : Math.min(haut * 0.56, haut - 320) - 20;
  const dpr = window.devicePixelRatio || 1;
  const w = toile.width;
  const h = toile.height;
  const entier = Math.floor(Math.min((dispoL * dpr) / w, (dispoH * dpr) / h));
  const e = entier >= Math.ceil(dpr) ? entier / dpr : Math.max(0.5, Math.min(dispoL / w, dispoH / h));
  toile.style.width = (w * e) + "px";
  toile.style.height = (h * e) + "px";
}
window.addEventListener("resize", () => {
  dimensionner();
  requestAnimationFrame(() => commandes.mesurer());
});

function egaux(a, b) {
  if (!a || !b || a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

// Sauvegarde batterie : écrite seulement si elle a changé, écritures en file (jamais deux à la fois).
function sauverBatterie() {
  const p = partie;
  if (!p || p.casse) return p ? p.ecriture : Promise.resolve();
  let s;
  try {
    s = p.emu.sauvegarde();
  } catch (e) {
    planter(e);
    return p.ecriture;
  }
  if (!s || egaux(s, p.sav)) return p.ecriture;
  p.sav = s;
  p.ecriture = p.ecriture
    .then(() => S.ecrireSauvegarde(p.id, s))
    .catch((e) => toast("Sauvegarde impossible : " + e.message));
  return p.ecriture;
}

function basculerPause(forcer) {
  const p = partie;
  if (!p) return;
  const pause = forcer === undefined ? !p.pause : forcer;
  if (pause === p.pause) return;
  p.pause = pause;
  commandes.relacher();
  if (pause) {
    sauverBatterie();
    son.suspendre();
    $("m-charger-date").textContent = "";
    S.lireEtat(p.id)
      .then((e) => {
        $("m-charger").disabled = !e;
        if (e) $("m-charger-date").textContent = new Date(e.date).toLocaleString("fr-FR", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
      })
      .catch(() => ($("m-charger").disabled = true));
    ouvrirVoile($("menu-pause"));
  } else {
    $("menu-pause").hidden = true;
    son.vider();
    son.demarrer();
    p.acc = 0;
  }
}

$("b-pause").addEventListener("click", () => basculerPause(true));
$("m-reprendre").addEventListener("click", () => basculerPause(false));
$("m-son").addEventListener("click", () => {
  reglages.coupe = !reglages.coupe;
  garderReglages();
  appliquerSon();
});
$("m-sauver").addEventListener("click", async () => {
  const p = partie;
  if (!p) return;
  try {
    await S.ecrireEtat(p.id, p.emu.sauver_etat());
    toast("État sauvé");
    basculerPause(false);
  } catch (e) {
    toast("État non sauvé : " + e.message);
  }
});
$("m-charger").addEventListener("click", async () => {
  const p = partie;
  if (!p) return;
  try {
    const e = await S.lireEtat(p.id);
    if (!e) return;
    p.emu.charger_etat(e.data);
    toast("État chargé");
    basculerPause(false);
  } catch (e) {
    toast("État illisible : " + e.message);
  }
});
$("m-biblio").addEventListener("click", () => quitter());

async function quitter() {
  const p = partie;
  if (!p) return;
  await sauverBatterie();
  arreter();
  afficherVue("biblio");
  chargerBiblio();
}

function arreter() {
  const p = partie;
  if (!p) return;
  cancelAnimationFrame(p.raf);
  clearInterval(p.minuteur);
  commandes.actif = false;
  commandes.relacher();
  son.vider();
  son.suspendre();
  if (p.verrou) p.verrou.release().catch(() => {});
  if (!p.casse) p.emu.free();
  partie = null;
  $("menu-pause").hidden = true;
}

function planter(e) {
  const p = partie;
  if (!p) return;
  // Mémoire du module douteuse : surtout ne rien écrire par-dessus la dernière bonne sauvegarde.
  p.casse = true;
  moduleCasse = e instanceof WebAssembly.RuntimeError;
  console.error(e);
  p.ecriture.finally(() => {
    arreter();
    afficherVue("biblio");
    chargerBiblio();
    toast("Le jeu s'est arrêté sur une erreur de l'émulateur. La dernière sauvegarde est gardée.");
  });
}

async function garderEveil() {
  try {
    if (navigator.wakeLock && partie) partie.verrou = await navigator.wakeLock.request("screen");
  } catch {}
}

// Arrière-plan : pause et sauvegarde tout de suite.
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "hidden") {
    basculerPause(true);
    sauverBatterie();
  } else if (partie) garderEveil();
});
window.addEventListener("pagehide", () => sauverBatterie());

// Premier geste n'importe où : le son peut démarrer (iOS).
const premierGeste = () => son.demarrer();
document.addEventListener("pointerdown", premierGeste, { once: true });
document.addEventListener("keydown", premierGeste, { once: true });

// ---------- Réglages ----------

$("b-reglages").addEventListener("click", async () => {
  $("r-volume").value = reglages.volume;
  $("r-coupe").checked = reglages.coupe;
  $("r-message").textContent = "";
  $("r-espace").textContent = "Tes jeux et sauvegardes restent sur cet appareil.";
  ouvrirVoile($("reglages"));
  try {
    const est = await navigator.storage.estimate();
    const persiste = navigator.storage.persisted ? await navigator.storage.persisted() : false;
    $("r-espace").textContent =
      `Tes jeux et sauvegardes restent sur cet appareil : ${(est.usage / 1048576).toFixed(1)} Mo utilisés. ` +
      (persiste ? "Le navigateur ne les effacera pas pour faire de la place." : "Exporte tes sauvegardes de temps en temps : le navigateur peut effacer les données d'un site peu utilisé.");
  } catch {}
});
$("r-fermer").addEventListener("click", () => ($("reglages").hidden = true));
$("r-volume").addEventListener("input", (e) => {
  reglages.volume = +e.target.value;
  garderReglages();
  appliquerSon();
});
$("r-coupe").addEventListener("change", (e) => {
  reglages.coupe = e.target.checked;
  garderReglages();
  appliquerSon();
});

$("r-exporter").addEventListener("click", async () => {
  const msg = $("r-message");
  try {
    const [savs, liste] = await Promise.all([S.listerSauvegardes(), S.listerJeux()]);
    if (!savs.length) {
      msg.textContent = "Aucune sauvegarde pour l'instant : elles apparaissent quand un jeu enregistre.";
      return;
    }
    const noms = new Set();
    const fichiers = savs.map((s) => {
      const jeu = liste.find((j) => j.id === s.id);
      let nom = `${jeu ? sansExt(jeu.nom) : "Jeu"} [${s.id.toUpperCase()}].sav`;
      for (let i = 2; noms.has(nom); i++) nom = nom.replace(/(\.sav)$/, ` ${i}$1`);
      noms.add(nom);
      return { nom, octets: s.data };
    });
    const date = new Date().toISOString().slice(0, 10);
    const blob = zipper(fichiers);
    const nom = `gbemu-sauvegardes-${date}.zip`;
    const fichier = new File([blob], nom, { type: "application/zip" });
    if (ios && navigator.canShare && navigator.canShare({ files: [fichier] })) {
      await navigator.share({ files: [fichier], title: nom });
    } else {
      const a = document.createElement("a");
      a.href = URL.createObjectURL(blob);
      a.download = nom;
      document.body.append(a);
      a.click();
      a.remove();
      setTimeout(() => URL.revokeObjectURL(a.href), 10000);
    }
    msg.textContent = `${fichiers.length} sauvegarde${fichiers.length > 1 ? "s" : ""} exportée${fichiers.length > 1 ? "s" : ""}.`;
  } catch (e) {
    if (e.name !== "AbortError") msg.textContent = "Export impossible : " + e.message;
  }
});

$("r-importer").addEventListener("click", () => $("f-sav").click());
$("f-sav").addEventListener("change", async (e) => {
  const fichiers = [...e.target.files];
  e.target.value = "";
  if (!fichiers.length) return;
  await pret;
  const sav = [];
  for (const f of fichiers) {
    try {
      const octets = new Uint8Array(await f.arrayBuffer());
      if (extension(f.name) === "zip") {
        for (const x of ouvrir_zip(octets)) if (extension(x.nom) === "sav") sav.push({ nom: x.nom, octets: x.octets });
      } else sav.push({ nom: f.name, octets });
    } catch {}
  }
  const r = await importerSauvegardes(sav);
  $("r-message").textContent =
    `${r.importees} sauvegarde${r.importees > 1 ? "s" : ""} importée${r.importees > 1 ? "s" : ""}` +
    (r.orphelines ? `, ${r.orphelines} sans jeu reconnu (nom « Jeu [CRC].sav » ou nom du fichier de la ROM attendu)` : "") + ".";
});

// Associe chaque .sav à un jeu : « [CRC] » dans le nom, sinon même nom que le fichier de la ROM.
// La sauvegarde remplacée reste en `ancienne`.
async function importerSauvegardes(sav) {
  const liste = await S.listerJeux().catch(() => []);
  let importees = 0;
  let orphelines = 0;
  for (const s of sav) {
    const m = s.nom.match(/\[([0-9a-fA-F]{8})\]/);
    let id = m ? m[1].toLowerCase() : null;
    if (!id) {
      const base = normaliser(sansExt(s.nom));
      const jeu = liste.find((j) => normaliser(sansExt(j.nom)) === base);
      id = jeu ? jeu.id : null;
    }
    if (!id || !s.octets.length) {
      orphelines++;
      continue;
    }
    try {
      await S.ecrireSauvegarde(id, s.octets);
      importees++;
    } catch {
      orphelines++;
    }
  }
  return { importees, orphelines };
}

// Échap ferme le voile ouvert (hors partie : la pause a sa propre touche).
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape" || partie) return;
  for (const id of ["confirmer", "reglages"]) if (!$(id).hidden) $(id).hidden = true;
});

// ---------- Démarrage ----------

if ("serviceWorker" in navigator) navigator.serviceWorker.register("sw.js").catch(() => {});
pret.catch((e) => toast("Le moteur de l'émulateur n'a pas pu se charger : " + e.message));
chargerBiblio();

