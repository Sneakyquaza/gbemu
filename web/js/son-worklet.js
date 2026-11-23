// Lecteur du son de la console (AudioWorklet) : file de blocs stéréo entrelacés reçus de la page.
// Manque de données : le dernier échantillon s'éteint en douceur (pas de craquement).
// Trop de retard (> 0,25 s) : les plus vieux blocs sont jetés.

class LecteurGb extends AudioWorkletProcessor {
  constructor() {
    super();
    this.blocs = [];
    this.pos = 0;
    this.attente = 0; // échantillons stéréo en file
    this.g = 0;
    this.d = 0;
    this.compte = 0;
    this.port.onmessage = (e) => {
      if (e.data === "vider") {
        this.blocs = [];
        this.pos = 0;
        this.attente = 0;
        return;
      }
      this.blocs.push(e.data);
      this.attente += e.data.length / 2;
      const max = sampleRate / 4;
      while (this.attente > max && this.blocs.length > 1) {
        const b = this.blocs.shift();
        this.attente -= (b.length - this.pos) / 2;
        this.pos = 0;
      }
    };
  }

  process(_entrees, sorties) {
    const sortie = sorties[0];
    const gauche = sortie[0];
    const droite = sortie[1] || sortie[0];
    for (let i = 0; i < gauche.length; i++) {
      const b = this.blocs[0];
      if (b) {
        this.g = b[this.pos];
        this.d = b[this.pos + 1];
        this.pos += 2;
        this.attente--;
        if (this.pos >= b.length) {
          this.blocs.shift();
          this.pos = 0;
        }
      } else {
        this.g *= 0.995;
        this.d *= 0.995;
      }
      gauche[i] = this.g;
      droite[i] = this.d;
    }
    // Niveau de la file, pour l'asservissement de la cadence (environ 10 fois par seconde).
    if (++this.compte % 37 === 0) this.port.postMessage(this.attente);
    return true;
  }
}

registerProcessor("lecteur-gb", LecteurGb);
