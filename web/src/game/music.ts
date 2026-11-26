// Background music for the host screen. The tracks are short original loops
// played by a small Web Audio synthesiser, so there are no audio files to
// download and no third-party licences to track.

type Chord = number[]; // MIDI notes

export type Track = {
  id: string;
  bpm: number;
  /** Swing for the off-beat 16ths, 0 to 0.33. */
  swing: number;
  chords: Chord[];
  /** 16 steps per bar: an index into the chord (+7 is an octave up), or null. */
  lead: (number | null)[][];
  leadWave: OscillatorType;
  leadOctave: number;
  /** Bass steps per bar (true plays the chord's root). */
  bass: boolean[];
  hats: boolean[];
  kick: boolean[];
  pad: boolean;
};

const x = true;
const _ = false;
const n = null;

// Chord helpers, by root MIDI note.
const maj = (r: number) => [r, r + 4, r + 7];
const min = (r: number) => [r, r + 3, r + 7];
const maj7 = (r: number) => [r, r + 4, r + 7, r + 11];
const min7 = (r: number) => [r, r + 3, r + 7, r + 10];

export const TRACKS: Track[] = [
  {
    id: "beach",
    bpm: 116,
    swing: 0,
    chords: [maj(60), min(57), maj(53), maj(55)],
    lead: [
      [0, n, 2, n, 4, n, 2, n, 5, n, 4, n, 2, n, n, n],
      [0, n, 2, n, 4, n, 5, 4, 2, n, 0, n, 2, n, n, n],
      [4, n, 5, n, 4, n, 2, n, 0, n, 2, n, 4, n, n, n],
      [2, n, 4, n, 5, n, 4, n, 2, n, 1, n, 0, n, n, n],
    ],
    leadWave: "triangle",
    leadOctave: 12,
    bass: [x, _, _, _, x, _, _, x, _, _, x, _, x, _, _, _],
    hats: [_, _, x, _, _, _, x, _, _, _, x, _, _, _, x, x],
    kick: [x, _, _, _, x, _, _, _, x, _, _, _, x, _, _, _],
    pad: false,
  },
  {
    id: "garden",
    bpm: 88,
    swing: 0.22,
    chords: [maj7(53), min7(57), min7(50), maj7(55)],
    lead: [
      [3, n, n, 2, n, n, 1, n, 2, n, n, n, n, n, n, n],
      [n, n, 1, n, 2, n, 3, n, n, n, 2, n, 1, n, n, n],
      [0, n, n, 1, n, n, 2, n, 3, n, 2, n, n, n, n, n],
      [n, n, 3, n, 2, n, 1, n, 0, n, n, n, n, n, n, n],
    ],
    leadWave: "sine",
    leadOctave: 12,
    bass: [x, _, _, _, _, _, _, _, x, _, _, x, _, _, _, _],
    hats: [x, _, x, _, x, _, x, _, x, _, x, _, x, _, x, _],
    kick: [x, _, _, _, _, _, _, _, x, _, x, _, _, _, _, _],
    pad: true,
  },
  {
    id: "thinking",
    bpm: 124,
    swing: 0,
    chords: [min(57), maj(53), maj(48), maj(55)],
    lead: [
      [0, 1, 2, 1, 0, 1, 2, 1, 0, 1, 2, 1, 0, 1, 2, 1],
      [0, 1, 2, 1, 0, 1, 2, 1, 0, 1, 2, 1, 0, 1, 2, 1],
      [0, 1, 2, 1, 0, 1, 2, 1, 0, 1, 2, 1, 0, 1, 2, 1],
      [0, 1, 2, 1, 0, 1, 2, 1, 2, 1, 0, 1, 2, 3, 4, 5],
    ],
    leadWave: "square",
    leadOctave: 12,
    bass: [x, _, x, _, x, _, x, _, x, _, x, _, x, _, x, _],
    hats: [_, _, x, _, _, _, x, _, _, _, x, _, _, _, x, _],
    kick: [x, _, _, _, x, _, _, _, x, _, _, _, x, _, _, _],
    pad: false,
  },
  {
    id: "river",
    bpm: 72,
    swing: 0,
    chords: [maj7(48), maj7(53), min7(57), maj(55)],
    lead: [
      [4, n, n, n, n, n, 2, n, n, n, n, n, 3, n, n, n],
      [n, n, n, n, 2, n, n, n, 1, n, n, n, n, n, n, n],
      [3, n, n, n, n, n, 2, n, n, n, 4, n, n, n, n, n],
      [n, n, n, n, 2, n, n, n, n, n, n, n, 0, n, n, n],
    ],
    leadWave: "sine",
    leadOctave: 24,
    bass: [x, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _],
    hats: [_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _],
    kick: [_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _],
    pad: true,
  },
];

const hz = (midi: number) => 440 * Math.pow(2, (midi - 69) / 12);

function chordNote(chord: Chord, i: number): number {
  const octave = Math.floor(i / chord.length);
  return chord[i % chord.length] + octave * 12;
}

/** Plays one track at a time, looping, with a master volume. */
export class MusicPlayer {
  private ctx: AudioContext | null = null;
  private master: GainNode | null = null;
  private noise: AudioBuffer | null = null;
  private timer: number | null = null;
  private track: Track | null = null;
  private step = 0;
  private nextAt = 0;
  private volume = 0.6;
  private muted = false;

  static supported(): boolean {
    return typeof window !== "undefined" && typeof window.AudioContext === "function";
  }

  private context(): AudioContext | null {
    if (!MusicPlayer.supported()) return null;
    if (!this.ctx) {
      this.ctx = new AudioContext();
      this.master = this.ctx.createGain();
      this.master.connect(this.ctx.destination);
      this.applyVolume();
      const len = this.ctx.sampleRate * 0.2;
      this.noise = this.ctx.createBuffer(1, len, this.ctx.sampleRate);
      const data = this.noise.getChannelData(0);
      for (let i = 0; i < len; i++) data[i] = Math.random() * 2 - 1;
    }
    return this.ctx;
  }

  /** Browsers only start audio after a click; call this from one. */
  unlock() {
    const ctx = this.context();
    if (ctx && ctx.state === "suspended") void ctx.resume();
  }

  play(id: string | null) {
    const track = TRACKS.find((t) => t.id === id) ?? null;
    if (track?.id === this.track?.id && this.timer !== null) return;
    this.stop();
    this.track = track;
    const ctx = this.context();
    if (!track || !ctx) return;
    this.step = 0;
    this.nextAt = ctx.currentTime + 0.1;
    this.timer = window.setInterval(() => this.schedule(), 25);
  }

  stop() {
    if (this.timer !== null) window.clearInterval(this.timer);
    this.timer = null;
    this.track = null;
  }

  setVolume(v: number) {
    this.volume = Math.min(1, Math.max(0, v));
    this.applyVolume();
  }

  setMuted(m: boolean) {
    this.muted = m;
    this.applyVolume();
  }

  dispose() {
    this.stop();
    void this.ctx?.close();
    this.ctx = null;
  }

  private applyVolume() {
    if (!this.master || !this.ctx) return;
    const target = this.muted ? 0 : this.volume * this.volume * 0.5;
    this.master.gain.setTargetAtTime(target, this.ctx.currentTime, 0.05);
  }

  private schedule() {
    const ctx = this.ctx;
    const t = this.track;
    if (!ctx || !t) return;
    const sixteenth = 60 / t.bpm / 4;
    while (this.nextAt < ctx.currentTime + 0.15) {
      const bar = Math.floor(this.step / 16) % t.chords.length;
      const s = this.step % 16;
      const at = this.nextAt + (s % 2 === 1 ? t.swing * sixteenth : 0);
      const chord = t.chords[bar];
      if (s === 0 && t.pad) for (const note of chord) this.tone(hz(note), at, sixteenth * 16, "sine", 0.05, 0.4);
      if (s === 0 || s === 8) if (!t.pad) for (const note of chord) this.tone(hz(note), at, sixteenth * 3, "triangle", 0.035, 0.01);
      const lead = t.lead[bar][s];
      if (lead !== null) this.tone(hz(chordNote(chord, lead) + t.leadOctave), at, sixteenth * 1.8, t.leadWave, t.leadWave === "square" ? 0.035 : 0.09, 0.01);
      if (t.bass[s]) this.tone(hz(chord[0] - 24), at, sixteenth * 2, "triangle", 0.16, 0.01);
      if (t.kick[s]) this.kick(at);
      if (t.hats[s]) this.hat(at);
      this.nextAt += sixteenth;
      this.step = (this.step + 1) % (16 * t.chords.length);
    }
  }

  private tone(freq: number, at: number, dur: number, wave: OscillatorType, gain: number, attack: number) {
    const ctx = this.ctx!;
    const osc = ctx.createOscillator();
    const g = ctx.createGain();
    osc.type = wave;
    osc.frequency.value = freq;
    g.gain.setValueAtTime(0, at);
    g.gain.linearRampToValueAtTime(gain, at + attack);
    g.gain.exponentialRampToValueAtTime(0.0001, at + dur);
    osc.connect(g).connect(this.master!);
    osc.start(at);
    osc.stop(at + dur + 0.05);
  }

  private kick(at: number) {
    const ctx = this.ctx!;
    const osc = ctx.createOscillator();
    const g = ctx.createGain();
    osc.frequency.setValueAtTime(120, at);
    osc.frequency.exponentialRampToValueAtTime(45, at + 0.12);
    g.gain.setValueAtTime(0.22, at);
    g.gain.exponentialRampToValueAtTime(0.0001, at + 0.18);
    osc.connect(g).connect(this.master!);
    osc.start(at);
    osc.stop(at + 0.2);
  }

  private hat(at: number) {
    const ctx = this.ctx!;
    const src = ctx.createBufferSource();
    src.buffer = this.noise;
    const hp = ctx.createBiquadFilter();
    hp.type = "highpass";
    hp.frequency.value = 7000;
    const g = ctx.createGain();
    g.gain.setValueAtTime(0.05, at);
    g.gain.exponentialRampToValueAtTime(0.0001, at + 0.05);
    src.connect(hp).connect(g).connect(this.master!);
    src.start(at);
    src.stop(at + 0.06);
  }
}
