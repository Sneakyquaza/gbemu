// Son : AudioContext créé au premier toucher (exigence iOS), AudioWorklet (repli
// ScriptProcessor), volume. Le niveau de la file sert à asservir la cadence (±0,5 %).

let ctx = null;
let gain = null;
let noeud = null;
let worklet = false;
let niveau = 0; // échantillons stéréo en attente
// Repli ScriptProcessor : même file que le worklet, côté page.
const file = { blocs: [], pos: 0, g: 0, d: 0 };

let volume = 0.8;
let coupe = false;

export function frequence() {
  return ctx ? ctx.sampleRate : 0;
}

// Cible : ~60 ms en attente.
export function correction() {
  if (!ctx || ctx.state !== "running") return 1;
  const cible = ctx.sampleRate * 0.06;
  const e = (cible - niveau) / cible;
  return 1 + Math.max(-0.005, Math.min(0.005, e * 0.005));
}

function appliquerVolume() {
  if (gain) gain.gain.value = coupe ? 0 : volume * volume;
}

export function reglerVolume(v, c) {
  volume = v;
  coupe = c;
  appliquerVolume();
}

// À appeler dans un geste (toucher, touche, clic).
export async function demarrer() {
  if (ctx) {
    if (ctx.state !== "running") ctx.resume().catch(() => {});
    return;
  }
  const AC = window.AudioContext || window.webkitAudioContext;
  if (!AC) return;
  // iOS 17+ : jouer même avec le bouton silencieux.
  if (navigator.audioSession) navigator.audioSession.type = "playback";
  ctx = new AC({ latencyHint: "interactive" });
  gain = ctx.createGain();
  gain.connect(ctx.destination);
  appliquerVolume();
  ctx.resume().catch(() => {});
  try {
    await ctx.audioWorklet.addModule("js/son-worklet.js");
    noeud = new AudioWorkletNode(ctx, "lecteur-gb", { outputChannelCount: [2] });
    noeud.port.onmessage = (e) => (niveau = e.data);
    worklet = true;
  } catch {
    noeud = ctx.createScriptProcessor(1024, 0, 2);
    noeud.onaudioprocess = lireFile;
  }
  noeud.connect(gain);
}

function lireFile(e) {
  const g = e.outputBuffer.getChannelData(0);
  const d = e.outputBuffer.getChannelData(1);
  for (let i = 0; i < g.length; i++) {
    const b = file.blocs[0];
    if (b) {
      file.g = b[file.pos];
      file.d = b[file.pos + 1];
      file.pos += 2;
      niveau--;
      if (file.pos >= b.length) {
        file.blocs.shift();
        file.pos = 0;
      }
    } else {
      file.g *= 0.995;
      file.d *= 0.995;
    }
    g[i] = file.g;
    d[i] = file.d;
  }
}

// Blocs stéréo entrelacés produits par la console.
export function pousser(ech) {
  if (!noeud || ech.length === 0) return;
  if (worklet) {
    noeud.port.postMessage(ech, [ech.buffer]);
    niveau += ech.length / 2;
  } else {
    file.blocs.push(ech);
    niveau += ech.length / 2;
    while (niveau > ctx.sampleRate / 4 && file.blocs.length > 1) {
      niveau -= (file.blocs.shift().length - file.pos) / 2;
      file.pos = 0;
    }
  }
}

export function vider() {
  niveau = 0;
  if (worklet) noeud.port.postMessage("vider");
  else {
    file.blocs = [];
    file.pos = 0;
  }
}

export function suspendre() {
  if (ctx && ctx.state === "running") ctx.suspend().catch(() => {});
}
