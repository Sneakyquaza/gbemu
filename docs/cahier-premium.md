# Cahier des charges : gbemu « premium » (22/11/2026)

Demande (22/11 14:32) : appli encore plus belle et premium, mappage des touches sur une vraie Game Boy dessinée, save states, et beaucoup des fonctionnalités des autres émulateurs.
Références étudiées : `references/emulateurs/` (vidéos 480p, planches `planche.jpg`, détails `details/planche_details.jpg`, sources dans `sources.txt`).

---

## 0. Ce qu'on retient des meilleurs

| Référence | Ce qui la rend premium | À reprendre |
|---|---|---|
| **Delta** (iOS) | La console *est* l'interface (skins), menu pause en grille d'icônes sur le jeu flouté, « Reprendre / Recommencer » à la réouverture, états verrouillables, état « aperçu » sur appui long | Grille d'icônes du menu pause, reprise automatique, états verrouillés |
| **OpenEmu** (macOS) | Bibliothèque façon iTunes (jaquettes, barre latérale par système), HUD flottant translucide en bas du jeu (quitter, pause, recommencer, états, réglages, volume, plein écran), **mappage sur une manette dessinée** (clic sur le bouton dans le panneau, l'anneau bleu entoure le bouton illustré) | HUD flottant, mappage illustré avec halo sur le bouton actif |
| **mGBA** | Mappage sur une GBA dessinée avec un champ sous chaque bouton, « Set all » qui enchaîne les boutons, raccourcis complets (avance rapide maintenue/basculée, rembobinage maintenu, retour d'une image), annulation du dernier chargement/sauvegarde d'état, 9 emplacements (F1-F9 / Maj+F1-F9) | « Tout configurer » en enchaîné, annuler le chargement, emplacements par touches F |
| **SameBoy** | Rendu le plus soigné : correction des couleurs réglable, mélange d'images (frame blending), mise à l'échelle OmniScale, passe-haut audio, 96 kHz, états BESS standards, palettes éditables | Correction couleur + mélange d'images, éditeur de palette plus tard |
| **RetroArch Ozone** | Barre latérale + liste dense, en-tête avec heure, pied avec rappels de touches manette | Pied d'aide contextuel (déjà fait), heure/batterie discrètes en mode TV |
| **ES-DE** | Navigation 100 % manette, carrousel de systèmes avec illustration de console, thèmes « transformants » | Mode TV : carrousel des plateformes |
| **Analogue Pocket** | Menus minimalistes en surimpression sur l'image, modes « Original DMG / GBP / Light » avec grille de pixels, mélange d'images en option, « Memories » (états) avec raccourci rapide | Modes d'écran nommés d'après les vraies consoles (sans marque), grille LCD |
| **Pizza Boy GBC/GBA Pro** | Skins par jeu, turbo, vitesse réglable, cheats, palettes, filtres LCD | Paramètres par jeu |
| **GB – Nintendo Switch Online** | Rembobinage maintenu (ZL+ZR, ~30-60 s), 4 points de suspension par jeu, filtres « GB / Pocket / GBC » | Rembobinage maintenu, nombre d'emplacements contenu |

Constats sur nos captures actuelles (`out/captures/`) :
- La base (fond ardoise-indigo, Rubik, accent baie) est saine, mais plate : aucune profondeur, aucune matière, les cartes se ressemblent toutes.
- Jaquette de repli « coucher de soleil » : bandeau titre noir **vide** (bug visible sur biblio-demo.png).
- Menu pause : deux lignes surlignées à la fois (survol souris + sélection clavier) → une seule surbrillance, la souris déplace la sélection.
- Le menu pause cache le jeu sans le flouter ni l'assombrir : on ne sent pas « l'arrêt ».

---

## 1. Direction visuelle : « boîtier de nuit »

Idée directrice : **la Game Boy la nuit, sous une lampe de bureau.** Plastique mat chaud, écran vert qui luit, boutons baie. Pas de néon, pas de verre à la mode : de la matière, de la lumière douce, des ombres longues.

### 1.1 Palette (jetons, `dessin.rs`)

Thème **Nuit** (défaut) :

| Jeton | Valeur | Usage |
|---|---|---|
| `FOND` | `#16171F` | fond d'app (plus profond qu'aujourd'hui) |
| `FOND_HALO` | `#22243A` → transparent | halo radial derrière la sélection / l'écran de jeu |
| `SURFACE` | `#23253A` | cartes, panneaux |
| `SURFACE_HAUTE` | `#2E3150` | carte sélectionnée, lignes actives |
| `TRAIT` | `#3A3D5C` à 60 % | bordures 1 px (haut plus clair : liseré lumière `#FFFFFF` à 6 %) |
| `TEXTE` | `#F0EBE3` | texte principal (blanc cassé « papier ») |
| `DOUX` | `#A29FB8` | secondaire |
| `DISCRET` | `#6C6A85` | tertiaire, aides |
| `ACCENT` | `#D24A86` | baie (boutons A/B), sélection, curseur |
| `ACCENT_CLAIR` | `#F07AAE` | halo d'accent, survol |
| `PLASTIQUE` | `#C9C4BC` / ombre `#9E998F` | console dessinée, cartouches |
| `ECRAN_LUEUR` | `#9BBC0F` à 18 % | lueur autour de l'écran de jeu (suit la palette choisie) |
| `SUCCES` / `ALERTE` | `#5BBF8A` / `#E8A23A` | toasts |

Plateformes inchangées : GB olive `#9AB048`, GBC sarcelle `#33A89C`, GBA lavande `#7A70E0`.

Thème **Jour** (option) : fond `#ECE8E1` (plastique DMG), surface `#F7F4EF`, texte `#24222E`, accent identique, ombres plus courtes et plus claires. Tous les écrans doivent passer par les jetons (aucune couleur en dur hors `dessin.rs`).

### 1.2 Typographie (libre, embarquée)

- **Rubik** (OFL, déjà embarquée) : titres en Gras 600/700, corps en Regular/Medium. On garde : arrondie comme le plastique.
- **Ajout : Silkscreen** (OFL, Jason Kottke, ~30 Ko) : uniquement pour les petits « tampons » rétro (numéro d'emplacement d'état, « ×4 », « REC », compteur du rembobinage). Jamais pour du texte courant.
- Échelle : 34 / 24 / 18 / 15 / 13 / 11 px. Interlettrage +2 % sur les titres en capitales (onglets plateforme).
- Chiffres tabulaires pour temps de jeu et dates (Rubik les a via `tnum` ; sinon alignement à droite).

### 1.3 Matières et ombres

- **Cartes** : rayon 14 px, dégradé vertical très léger (haut +3 % de luminosité), liseré haut 1 px blanc 6 %, ombre portée douce (`epaint::Shadow` : décalage 0/10, flou 28, noir 45 %). Sélection : soulèvement de 4 px, ombre plus longue, anneau accent 2 px avec halo `ACCENT_CLAIR` 25 % flou 16.
- **Grain** : texture de bruit 256×256 générée au démarrage (pas de fichier), mélangée à 3 % sur le fond : casse l'aplat numérique.
- **Vignettage** : dégradé radial sombre 0 → 35 % vers les bords, sur fond de bibliothèque et autour du jeu.
- **Verre dépoli** (menus en surimpression) : pas de vrai flou GPU nécessaire au départ ; on capture la dernière image du jeu en texture réduite (×1/4) puis agrandie lissée = flou bon marché, assombrie à 55 %.

### 1.4 Mouvement (toutes les durées réglées une fois dans `dessin.rs`)

- Courbe unique « ressort doux » : `ease_out_back` léger (dépassement 4 %) pour les sélections, `ease_out_cubic` pour les fondus.
- Déplacement de sélection : 140 ms ; soulèvement de carte : 180 ms ; ouverture de menu : 220 ms (échelle 0,96 → 1 + fondu) ; fermeture : 140 ms.
- Lancement d'un jeu : **la jaquette s'agrandit jusqu'à devenir l'écran** (220 ms), le fond s'éteint, la lueur verte s'allume. Retour : l'inverse, l'image du jeu rétrécit dans sa carte (qui prend la capture comme miniature).
- Option « Réduire les animations » : toutes les durées à 0, fondus gardés à 80 ms.

### 1.5 Sons d'interface (optionnels, coupés par défaut ? non : activés, volume 40 %)

Petits sons synthétisés par nous (générés en Rust au démarrage, ou faits avec `genny`/`chirp`, aucun échantillon tiers) : déplacement (clic plastique 15 ms), valider (double bip carré façon APU, 60 ms), retour (bip descendant), sauvegarde d'état (« ka-chunk » de cartouche), erreur (bourdon court). Un réglage « Sons d'interface » + volume. Jamais pendant le jeu sauf confirmation d'état.

### 1.6 Écran de démarrage

Pas d'écran de chargement (ouverture < 2 s, actuellement ~100 ms) : une **animation de 600 ms maximum superposée à la bibliothèque déjà prête** : le mot « gbemu » descend de 12 px avec un fondu (clin d'œil au défilement du logo au démarrage d'une DMG, sans logo Nintendo), puis un « ding » à deux notes (son d'interface). Passable par n'importe quelle touche, désactivable, jamais rejouée dans la même session.

### 1.7 Bibliothèque

- En-tête : logo texte + barre de recherche + onglets plateforme (pastilles) + compteur. Ajout : **onglets « Récents » et « Favoris »** à gauche des plateformes.
- **Rangée « Reprendre »** en haut (si au moins une partie a une reprise auto) : grandes cartes 16:9 avec la capture du dernier instant, le temps de jeu, « il y a 2 h ». Entrée = reprendre directement.
- Grille : cartes jaquette (vraie jaquette si présente, sinon cartouche de repli soignée : étiquette colorée par plateforme, titre bien composé, **pas de bandeau vide**). Sous la carte : titre, puis ligne discrète « GB · 3 h 12 · ★ ».
- **Panneau détail** (touche Y / clic droit / Info) : panneau latéral droit 360 px glissant, avec jaquette grande, année, temps de jeu, dernier lancement, emplacements d'état (miniatures), actions (Jouer, Reprendre, Favori, Palette de ce jeu, Ouvrir le dossier des sauvegardes).
- Tri : nom, récent, temps de jeu, année ; via un menu « Trier » (F3 / bouton Ouest manette).
- Fond : halo coloré derrière la carte sélectionnée, teinté par la couleur dominante de la jaquette (moyenne calculée une fois au chargement de la jaquette).

Maquette bibliothèque :

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ gbemu ▪▪▪   ( ⌕ Rechercher un jeu…        )   Récents  ★Favoris  [Tous] GB GBC GBA │
│                                                                                │
│  REPRENDRE                                                                     │
│  ╭────────────────────────╮  ╭────────────────────────╮                        │
│  │ ▓▓▓ capture 16:9 ▓▓▓▓▓ │  │ ▓▓▓ capture ▓▓▓▓▓▓▓▓▓▓ │                        │
│  │ Donjon Douillet        │  │ Kobold Kart            │                        │
│  │ ● GB · il y a 2 h · 3h12│ │ ● GB · hier · 0h40     │                        │
│  ╰────────────────────────╯  ╰────────────────────────╯                        │
│                                                                                │
│  TOUS LES JEUX  (24)                                     Trier : Récents ▾     │
│  ╭──────╮ ╭──────╮ ╔══════╗ ╭──────╮ ╭──────╮ ╭──────╮                         │
│  │ ▒▒▒▒ │ │ ▒▒▒▒ │ ║ ▒▒▒▒ ║ │ ▒▒▒▒ │ │ ▒▒▒▒ │ │ ▒▒▒▒ │   ← sélection soulevée,  │
│  │ ▒▒▒▒ │ │ ▒▒▒▒ │ ║ ▒▒▒▒ ║ │ ▒▒▒▒ │ │ ▒▒▒▒ │ │ ▒▒▒▒ │     anneau baie + halo   │
│  ╰──────╯ ╰──────╯ ╚══════╝ ╰──────╯ ╰──────╯ ╰──────╯                         │
│  Boulanger Carillon Cerf-vol. ...                                              │
│  ● GB 1h ★ ● GB    ● GB 12m                                                    │
│ ────────────────────────────────────────────────────────────────────────────── │
│ [Ⓐ] Jouer  [Ⓨ] Détails  [Ⓧ] Favori  [LB/RB] Plateforme  [☰] Réglages            │
└──────────────────────────────────────────────────────────────────────────────┘
```

### 1.8 Écran de jeu

- Image entière (mise à l'échelle entière par défaut, option « remplir »), centrée, **lueur douce de la couleur claire de la palette** autour (flou 40 px, 18 %), fond `FOND` vignetté.
- Option **« Cadre console »** : l'écran de jeu est posé dans la façade de la Game Boy dessinée (même illustration que l'écran de mappage, cadre gris foncé autour de l'écran, petite LED rouge de « batterie » qui s'allume). Désactivé en plein écran par défaut, activable.
- **HUD flottant** (façon OpenEmu) : apparaît 2 s quand la souris bouge, en bas au centre, pilule translucide : ⏸ · ⟲ Rembobiner · ⏩ Vitesse · 💾 États · 📷 Capture · 🔊 volume · ⛶ Plein écran. Jamais affiché à la manette (la manette passe par le menu pause).
- Indicateurs discrets en haut à droite, police Silkscreen : `×4` (avance rapide), `◀◀` (rembobinage, avec barre de durée restante), `ÉTAT 3 ✓` (sauvegardé), qui s'effacent en 1,2 s.

### 1.9 Menu pause en surimpression

Le jeu s'arrête, son image se fige, floutée (verre dépoli bon marché) et assombrie. À gauche, une colonne de **grandes tuiles d'action en grille 2 × 4** (façon Delta) ; à droite, l'**aperçu des emplacements d'état** quand la tuile États est sélectionnée.

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ ░░░░░░░░░░░░░░░ image du jeu figée, floutée, assombrie 55 % ░░░░░░░░░░░░░░░░░ │
│ ░  Donjon Douillet                                         3 h 12 de jeu  ░ │
│ ░  ╭──────────╮ ╭──────────╮        ÉTATS                                  ░ │
│ ░  │    ▶     │ │    ⟲     │        ╭─────╮ ╭─────╮ ╭─────╮ ╭─────╮ ╭─────╮░ │
│ ░  │ Reprendre│ │Recommencer│       │ ▓▓▓ │ │ ▓▓▓ │ │ ▓▓▓ │ │  +  │ │     │░ │
│ ░  ╰──────────╯ ╰──────────╯        │1 14:02│2 hier│3 auto│ │ 4   │ │ 5   │░ │
│ ░  ╭──────────╮ ╭──────────╮        ╰─────╯ ╰─────╯ ╰─────╯ ╰─────╯ ╰─────╯░ │
│ ░  │    💾     │ │    📂     │        ╭─────╮ ╭─────╮ ╭─────╮ ╭─────╮ ╭─────╮░ │
│ ░  │ Sauver   │ │ Charger  │        │  6  │ │  7  │ │  8  │ │  9  │ │ 10  │░ │
│ ░  ╰──────────╯ ╰──────────╯        ╰─────╯ ╰─────╯ ╰─────╯ ╰─────╯ ╰─────╯░ │
│ ░  ╭──────────╮ ╭──────────╮                                               ░ │
│ ░  │    ◐     │ │    ⌨     │        Annuler le dernier chargement  (Ctrl+Z)░ │
│ ░  │  Écran   │ │ Touches  │                                               ░ │
│ ░  ╰──────────╯ ╰──────────╯                                               ░ │
│ ░  ╭──────────╮ ╭──────────╮                                               ░ │
│ ░  │    ✦     │ │    ⌂     │                                               ░ │
│ ░  │ Codes    │ │ Quitter  │                                               ░ │
│ ░  ╰──────────╯ ╰──────────╯                                               ░ │
│ ░ [Ⓐ] Choisir  [Ⓑ] Retour  [Ⓨ] Supprimer l'état  [Ⓧ] Verrouiller           ░ │
└──────────────────────────────────────────────────────────────────────────────┘
```

Icônes : dessinées en primitives egui (traits 2 px arrondis) dans `dessin.rs`, pas de police d'icônes (évite une dépendance et les problèmes de licence).

### 1.10 Transitions récapitulées

| De → vers | Transition |
|---|---|
| Démarrage → bibliothèque | intro 600 ms superposée, passable |
| Bibliothèque → jeu | jaquette qui s'agrandit en écran (220 ms) + lueur qui s'allume |
| Jeu → pause | image figée qui se floute/assombrit (160 ms), tuiles en cascade (30 ms d'écart) |
| Pause → jeu | inverse, 120 ms ; son du jeu reprend en fondu 50 ms (pas de clic) |
| Jeu → bibliothèque | image qui rétrécit dans sa carte ; la carte prend la capture |
| Panneaux (réglages, détail, mappage) | glissement latéral 24 px + fondu 200 ms |
| Chargement d'état | flash blanc 8 % 90 ms + son « ka-chunk » |

---

## 2. Écran de mappage des touches : « la Game Boy sur le bureau »

### 2.1 Illustration

- Une **Game Boy de forme DMG dessinée par nous** en vectoriel (chemins egui : `PathShape`, rectangles arrondis, dégradés par maillage `Mesh` à couleurs par sommet) : boîtier gris chaud `PLASTIQUE`, coin bas droit arrondi caractéristique, cadre d'écran gris ardoise, croix noire, deux boutons ronds baie inclinés, deux pastilles Select/Start obliques, grille de haut-parleur en 6 fentes diagonales, LED.
- **Aucun logo, aucun texte de marque** : pas de « Nintendo », pas de « GAME BOY », pas de « DOT MATRIX WITH STEREO SOUND ». À la place sur le cadre de l'écran : rien, ou « gbemu » en petites capitales discrètes. Les libellés des boutons sont les fonctions génériques (A, B, SELECT, START) placées à côté, comme sur l'original.
- Dessin paramétrique (taille libre, net en 4K), fichier dédié `console.rs` réutilisé par le « Cadre console » de l'écran de jeu.
- Éclairage : dégradé haut-gauche → bas-droit, ombre portée douce sous la console, petit reflet spéculaire sur les boutons (ellipse blanche 20 %).

### 2.2 Interaction

```
┌──────────────────────────────────────────────────────────────────────────────┐
│  Touches                       Profil : [ Clavier ▾ ] [ Manette Xbox (1) ▾ ]  │
│                                                                              │
│              ╭───────────────────────────╮                                   │
│              │ ╭───────────────────────╮ │       HAUT       ↑   Flèche haut  │
│              │ │                       │ │       BAS        ↓   Flèche bas   │
│              │ │     écran (démo du     │ │       GAUCHE     ←   Flèche gauche│
│              │ │     jeu sélectionné    │ │       DROITE     →   Flèche droite│
│              │ │     ou motif)          │ │    ▶  A          X   ← clignote :  │
│              │ ╰───────────────────────╯ │                      « Appuyez sur │
│              │                           │                       une touche… »│
│              │    ┃          ◉ (A)  ◀──── halo baie                  │
│              │  ━━╋━━     ◉ (B)          │       B          Z               │
│              │    ┃                      │       SELECT     Retour arrière   │
│              │        ╱ ╱                │       START      Entrée           │
│              │   SELECT START     ⁄⁄⁄⁄   │                                   │
│              ╰───────────────────────────╯       Raccourcis ▸ (avance, rembo- │
│                                                   binage, états, capture…)   │
│  [Tout configurer]  [Réinitialiser ce profil]  [Effacer la touche]  [Fermer] │
└──────────────────────────────────────────────────────────────────────────────┘
```

- **Cliquer un bouton de la console** (ou le choisir dans la liste à droite, ou le parcourir à la manette) → le bouton s'enfonce (décalage 2 px, ombre réduite), halo `ACCENT_CLAIR` pulsant, bulle « Appuyez sur une touche ou un bouton… (Échap pour annuler, 5 s) ».
- La prochaine touche clavier **ou** bouton/axe de manette est capturée. Sur la console, le bouton affiche brièvement la touche attribuée dans une pastille (style `puce` existant).
- **Survol d'un bouton physique** : appuyer sur une touche déjà attribuée fait s'allumer le bouton correspondant sur la console (test en direct).
- **Tout configurer** : enchaîne Haut, Bas, Gauche, Droite, A, B, Select, Start (comme mGBA « Set all »), la console met en évidence chaque bouton à tour de rôle.
- **Conflits** : si la touche est déjà prise dans le même profil, l'autre bouton perd la touche et clignote orange 1 s, toast « X retirée de B ».
- Plusieurs touches par bouton (jusqu'à 2 au clavier, 2 à la manette).
- Touches réservées non attribuables : Échap (pause) sauf confirmation ; F11 reste libre si l'utilisateur le veut.
- **Profils** : un profil « Clavier » + un profil par modèle de manette, identifié par le GUID SDL que donne gilrs (`Gamepad::uuid()`) avec le nom affiché ; profil « Manette (défaut) » positionnel appliqué à toute manette inconnue. Un sélecteur d'onglets en haut. La manette branchée en dernier est mise en avant automatiquement.
- **Réinitialiser** (profil courant, avec confirmation) et **Effacer la touche**.
- Section repliable **Raccourcis** : liste (pas de dessin) : Pause, Avance rapide (maintenue / basculée), Rembobiner (maintenu), Sauver état rapide, Charger état rapide, Emplacement suivant/précédent, Capture d'écran, Plein écran, Turbo A / Turbo B. Même mécanique de capture.
- Stockage : `gbemu-donnees/touches.json` (versionné, `"version": 1`), écrit via `sauve.rs` (atomique).
- Accès : réglages ▸ Touches, menu pause ▸ Touches, et **à la première connexion d'une manette inconnue** un toast « Nouvelle manette : la configurer ? ».

---

## 3. Save states

### 3.1 Cœur (`gb-core`, prérequis)

- `GameBoy::save_state(&self) -> Vec<u8>` et `GameBoy::load_state(&mut self, &[u8]) -> Result<(), StateError>`.
- Format maison simple et versionné, **sans dépendance** (gb-core reste sans dépendance) : en-tête `b"GBST"`, version u16, CRC32 de la ROM (refus si autre jeu), puis blocs étiquetés (`CPU `, `BUS `, `PPU `, `APU `, `TIM `, `CART`, `RAM `…) longueur + données, petit-boutiste. Un bloc inconnu est sauté (compatibilité future). Option ultérieure : export BESS (format de SameBoy, interopérable) — non prioritaire.
- Tout l'état : registres, WRAM, HRAM, VRAM, OAM, E/S, timer interne (DIV 16 bits), PPU (ligne, point, mode, fenêtre), APU (canaux, séquenceur, position d'onde), DMA, série, cartouche (banques, RAM externe, RTC + horodatage), joypad. **Pas** la ROM.
- Charge utile ~ 40-60 Ko non compressé (DMG). Aucune panique sur données tronquées ou corrompues (test fuzz léger dans `tests/robustesse.rs`).
- Critère : `save_state` → N images → `load_state` → N images donne exactement le même `frame()` et les mêmes échantillons audio (test déterministe sur dmg-acid2 et une ROM blargg).

### 3.2 Application

- **10 emplacements par jeu** + 1 emplacement **Auto** (reprise) + 1 **tampon d'annulation**, dossier `gbemu-donnees/etats/<nom> [CRC]/` :
  `1.state` … `10.state`, `auto.state`, `annule.state`, et pour chacun `N.png` (miniature 160×144 de l'image au moment de la sauvegarde) et `N.json` (date locale, temps de jeu cumulé, verrou, nom libre facultatif).
- Écriture atomique (`sauve.rs` : tmp + rename) ; état précédent gardé en `.bak` comme pour les `.sav`.
- **Raccourcis** (modifiables dans le mappage) :
  - `F5` sauvegarde rapide dans l'emplacement courant, `F8` chargement rapide de l'emplacement courant ;
  - `F6` / `F7` emplacement précédent / suivant (indicateur « ÉTAT 3 » + miniature 1,2 s) ;
  - `Maj+F1…F10` sauver dans l'emplacement 1…10, `F1…F10` charger (façon mGBA ; F11 reste plein écran, F2 réglages hors jeu) ;
  - `Ctrl+Z` annuler le dernier chargement (recharge `annule.state`, qui est écrit juste **avant** chaque chargement) ; `Ctrl+Maj+Z` annuler la dernière sauvegarde (restaure le `.bak` de l'emplacement).
  - Manette : Select+Start = pause (existant) ; dans le menu pause, tuiles Sauver/Charger. Raccourcis manette optionnels (ex. Select+R1 sauver, Select+L1 charger).
- **Sauvegarde auto à la fermeture** (quitter la partie, fermer la fenêtre, perte de focus prolongée non) → `auto.state` + miniature. **Au relancement** du jeu : carte « Reprendre là où vous étiez ? (capture, date) » avec `Reprendre` (défaut) / `Recommencer depuis la sauvegarde du jeu`. Réglage : « Reprise automatique : Demander / Toujours / Jamais ».
- La sauvegarde batterie `.sav` reste **la référence** et continue d'être écrite comme aujourd'hui (le chargement d'un état ne doit jamais écraser un `.sav` plus récent sans passage par la logique existante : on recharge la RAM externe de l'état en mémoire, et le `.sav` sera réécrit au cycle normal ; on le documente dans l'aide : « les états ne remplacent pas la sauvegarde du jeu »).
- **Emplacement verrouillé** (Delta) : ne peut pas être écrasé par F5 ni Maj+Fn ; toast « Emplacement 2 verrouillé ».
- Grille des emplacements (menu pause et panneau détail de la bibliothèque) : miniature, numéro en Silkscreen, date relative (« il y a 5 min », « hier 21:14 »), cadenas ; vide = « + ». Y = supprimer (avec confirmation), X = verrouiller.
- Chargement d'un état d'une version de format plus ancienne inconnue → message clair, jamais de plantage.

---

## 4. Autres fonctionnalités, priorisées

Coût : S ≤ ½ journée agent, M ≈ 1 journée, L > 1 journée. Cœur : ce que `gb-core` doit fournir.

| # | Fonctionnalité | Utilité | Coût | Dépendance cœur |
|---|---|---|---|---|
| 1 | **Save states** (§3) | La fonctionnalité la plus demandée | L (cœur M + app M) | `save_state/load_state` |
| 2 | **Mappage illustré + profils** (§2) | Demande explicite, confort manette | M | aucune |
| 3 | **Reprise automatique** | Ouvrir l'app = reprendre en 1 touche | S (après 1) | save states |
| 4 | **Rembobinage maintenu** (touche, ~30 s, réglable 10-60 s) | Effet « waouh », pardonne les erreurs | M | save states ; tampon circulaire d'états toutes les 2 images, différentiel XOR + compression RLE dans l'app (~5 Mo pour 60 s) ; son joué à l'envers ou coupé |
| 5 | **Avance rapide réglable** (×2, ×3, ×4, ×8, illimité ; maintenue ou basculée) + ralenti ×0,5 | Grinding, dialogues | S | aucune (existe à ×4) ; son : accéléré-sans-hausse ou coupé (option) |
| 6 | **Filtres d'écran** : Net, Lisse (bilinéaire), **Grille LCD** (lignes entre pixels, façon modes « Original » de l'Analogue Pocket), **Rémanence** (mélange avec l'image précédente 50 % : jeux qui clignotent), **Rétroéclairé** (palette éclaircie + halo + léger flou lumineux) ; shader GLSL via `egui_glow::CallbackFn` | Rendu premium, fidélité | M | aucune |
| 7 | **Palettes DMG** : Verte (DMG `#0F380F #306230 #8BAC0F #9BBC0F`), Poche (gris neutres), Lumière (sarcelle rétroéclairée), Gris pur, Sépia, Nuit (inversée douce), + éditeur 4 couleurs ; **palette par jeu** | Personnalisation, nostalgie | S (éditeur : M) | aucune (le cœur sort des indices 0-3 ? sinon conversion app) |
| 8 | **Captures d'écran** (F12 / HUD) : PNG à l'échelle ×1 et ×4 (option), dossier `captures/`, toast avec aperçu ; copie dans le presse-papiers | Partage | S | `frame()` existant |
| 9 | **Favoris, récents, temps de jeu** (compteur par CRC, `stats.json`) + rangée « Reprendre » | Bibliothèque vivante | S | aucune |
| 10 | **Mode TV / manette complet** : tout accessible sans clavier, aide en pied adaptée (glyphes Xbox / PlayStation / Switch selon la manette détectée), grandes tailles en plein écran, carrousel des plateformes | Jouer au canapé | M | aucune |
| 11 | **Thèmes Nuit / Jour** (+ « Suivre le système » sous Windows) | Confort, premium | S (si jetons respectés) | aucune |
| 12 | **Tri, recherche avancée** (tri nom/récent/temps/année ; recherche floue tolérante aux accents, déjà en partie) | Grandes bibliothèques | S | aucune |
| 13 | **Panneau détail du jeu** (§1.7) | Point d'entrée états/palette/infos | M | aucune |
| 14 | **Cheats Game Genie / GameShark** : liste par jeu, activation case à cocher, import d'un fichier texte | Demandé dans tous les émulateurs | M | Game Genie = substitution à la lecture ROM (adresse + valeur + comparaison) ; GameShark = écriture RAM à chaque image (`poke` à ajouter) |
| 15 | **Réglages audio** : volume général, coupure en arrière-plan, latence (basse/normale/sûre), coupure en avance rapide, volume des sons d'interface | Confort | S | aucune |
| 16 | **Multi-langue FR / EN** : toutes les chaînes dans `textes.rs` (table clé → FR/EN), détection de la langue système | Diffusion | M (extraction des chaînes) | aucune |
| 17 | **Jaquettes** : choisir une image locale par glisser-déposer sur la carte, re-télécharger, option « styles : jaquette / capture du titre / cartouche » | Bibliothèque belle même sans réseau | S | aucune |
| 18 | **Plein écran soigné** : sans bordure, curseur masqué après 2 s, mise à l'échelle entière / remplir / étirer, rotation non | Base | S | aucune |
| 19 | **Turbo A / B** (tir automatique, fréquence réglable) | Jeux d'action | S | aucune |
| 20 | **Paramètres par jeu** (palette, filtre, vitesse, profil de touches) | Pizza Boy le fait | S | aucune |
| 21 | **Enregistrement GIF / vidéo** courte (dernières 10 s en GIF) | Partage réseaux | M | aucune |
| 22 | **Correction couleur GBC** (façon SameBoy) | Pour la future étape GBC | S | GBC |
| 23 | Vibration (cartouches rumble MBC5), câble link local, Game Boy Printer | Rare | L | cœur |
| 24 | Succès RetroAchievements | Communauté | L | `peek` + bibliothèque rcheevos (C) |

Hors champ : téléchargement de ROM, jaquettes autres que libretro-thumbnails, toute marque Nintendo.

---

## 5. Découpage en lots parallèles

Contrainte : `app.rs` fait 1 866 lignes et touche à tout. Pour que 3-4 agents travaillent en même temps sans conflits, **un court Lot 0 séquentiel** le découpe d'abord, puis chaque lot vit surtout dans ses propres fichiers avec de petits points d'accroche.

### Lot 0 (séquentiel, préalable, ~S-M) : découper `app.rs` sans changer le comportement
- `app.rs` → garde `App`, la boucle `ui`, l'état global et l'aiguillage.
- Nouveaux : `ecran_biblio.rs` (`entrees_biblio`, `dessiner_biblio`, `carte`, `dessiner_vide`), `ecran_jeu.rs` (`dessiner_jeu`, entrées de jeu), `menus.rs` (`Menu`, `GenreMenu`, `Act`, `elements`, `agir`, `entrees_menu`, `dessiner_menu`), `toast.rs`.
- Ajout d'une table d'actions `Action` (Pause, AvanceRapide, SauverEtat(n)…) consultée par les entrées : point d'accroche des raccourcis.
- Critères : captures identiques avant/après (mêmes commandes `--capture`), `cargo clippy -D warnings` vert (Linux + windows-gnu).

### Lot A : cœur — états et rembobinage (`gb-core` seul)
- Fichiers : `gb-core/src/etat.rs` (nouveau : écrivain/lecteur de blocs), `lib.rs` (`save_state`, `load_state`, `StateError`), `cpu.rs`, `bus.rs`, `ppu.rs`, `apu.rs`, `timer.rs`, `cartridge.rs` (méthodes `ecrire_etat/lire_etat` par module), `gb-core/tests/etats.rs`, `tests/robustesse.rs` ; + `poke(addr, val)` et crochet de lecture ROM pour cheats (Game Genie) si le temps le permet.
- `gb-headless` : options `--save-state f --load-state f` pour les tests.
- Indépendant de l'interface ; ne touche pas `gb-app`.

### Lot B : direction visuelle (`gb-app`, rendu)
- Fichiers : `dessin.rs` (jetons Nuit/Jour, ombres, grain, vignettage, courbes et durées d'animation, icônes vectorielles), `ecran_biblio.rs` (rangée Reprendre, onglets Récents/Favoris, tri, panneau détail, halo, carte de repli corrigée), `menus.rs` (menu pause en tuiles + verre dépoli, surbrillance unique), `intro.rs` (nouveau), `sons_ui.rs` (nouveau, sons synthétisés), `stats.rs` (nouveau : favoris, temps de jeu, récents), `assets/fonts/Silkscreen-Regular.ttf` (+ licence OFL).
- La zone « États » du menu pause est laissée en emplacement réservé (fonction `dessiner_grille_etats` fournie par le Lot D).

### Lot C : touches, manettes, mappage illustré
- Fichiers : `console.rs` (nouveau : Game Boy vectorielle paramétrique, boutons cliquables, état enfoncé/allumé), `touches.rs` (nouveau : profils, `touches.json`, capture clavier/manette, conflits, raccourcis), `ecran_touches.rs` (nouveau : l'écran §2), `manette.rs` (identification par GUID, nom, glyphes Xbox/PS/Switch pour le pied d'aide), accroche minimale dans `menus.rs` (entrée « Touches ») et `app.rs` (routage de l'écran).
- Fournit aussi turbo A/B et la table des raccourcis utilisée par les Lots B et D.

### Lot D : états dans l'app, rembobinage, avance, captures, filtres
- Dépend de l'**API** du Lot A (signatures fixées dès le départ ; l'agent D code contre `save_state/load_state` et peut tester avec un faux état tant que A n'est pas fusionné).
- Fichiers : `etats.rs` (nouveau : emplacements 1-10, auto, annulation, verrou, miniatures PNG, json), `rembobinage.rs` (nouveau : tampon circulaire différentiel), `jeu.rs` (vitesses ×0,5…illimitée, reprise auto, sauvegarde auto à la fermeture), `ecran_jeu.rs` (HUD flottant, indicateurs, captures F12, filtres d'écran + shader `egui_glow`, cadre console via `console.rs` du Lot C ou en simple cadre si pas encore fusionné), `menus.rs` (grille d'états : `dessiner_grille_etats`).

### Ordre et fusion
1. Lot 0 seul (rapide).
2. A, B, C en parallèle (worktrees séparés) ; D démarre en parallèle sur la partie avance/captures/filtres, puis branche les états quand A est fusionné.
3. Ensuite, lots plus petits : cheats (cœur + `cheats.rs`), multi-langue (`textes.rs`, extraction de toutes les chaînes — à faire **après** B pour ne pas traduire deux fois), GIF, paramètres par jeu.

### Vérifications communes à chaque lot
- `cargo fmt`, `cargo clippy --all-targets -- -D warnings` (Linux et `--target x86_64-pc-windows-gnu`), `cargo test`.
- Captures sous Xvfb pour chaque écran touché (bibliothèque, jeu, pause, réglages, touches, états) dans `out/captures/`, relues par l'orchestrateur ; ajouter les options `--menu touches|etats` à `--capture`.
- Démarrage toujours < 2 s (mesuré), aucune panique sur état ou `touches.json` corrompu, sauvegardes `.sav` jamais perdues.
