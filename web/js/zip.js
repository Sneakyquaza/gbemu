// Archive .zip sans compression (méthode « stored ») pour l'export des sauvegardes.

const TABLE = new Uint32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

export function crc32(o) {
  let c = 0xffffffff;
  for (let i = 0; i < o.length; i++) c = TABLE[(c ^ o[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

// fichiers : [{ nom, octets: Uint8Array }] → Blob
export function zipper(fichiers) {
  const enc = new TextEncoder();
  const parties = [];
  const central = [];
  let pos = 0;
  for (const f of fichiers) {
    const nom = enc.encode(f.nom);
    const crc = crc32(f.octets);
    const n = f.octets.length;
    const local = new DataView(new ArrayBuffer(30));
    local.setUint32(0, 0x04034b50, true);
    local.setUint16(4, 20, true);
    local.setUint16(6, 0x0800, true); // noms en UTF-8
    local.setUint32(14, crc, true);
    local.setUint32(18, n, true);
    local.setUint32(22, n, true);
    local.setUint16(26, nom.length, true);
    parties.push(local, nom, f.octets);
    const c = new DataView(new ArrayBuffer(46));
    c.setUint32(0, 0x02014b50, true);
    c.setUint16(4, 20, true);
    c.setUint16(6, 20, true);
    c.setUint16(8, 0x0800, true);
    c.setUint32(16, crc, true);
    c.setUint32(20, n, true);
    c.setUint32(24, n, true);
    c.setUint16(28, nom.length, true);
    c.setUint32(42, pos, true);
    central.push(c, nom);
    pos += 30 + nom.length + n;
  }
  const taille = central.reduce((s, p) => s + p.byteLength, 0);
  const fin = new DataView(new ArrayBuffer(22));
  fin.setUint32(0, 0x06054b50, true);
  fin.setUint16(8, fichiers.length, true);
  fin.setUint16(10, fichiers.length, true);
  fin.setUint32(12, taille, true);
  fin.setUint32(16, pos, true);
  return new Blob([...parties, ...central, fin], { type: "application/zip" });
}
