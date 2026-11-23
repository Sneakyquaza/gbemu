#!/usr/bin/env python3
"""Fichier de métadonnées intégré à l'exe : `gb-app/donnees/jeux.bin`.

Sources (toutes CC BY-SA, attribution dans LICENCES et dans la fiche) :
- libretro-database `metadat/` : noms No-Intro, région, année/mois, développeur, éditeur, genre,
  joueurs, ESRB, par CRC ;
- Wikidata (SPARQL) : jeux des plateformes Game Boy (Q186437), Game Boy Color (Q203992),
  Game Boy Advance (Q188642), lien frwiki (enwiki en repli) ;
- Wikipédia (API REST page/summary) : introduction réduite à 1-3 phrases.

Lancer avec `python3 -I scripts/donnees-jeux.py` depuis la racine du dépôt.
Cache (reprise après coupure) : `references/donnees-jeux/` (ignoré par git).
Rapport d'appariement : `references/donnees-jeux/rapport.txt`.
"""
import json, os, re, struct, sys, time, unicodedata, urllib.error, urllib.parse, urllib.request, zlib

RACINE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CACHE = os.path.join(RACINE, "references", "donnees-jeux")
SORTIE = os.path.join(RACINE, "gb-app", "donnees", "jeux.bin")
UA = "gbemu-dataset/0.1 (https://github.com/Sneakyquaza/gbemu)"
DEPOT = "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat"
SYSTEMES = [(0, "Nintendo - Game Boy", "Q186437"),
            (1, "Nintendo - Game Boy Color", "Q203992"),
            (2, "Nintendo - Game Boy Advance", "Q188642")]
METADAT = ["releaseyear", "releasemonth", "developer", "publisher", "genre", "maxusers", "esrb"]
CLES = {"releaseyear": "releaseyear", "releasemonth": "releasemonth", "developer": "developer",
        "publisher": "publisher", "genre": "genre", "maxusers": "users", "esrb": "esrb_rating"}
PAUSE = 0.25  # ≤ 4 requêtes/s vers Wikipédia

_derniere = [0.0]


def http(url, accept="application/json", essais=6):
    """GET avec pause entre requêtes et reprise sur erreur (429/5xx/réseau). None si 404."""
    for n in range(essais):
        attente = PAUSE - (time.monotonic() - _derniere[0])
        if attente > 0:
            time.sleep(attente)
        _derniere[0] = time.monotonic()
        req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept": accept})
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                return r.read()
        except urllib.error.HTTPError as e:
            if e.code == 404:
                return None
            if e.code not in (429, 500, 502, 503, 504):
                raise
            pause = int(e.headers.get("Retry-After") or 0) or 5 * 2 ** n
        except (urllib.error.URLError, TimeoutError, ConnectionError):
            pause = 5 * 2 ** n
        print(f"  reprise dans {pause} s : {url[:90]}", file=sys.stderr)
        time.sleep(min(pause, 120))
    raise RuntimeError(f"échec : {url}")


def en_cache(chemin, obtenir):
    """Contenu du cache, sinon `obtenir()` écrit dans le cache (écriture atomique)."""
    if os.path.exists(chemin):
        with open(chemin, "rb") as f:
            return f.read()
    d = obtenir()
    if d is None:
        return None
    os.makedirs(os.path.dirname(chemin), exist_ok=True)
    with open(chemin + ".tmp", "wb") as f:
        f.write(d)
    os.replace(chemin + ".tmp", chemin)
    return d


# ---------------------------------------------------------------- libretro-database

JETON = re.compile(r'"([^"]*)"|([()])|([^\s()"]+)')


def blocs_jeu(texte):
    """Dictionnaires des blocs `game ( … )` (champs du sous-bloc `rom` compris, 1re valeur)."""
    j = [(m.group(1) if m.group(1) is not None else m.group(3), m.group(2))
         for m in JETON.finditer(texte)]
    i = 0
    while i + 1 < len(j):
        if j[i][0] == "game" and j[i + 1][1] == "(":
            i += 2
            prof, champs = 1, {}
            while i < len(j) and prof > 0:
                mot, par = j[i]
                if par == "(":
                    prof += 1
                elif par == ")":
                    prof -= 1
                elif i + 1 < len(j) and j[i + 1][0] is not None:
                    champs.setdefault(mot, j[i + 1][0])
                    i += 1
                i += 1
            yield champs
        else:
            i += 1


def libretro():
    jeux = {}
    for pf, systeme, _ in SYSTEMES:
        def dat(sous):
            url = f"{DEPOT}/{sous}/{urllib.parse.quote(systeme)}.dat"
            chemin = os.path.join(CACHE, "libretro", sous, systeme + ".dat")
            return en_cache(chemin, lambda: http(url, "text/plain")).decode("utf-8", "replace")
        for c in blocs_jeu(dat("no-intro")):
            try:
                crc = int(c["crc"], 16)
            except (KeyError, ValueError):
                continue
            jeux[crc] = {"pf": pf, "nom": c.get("name", ""), "region": c.get("region", "")}
        for sous in METADAT:
            cle = CLES[sous]
            for c in blocs_jeu(dat(sous)):
                try:
                    j = jeux.get(int(c.get("crc", ""), 16))
                except ValueError:
                    continue
                if j is not None and cle in c:
                    # « Nintendo ??? » : studio incertain chez libretro, la société seule reste.
                    j.setdefault(sous, re.sub(r"\s*\?+$", "", c[cle].strip()))
    return jeux


# ---------------------------------------------------------------- Wikidata

SPARQL = """SELECT ?i ?en ?mul ?fr ?frt ?ent (GROUP_CONCAT(DISTINCT ?al; separator="|") AS ?als) WHERE {
  ?i wdt:P400 wd:%s .
  FILTER NOT EXISTS { ?i wdt:P31 wd:Q7058673 }
  OPTIONAL { ?i rdfs:label ?en FILTER(lang(?en) = "en") }
  OPTIONAL { ?i rdfs:label ?mul FILTER(lang(?mul) = "mul") }
  OPTIONAL { ?i rdfs:label ?fr FILTER(lang(?fr) = "fr") }
  OPTIONAL { ?fa schema:about ?i; schema:isPartOf <https://fr.wikipedia.org/>; schema:name ?frt }
  OPTIONAL { ?ea schema:about ?i; schema:isPartOf <https://en.wikipedia.org/>; schema:name ?ent }
  OPTIONAL { ?i skos:altLabel ?al FILTER(lang(?al) IN ("en", "fr", "mul")) }
} GROUP BY ?i ?en ?mul ?fr ?frt ?ent"""


def wikidata():
    """{qid: {"pf": {0,1,2}, "noms": [...], "fr": titre frwiki, "en": titre enwiki}}"""
    elements = {}
    for pf, _, qid in SYSTEMES:
        url = "https://query.wikidata.org/sparql?" + urllib.parse.urlencode(
            {"query": SPARQL % qid, "format": "json"})
        brut = en_cache(os.path.join(CACHE, "wikidata", qid + ".json"),
                        lambda: http(url, "application/sparql-results+json"))
        for b in json.loads(brut)["results"]["bindings"]:
            v = {k: x["value"] for k, x in b.items()}
            q = v["i"].rsplit("/", 1)[-1]
            e = elements.setdefault(q, {"pf": set(), "noms": [], "alias": [], "fr": None,
                                        "en": None})
            e["pf"].add(pf)
            for k in ("en", "mul", "fr"):
                if k in v and v[k] not in e["noms"]:
                    e["noms"].append(v[k])
            for k, t in (("frt", "fr"), ("ent", "en")):
                if k in v:
                    e[t] = e[t] or v[k]
                    nom = re.sub(r"\s*\((jeu vidéo|video game)[^)]*\)$", "", v[k])
                    if nom not in e["noms"]:
                        e["noms"].append(nom)
            for a in filter(None, v.get("als", "").split("|")):
                if a not in e["noms"] and a not in e["alias"]:
                    e["alias"].append(a)
    return elements


# ---------------------------------------------------------------- appariement

ARTICLES = {"the", "a", "an", "le", "la", "les", "l", "un", "une"}
REMPLISSAGE = {"version", "edition", "jeu", "game"}
ROMAINS = {"ii": "2", "iii": "3", "iv": "4", "v": "5", "vi": "6", "vii": "7", "viii": "8",
           "ix": "9", "xi": "11", "xii": "12", "xiii": "13"}
NON_COMMERCIAL = re.compile(r"\((Beta|Proto|Demo|Sample|Kiosk|Unl|Pirate|Aftermarket|Test Program|"
                            r"Program|Debug|Enhancement Chip)[^)]*\)|\[BIOS\]|BIOS", re.I)


def mots(s):
    s = unicodedata.normalize("NFKD", s)
    s = "".join(c for c in s if not unicodedata.combining(c)).lower()
    s = re.sub(r"\([^)]*\)|\[[^]]*\]", " ", s)
    s = s.replace("&", " and ").replace("+", " plus ").replace("'", " ")
    m = [ROMAINS.get(t, t) for t in re.split(r"[^a-z0-9]+", s) if t]
    return [t for t in m if t not in ARTICLES]


def cles(nom):
    """Clés d'appariement (stricte, puis sans mots de remplissage), sans doublon."""
    m = mots(nom)
    k = [" ".join(m), " ".join(t for t in m if t not in REMPLISSAGE)]
    return [x for i, x in enumerate(k) if x and x not in k[:i]]


def variantes(nom):
    """« Pokémon Rouge et Bleu » → « Pokémon Rouge », « Pokémon Bleu » (paires de versions)."""
    m = mots(nom)
    if len(m) >= 4 and m[-2] in ("and", "et"):
        return [" ".join(m[:-3] + [m[-3]]), " ".join(m[:-3] + [m[-1]])]
    return []


def titre_nointro(nom):
    """« Legend of Zelda, The - Link's Awakening (USA) » → « Legend of Zelda - Link's Awakening »."""
    base = nom.split(" (")[0].strip()
    return re.sub(r", (The|A|An|Le|La|Les|L'|Die|Der|Das|El|Il)\b", "", base)


def apparier(jeux, elements):
    # index[pf][clé] = {qid: rang} : 0 = libellé exact, +1 = sans remplissage, +2 = alias,
    # 3 = variante de paire.
    index = {pf: {} for pf, _, _ in SYSTEMES}
    for q, e in elements.items():
        noms = [(n, 0) for n in e["noms"]] + [(n, 2) for n in e["alias"]]
        for nom, base in noms:
            ks = [(k, base + r) for r, k in enumerate(cles(nom))] + [(k, 3) for k in variantes(nom)]
            for pf in e["pf"]:
                for k, r in ks:
                    d = index[pf].setdefault(k, {})
                    d[q] = min(r, d.get(q, 9))
    voisins = {0: [0, 1], 1: [1, 0], 2: [2]}
    resultat, doutes = {}, []
    for crc, j in jeux.items():
        titre = titre_nointro(j["nom"])
        trouve = None
        for pf in voisins[j["pf"]]:
            for rang_cle, k in enumerate(cles(titre)):
                cand = index[pf].get(k)
                if not cand:
                    continue
                meilleur = min(cand.values())
                qs = sorted(q for q, r in cand.items() if r == meilleur)
                if len(qs) > 1:
                    # Plusieurs éléments : celui qui a un article frwiki, s'il est seul.
                    avec_fr = [q for q in qs if elements[q]["fr"]]
                    if len(avec_fr) != 1:
                        doutes.append(f"ambigu   {j['nom']} -> {', '.join(qs)}")
                        trouve = "ambigu"
                        break
                    qs = avec_fr
                trouve = (qs[0], rang_cle + meilleur + (pf != j["pf"]))
                break
            if trouve:
                break
        if isinstance(trouve, tuple):
            q, ecart = trouve
            resultat[crc] = q
            if ecart:
                doutes.append(f"approché {j['nom']} -> {q} {elements[q]['noms'][:1]}")
    return resultat, doutes


# ---------------------------------------------------------------- Wikipédia

ABREV = re.compile(r"\b(?:[A-Z]|Inc|Co|Ltd|Corp|Jr|Sr|St|Dr|Mr|vs|etc|env|av|apr|J\.-C)\.$")
NON_LATIN = re.compile(r"[Ͱ-ϿЀ-ӿ　-鿿가-힯＀-￯]")


def nettoyer(texte):
    """Introduction réduite à 1-3 phrases (≤ ~420 caractères), parenthèses en écriture non
    latine (titre japonais, transcription) retirées."""
    t = re.sub(r"\s+", " ", texte or "").strip()
    prec = None
    while prec != t:
        prec = t
        t = re.sub(r"\s*\((?=[^()]*" + NON_LATIN.pattern + r")[^()]*\)", "", t)
    t = re.sub(r"\s*\(\s*[,;]?\s*\)", "", t)
    t = re.sub(r"\s+([,.;:])", r"\1", t).strip()
    morceaux = re.split(r"(?<=[.!?…])\s+(?=[A-ZÀ-ÖØ-Þ«\"])", t)
    phrases = []
    for m in morceaux:
        if phrases and ABREV.search(phrases[-1]):
            phrases[-1] += " " + m
        else:
            phrases.append(m)
    sortie = ""
    for p in phrases[:3]:
        if sortie and len(sortie) + len(p) > 420:
            break
        sortie = (sortie + " " + p).strip()
    if len(sortie) > 600:
        sortie = sortie[:597].rsplit(" ", 1)[0] + "…"
    return sortie


def resume(langue, titre):
    """(texte nettoyé, titre canonique) ou None (page absente, homonymie, vide)."""
    t = urllib.parse.quote(titre.replace(" ", "_"), safe="")
    url = f"https://{langue}.wikipedia.org/api/rest_v1/page/summary/{t}?redirect=true"
    nom = re.sub(r"[^\w.-]", "_", titre)[:150]
    brut = en_cache(os.path.join(CACHE, "wiki", langue, nom + ".json"), lambda: http(url) or b"{}")
    d = json.loads(brut or b"{}")
    if d.get("type") != "standard":
        return None
    canonique = d.get("titles", {}).get("canonical", titre).replace("_", " ")
    # Lien Wikidata vers une redirection (section d'une liste, d'une série, d'un film…) ou
    # vers une liste : l'article ne parle pas que de ce jeu.
    plat = lambda x: re.sub(r"\W", "", x.lower())
    if plat(canonique) != plat(titre) or re.match(r"List of |Liste d", canonique):
        return None
    texte = nettoyer(d.get("extract", ""))
    if len(texte) < 40:
        return None
    return texte, canonique


# ---------------------------------------------------------------- fichier intégré

def ecrire(jeux, resumes):
    """Format (petit-boutiste) : « GBJX », version u8, puis flux zlib de :
    u32 nb de chaînes, chaînes (u16 longueur + UTF-8) ; u32 nb de jeux, par jeu :
    u32 CRC, u8 plateforme, u16 année (0 = inconnue), u8 mois, u8 joueurs, 8 × u32 indices de
    chaînes (nom, région, développeur, éditeur, genre, ESRB, résumé, source « fr:Titre »)."""
    chaines, idx = [], {}

    def s(x):
        x = x or ""
        if x not in idx:
            idx[x] = len(chaines)
            chaines.append(x)
        return idx[x]

    s("")
    corps = bytearray()
    for crc in sorted(jeux):
        j = jeux[crc]
        ent = lambda k, lo, hi: (lambda v: v if v is not None and lo <= v <= hi else 0)(
            int(j[k]) if j.get(k, "").isdigit() else None)
        r = resumes.get(crc, ("", ""))
        corps += struct.pack("<IBHBB", crc, j["pf"], ent("releaseyear", 1980, 2100),
                             ent("releasemonth", 1, 12), ent("maxusers", 1, 255))
        corps += struct.pack("<8I", s(j["nom"]), s(j["region"]), s(j.get("developer")),
                             s(j.get("publisher")), s(j.get("genre")), s(j.get("esrb")),
                             s(r[0]), s(r[1]))
    tete = bytearray(struct.pack("<I", len(chaines)))
    for c in chaines:
        b = c.encode("utf-8")[:65535].decode("utf-8", "ignore").encode("utf-8")
        tete += struct.pack("<H", len(b)) + b
    flux = bytes(tete) + struct.pack("<I", len(jeux)) + bytes(corps)
    os.makedirs(os.path.dirname(SORTIE), exist_ok=True)
    with open(SORTIE + ".tmp", "wb") as f:
        f.write(b"GBJX" + bytes([1]) + zlib.compress(flux, 9))
    os.replace(SORTIE + ".tmp", SORTIE)
    return len(flux)


def main():
    print("libretro-database…")
    jeux = libretro()
    print(f"  {len(jeux)} jeux par CRC")
    print("Wikidata…")
    elements = wikidata()
    print(f"  {len(elements)} éléments, {sum(1 for e in elements.values() if e['fr'])} avec frwiki")
    paires, doutes = apparier(jeux, elements)
    commerciaux = [c for c, j in jeux.items() if not NON_COMMERCIAL.search(j["nom"])]
    apparies_c = sum(1 for c in commerciaux if c in paires)
    qids = set(paires.values())
    print(f"  appariés : {len(paires)}/{len(jeux)} CRC ({100 * len(paires) / len(jeux):.1f} %), "
          f"commerciaux {apparies_c}/{len(commerciaux)} "
          f"({100 * apparies_c / max(1, len(commerciaux)):.1f} %), {len(qids)} éléments")
    print("Wikipédia…")
    par_qid = {}
    for n, q in enumerate(sorted(qids)):
        e = elements[q]
        for langue in ("fr", "en"):
            if e[langue]:
                r = resume(langue, e[langue])
                if r:
                    par_qid[q] = (r[0], f"{langue}:{r[1]}")
                    break
        if n % 100 == 0:
            print(f"  {n}/{len(qids)}", flush=True)
    resumes = {c: par_qid[q] for c, q in paires.items() if q in par_qid}
    fr = sum(1 for v in resumes.values() if v[1].startswith("fr:"))
    brut = ecrire(jeux, resumes)
    taille = os.path.getsize(SORTIE)
    lignes = [
        f"jeux (CRC) : {len(jeux)}",
        f"avec éditeur ou développeur : "
        f"{sum(1 for j in jeux.values() if j.get('publisher') or j.get('developer'))}",
        f"appariés Wikidata : {len(paires)} ({100 * len(paires) / len(jeux):.1f} %), "
        f"commerciaux {apparies_c}/{len(commerciaux)} ({100 * apparies_c / max(1, len(commerciaux)):.1f} %)",
        f"résumés : {len(resumes)} CRC (fr {fr}, en {len(resumes) - fr}), {len(par_qid)} articles",
        f"fichier : {taille} octets (non compressé {brut})",
        "", "Doutes :", *sorted(doutes),
        "", "Commerciaux non appariés :",
        *sorted(jeux[c]["nom"] for c in commerciaux if c not in paires),
    ]
    with open(os.path.join(CACHE, "rapport.txt"), "w") as f:
        f.write("\n".join(lignes) + "\n")
    print("\n".join(lignes[:5]))


def tests():
    """`python3 -I scripts/donnees-jeux.py --tests` : normalisation, appariement, nettoyage."""
    assert cles(titre_nointro("Legend of Zelda, The - Link's Awakening (USA, Europe)")) == \
        cles("The Legend of Zelda: Link's Awakening")
    assert cles("Final Fantasy Legend II")[0] == cles("Final Fantasy Legend 2")[0]
    assert cles("Pokémon Édition Jaune")[0] == "pokemon edition jaune"
    assert "pokemon jaune" in cles("Pokémon Édition Jaune")
    assert variantes("Pokémon Rouge et Bleu") == ["pokemon rouge", "pokemon bleu"]
    assert cles("Mario & Luigi: Superstar Saga")[0] == "mario and luigi superstar saga"
    el = {
        "Q1": {"pf": {0}, "noms": ["Pokémon Red and Blue"], "alias": [], "fr": "P", "en": None},
        "Q2": {"pf": {2}, "noms": ["Golden Sun"], "alias": [], "fr": "G", "en": None},
        "Q3": {"pf": {2}, "noms": ["Golden Sun: The Lost Age"], "alias": ["Golden Sun"],
               "fr": "GS2", "en": None},
        "Q4": {"pf": {0}, "noms": ["Casper"], "alias": [], "fr": None, "en": "C"},
        "Q5": {"pf": {0}, "noms": ["Casper"], "alias": [], "fr": None, "en": "C2"},
        "Q6": {"pf": {0}, "noms": ["Tetris"], "alias": [], "fr": "T", "en": None},
    }
    jeux = {
        1: {"pf": 0, "nom": "Pokemon - Blue Version (USA, Europe) (SGB Enhanced)"},
        2: {"pf": 2, "nom": "Golden Sun (France)"},
        3: {"pf": 0, "nom": "Casper (Europe)"},
        4: {"pf": 1, "nom": "Tetris (World) (Rev 1)"},  # GBC : élément Game Boy accepté
        5: {"pf": 2, "nom": "Tetris (World)"},  # GBA : jamais apparié à un jeu Game Boy
    }
    paires, doutes = apparier(jeux, el)
    assert paires == {1: "Q1", 2: "Q2", 4: "Q6"}, paires
    assert any(d.startswith("ambigu   Casper") for d in doutes)
    assert nettoyer("Tetris (テトリス, Tetorisu?) est un jeu. Il sort en 1989. Puis M. X le "
                    "porte. Quatrième phrase.") == \
        "Tetris est un jeu. Il sort en 1989. Puis M. X le porte."
    assert len(nettoyer("mot " * 400)) <= 600
    print("tests : OK")


if __name__ == "__main__":
    tests() if sys.argv[1:] == ["--tests"] else main()
