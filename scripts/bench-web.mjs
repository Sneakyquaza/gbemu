// Vitesse du module wasm sans écran (Node) : ms par image, écran RGBA et son compris.
// Usage : node scripts/bench-web.mjs ROM [IMAGES]   (après scripts/web.sh)
import { readFileSync } from "node:fs";
import { initSync, Emu, analyser_rom } from "../out/web/pkg/gb_web.js";

const [rom, n = "3000"] = process.argv.slice(2);
initSync({ module: readFileSync(new URL("../out/web/pkg/gb_web_bg.wasm", import.meta.url)) });
const octets = new Uint8Array(readFileSync(rom));
const info = analyser_rom(rom, octets);
const emu = new Emu(info.console, octets);
emu.set_frequence(48000);
const images = +n;
for (let i = 0; i < 120; i++) emu.run_frame(); // échauffement (JIT)
const t0 = performance.now();
for (let i = 0; i < images; i++) {
  emu.run_frame();
  emu.ecran();
  emu.echantillons();
}
const ms = (performance.now() - t0) / images;
console.log(`${rom} [${info.console}] : ${ms.toFixed(3)} ms/image (${(1000 / ms / emu.hz).toFixed(1)}× le temps réel)`);
