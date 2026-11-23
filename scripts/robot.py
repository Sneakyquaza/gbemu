#!/usr/bin/env python3
"""Utilisateurs simulés à grande échelle (`gbemu --robot`).

  python3 -I scripts/robot.py lancer --sortie DOSSIER [--debut 0] [--courtes 5000]
        [--duree-courte 20] [--longues 300] [--duree-longue 300] [--coeurs 6]
  python3 -I scripts/robot.py bilan DOSSIER
  python3 -I scripts/robot.py reduire TRACE.json [--sortie MIN.json]

Chaque session tourne dans son dossier (DOSSIER/sessions/<graine>, effacé par le robot si elle
réussit), sous `nice -n 10`, dans un espace réseau vide (`unshare -rn` : aucune requête ne
sort). Les bilans (une ligne JSON par session) vont dans DOSSIER/bilans.jsonl, les traces des
échecs dans DOSSIER/traces/<graine>.json (rejouables : `gbemu --robot 0 --rejouer F`).
"""

import argparse
import collections
import json
import os
import re
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor

RACINE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.environ.get("GBEMU_EXE") or os.path.join(RACINE, "target", "release", "gbemu")
ROMS = os.environ.get("GBEMU_ROMS", os.path.join(RACINE, "roms"))


def commande(args):
    base = ["nice", "-n", "10"]
    if subprocess.run(["unshare", "-rn", "true"], capture_output=True).returncode == 0:
        base += ["unshare", "-rn"]
    return base + [EXE] + args


def session(sortie, graine, duree):
    d = os.path.join(sortie, "sessions", str(graine))
    trace = os.path.join(sortie, "traces", f"{graine}.json")
    args = ["--robot", str(graine), "--duree", str(duree), "--dossier", d,
            "--trace", trace]
    if os.path.isdir(ROMS):
        args += ["--roms-test", ROMS]
    try:
        r = subprocess.run(commande(args), capture_output=True, text=True,
                           timeout=max(600, duree * 4), cwd=sortie)
        ligne = (r.stdout.strip().splitlines() or [""])[-1]
        b = json.loads(ligne)
    except subprocess.TimeoutExpired:
        b = {"graine": graine, "ok": False, "genre": "delai", "message": "session > délai"}
    except (json.JSONDecodeError, IndexError):
        b = {"graine": graine, "ok": False, "genre": "sortie",
             "message": f"code {r.returncode} : {r.stderr[-400:]}"}
    b["duree_demandee"] = duree
    # Image lente : la trace est rejouée deux fois ; jamais reproduite, c'est le serveur (autres
    # programmes) et non l'appli. Compté à part dans le bilan.
    if not b["ok"] and b["genre"] == "image-lente" and os.path.exists(trace):
        t = json.load(open(trace))
        if all(rejouer(t)["ok"] for _ in range(2)):
            b.update(ok=True, bruit=b["message"].splitlines()[0])
    return b


def lancer(a):
    os.makedirs(os.path.join(a.sortie, "sessions"), exist_ok=True)
    os.makedirs(os.path.join(a.sortie, "traces"), exist_ok=True)
    taches = [(a.debut + i, a.duree_courte) for i in range(a.courtes)]
    taches += [(a.debut + a.courtes + i, a.duree_longue) for i in range(a.longues)]
    # Longues d'abord : la fin du lot reste bien remplie.
    taches.sort(key=lambda t: -t[1])
    fichier = os.path.join(a.sortie, "bilans.jsonl")
    with open(fichier, "a") as f, ThreadPoolExecutor(a.coeurs) as ex:
        for i, b in enumerate(ex.map(lambda t: session(a.sortie, *t), taches)):
            f.write(json.dumps(b, ensure_ascii=False) + "\n")
            f.flush()
            if not b["ok"]:
                print(f"ÉCHEC {b['graine']} {b.get('profil')} {b['genre']} : "
                      f"{b['message'].splitlines()[0] if b['message'] else ''}", flush=True)
            if (i + 1) % 500 == 0:
                print(f"{i + 1}/{len(taches)} sessions", flush=True)
    bilan(fichier)


def signature(b):
    m = (b.get("message") or "").splitlines()
    m = m[0] if m else ""
    if b["genre"] == "panique":
        # Emplacement de la panique (fichier:ligne), sans le détail des valeurs.
        loc = re.search(r"at ([^\s]+:\d+):\d+", b["message"])
        m = loc.group(1) if loc else m
    return b["genre"], re.sub(r"\d+", "N", m)[:120]


def bilan(fichier):
    if os.path.isdir(fichier):
        fichier = os.path.join(fichier, "bilans.jsonl")
    L = [json.loads(l) for l in open(fichier) if l.strip()]
    ok = sum(1 for b in L if b["ok"])
    sim = sum(b.get("secondes_simulees", 0) for b in L) / 3600
    bruit = sum(1 for b in L if b.get("bruit"))
    print(f"{len(L)} sessions, {ok} réussies, {len(L) - ok} échecs, {sim:.1f} h simulées, "
          f"{bruit} images lentes non reproduites au rejeu (serveur), "
          f"pire image {max((b.get('pire_image_ms', 0) for b in L), default=0):.0f} ms, "
          f"mémoire max {max((b.get('rss_max_mo', 0) for b in L), default=0)} Mo, "
          f"{sum(b.get('rouvertures', 0) for b in L)} réouvertures, "
          f"{sum(b.get('parties', 0) for b in L)} parties")
    par_profil = collections.Counter(b.get("profil") for b in L)
    print("profils :", dict(par_profil))
    groupes = collections.defaultdict(list)
    for b in L:
        if not b["ok"]:
            groupes[signature(b)].append(b["graine"])
    for (g, m), v in sorted(groupes.items(), key=lambda x: -len(x[1])):
        print(f"  {len(v):5} × {g} : {m}  (graines {v[:6]})")


def rejouer(trace):
    with tempfile.TemporaryDirectory() as d:
        f = os.path.join(d, "t.json")
        with open(f, "w") as o:
            json.dump(trace, o)
        args = ["--robot", "0", "--rejouer", f, "--dossier", os.path.join(d, "s")]
        if os.path.isdir(ROMS):
            args += ["--roms-test", ROMS]
        r = subprocess.run(commande(args), capture_output=True, text=True, timeout=600, cwd=d)
        try:
            return json.loads(r.stdout.strip().splitlines()[-1])
        except (json.JSONDecodeError, IndexError):
            return {"ok": False, "genre": "sortie", "message": r.stderr[-300:]}


def reduire(a):
    """Réduction par dichotomie (ddmin) : retire des gestes tant que l'échec persiste."""
    trace = json.load(open(a.trace))
    b = rejouer(trace)
    if b["ok"]:
        print("échec non reproduit au rejeu")
        return
    cible = signature(b)
    print("échec :", cible)
    # Durée ramenée à l'échec (+ 1 s).
    trace["duree"] = min(trace["duree"], b.get("secondes_simulees", trace["duree"]) + 1.0)
    g = [x for x in trace["gestes"] if x[0] <= b.get("image", 1 << 60)]
    n = 2
    while len(g) >= 2:
        taille = max(1, len(g) // n)
        reduit = False
        for i in range(0, len(g), taille):
            essai = g[:i] + g[i + taille:]
            r = rejouer(dict(trace, gestes=essai))
            if not r["ok"] and signature(r) == cible:
                g, n, reduit = essai, max(n - 1, 2), True
                print(f"  {len(g)} gestes", flush=True)
                break
        if not reduit:
            if taille == 1:
                break
            n = min(len(g), n * 2)
    trace["gestes"] = g
    sortie = a.sortie or a.trace.replace(".json", ".min.json")
    json.dump(trace, open(sortie, "w"), ensure_ascii=False, indent=1)
    print(f"trace réduite : {len(g)} gestes → {sortie}")


def main():
    p = argparse.ArgumentParser()
    s = p.add_subparsers(dest="cmd", required=True)
    l = s.add_parser("lancer")
    l.add_argument("--sortie", required=True)
    l.add_argument("--debut", type=int, default=0)
    l.add_argument("--courtes", type=int, default=5000)
    l.add_argument("--duree-courte", type=float, default=20)
    l.add_argument("--longues", type=int, default=300)
    l.add_argument("--duree-longue", type=float, default=300)
    l.add_argument("--coeurs", type=int, default=6)
    b = s.add_parser("bilan")
    b.add_argument("fichier")
    r = s.add_parser("reduire")
    r.add_argument("trace")
    r.add_argument("--sortie")
    a = p.parse_args()
    a.sortie = os.path.abspath(a.sortie) if getattr(a, "sortie", None) else None
    {"lancer": lancer, "bilan": lambda a: bilan(a.fichier), "reduire": reduire}[a.cmd](a)


if __name__ == "__main__":
    sys.exit(main())
