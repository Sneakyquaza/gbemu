// Entrées : commandes tactiles multitouch (croix à 8 directions qu'on glisse, A/B qu'on peut
// enchaîner du doigt), clavier et manette (Gamepad API). Rend le masque des boutons tenus.

export const B = {
  DROITE: 0x01,
  GAUCHE: 0x02,
  HAUT: 0x04,
  BAS: 0x08,
  A: 0x10,
  B: 0x20,
  SELECT: 0x40,
  START: 0x80,
  R: 0x200,
  L: 0x100,
};

const BITS = { a: B.A, b: B.B, select: B.SELECT, start: B.START, l: B.L, r: B.R };

// Touches par défaut de l'app de bureau (gb-app/src/touches.rs).
const CLAVIER = {
  arrowup: B.HAUT,
  arrowdown: B.BAS,
  arrowleft: B.GAUCHE,
  arrowright: B.DROITE,
  x: B.A,
  k: B.A,
  z: B.B,
  w: B.B,
  j: B.B,
  backspace: B.SELECT,
  " ": B.SELECT,
  enter: B.START,
  a: B.L,
  s: B.R,
};

// Manette « standard » : Est/Nord = A, Sud/Ouest = B (positionnel, comme sur PC).
const MANETTE = [
  [0, B.B],
  [1, B.A],
  [2, B.B],
  [3, B.A],
  [4, B.L],
  [5, B.R],
  [6, B.L],
  [7, B.R],
  [8, B.SELECT],
  [9, B.START],
  [12, B.HAUT],
  [13, B.BAS],
  [14, B.GAUCHE],
  [15, B.DROITE],
];

const MARGE = 14; // px ajoutés autour des boutons pour le toucher
const MARGE_CROIX = 28;

export class Commandes {
  constructor(zone, surPause) {
    this.zone = zone;
    this.surPause = surPause;
    this.doigts = new Map(); // pointerId -> { croix: bool, bits }
    this.clavier = 0;
    this.tactile = 0;
    this.precedent = 0;
    this.actif = false;
    this.rects = [];
    this.croix = zone.querySelector("[data-croix]");
    this.boutons = [...zone.querySelectorAll("[data-b]")];

    const sur = (type, f) => zone.addEventListener(type, f, { passive: false });
    sur("pointerdown", (e) => this.toucher(e, true));
    sur("pointermove", (e) => this.toucher(e, false));
    for (const t of ["pointerup", "pointercancel", "lostpointercapture"]) {
      sur(t, (e) => this.lacher(e));
    }
    // Pas de loupe, de menu ni de sélection au toucher long.
    sur("contextmenu", (e) => e.preventDefault());
    sur("touchstart", (e) => {
      if (!e.target.closest("[data-hors-commandes]")) e.preventDefault();
    });
    window.addEventListener("keydown", (e) => this.touche(e, true));
    window.addEventListener("keyup", (e) => this.touche(e, false));
    window.addEventListener("blur", () => this.relacher());
    window.addEventListener("resize", () => this.mesurer());
  }

  // Position des commandes à l'écran (après chaque mise en page).
  mesurer() {
    const r = (el, m) => {
      const b = el.getBoundingClientRect();
      return { el, x0: b.left - m, y0: b.top - m, x1: b.right + m, y1: b.bottom + m };
    };
    this.rectCroix = r(this.croix, MARGE_CROIX);
    this.rects = this.boutons
      .filter((el) => el.offsetParent !== null)
      .map((el) => Object.assign(r(el, MARGE), { bit: BITS[el.dataset.b] }));
  }

  dedans(r, x, y) {
    return x >= r.x0 && x <= r.x1 && y >= r.y0 && y <= r.y1;
  }

  // Direction de la croix (8 secteurs, petite zone morte au centre).
  direction(x, y) {
    const r = this.rectCroix;
    const cx = (r.x0 + r.x1) / 2;
    const cy = (r.y0 + r.y1) / 2;
    const dx = x - cx;
    const dy = y - cy;
    const rayon = (r.x1 - r.x0) / 2 - MARGE_CROIX;
    if (Math.hypot(dx, dy) < rayon * 0.18) return 0;
    const secteur = Math.round(Math.atan2(dy, dx) / (Math.PI / 4)) & 7; // 0 = droite, sens horaire
    return [B.DROITE, B.DROITE | B.BAS, B.BAS, B.BAS | B.GAUCHE, B.GAUCHE, B.GAUCHE | B.HAUT, B.HAUT, B.HAUT | B.DROITE][secteur];
  }

  toucher(e, debut) {
    if (!this.actif || e.pointerType === "mouse" && e.buttons === 0) return;
    if (e.target.closest("[data-hors-commandes]")) return;
    const x = e.clientX;
    const y = e.clientY;
    let d = this.doigts.get(e.pointerId);
    if (debut) {
      if (!this.rects.length) this.mesurer();
      d = { croix: this.dedans(this.rectCroix, x, y), bits: 0 };
      if (!d.croix && !this.rects.some((r) => this.dedans(r, x, y))) return;
      this.doigts.set(e.pointerId, d);
      e.preventDefault();
    }
    if (!d) return;
    // Le doigt posé sur la croix la garde jusqu'au lâcher ; sinon il passe d'un bouton à l'autre.
    d.bits = d.croix
      ? this.direction(x, y)
      : this.rects.reduce((m, r) => (this.dedans(r, x, y) ? m | r.bit : m), 0);
    this.majTactile();
  }

  lacher(e) {
    if (this.doigts.delete(e.pointerId)) this.majTactile();
  }

  majTactile() {
    let m = 0;
    for (const d of this.doigts.values()) m |= d.bits;
    if (m & ~this.tactile && navigator.vibrate) navigator.vibrate(8);
    this.tactile = m;
    this.afficher(m);
  }

  // Retour visuel des boutons tenus.
  afficher(m) {
    for (const el of this.boutons) el.classList.toggle("tenu", (m & BITS[el.dataset.b]) !== 0);
    const dirs = [
      [B.HAUT, "haut"],
      [B.BAS, "bas"],
      [B.GAUCHE, "gauche"],
      [B.DROITE, "droite"],
    ];
    for (const [bit, nom] of dirs) this.croix.classList.toggle(nom, (m & bit) !== 0);
  }

  touche(e, bas) {
    if (!this.actif || e.target.closest("input, select, textarea")) return;
    const k = e.key.toLowerCase();
    if (k === "escape" || k === "p") {
      if (bas && !e.repeat) this.surPause();
      return;
    }
    const bit = CLAVIER[k];
    if (!bit) return;
    e.preventDefault();
    this.clavier = bas ? this.clavier | bit : this.clavier & ~bit;
  }

  manette() {
    if (!navigator.getGamepads) return 0;
    let m = 0;
    for (const p of navigator.getGamepads()) {
      if (!p || !p.connected) continue;
      for (const [i, bit] of MANETTE) if (p.buttons[i] && p.buttons[i].pressed) m |= bit;
      const [ax = 0, ay = 0] = p.axes;
      if (ax < -0.5) m |= B.GAUCHE;
      if (ax > 0.5) m |= B.DROITE;
      if (ay < -0.5) m |= B.HAUT;
      if (ay > 0.5) m |= B.BAS;
      // Bouton « home » / guide : pause.
      if (p.buttons[16] && p.buttons[16].pressed && !this.guide) this.surPause();
      this.guide = p.buttons[16] && p.buttons[16].pressed;
    }
    return m;
  }

  // Masque des boutons tenus (tous moyens confondus) ; gauche+droite opposés annulés.
  lire() {
    let m = this.tactile | this.clavier | this.manette();
    if ((m & (B.GAUCHE | B.DROITE)) === (B.GAUCHE | B.DROITE)) m &= ~(B.GAUCHE | B.DROITE);
    if ((m & (B.HAUT | B.BAS)) === (B.HAUT | B.BAS)) m &= ~(B.HAUT | B.BAS);
    return m;
  }

  relacher() {
    this.doigts.clear();
    this.clavier = 0;
    this.tactile = 0;
    this.afficher(0);
  }
}
