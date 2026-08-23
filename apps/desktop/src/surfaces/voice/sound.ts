/**
 * Two soft chimes for voice dictation (§54): one for "listening starts", one
 * for "it's done". Synthesized with the Web Audio API rather than shipped as
 * audio files — this app's CSP has no `media-src`/`data:` allowance for
 * audio, and an oscillator needs neither.
 *
 * Both cues use pure sine tones (no harmonics — the softest waveform there
 * is) and a two-note pair that shares the same interval in each direction,
 * C5 → E5 to open, E5 → C5 to close, so the pair reads as one coherent idea
 * rather than two unrelated jingles. Every note ramps its gain up and back
 * down rather than starting or stopping cold — an oscillator switched on or
 * off at full volume produces an audible click from the waveform
 * discontinuity, which is its own small version of the harsh edge this was
 * written to get away from.
 *
 * `PEAK_GAIN` and the two note tables are the only things worth touching:
 * this was tuned by eye, not by ear — nothing here has been listened to.
 * Alan is the one confirming it actually sounds pleasant.
 */

const PEAK_GAIN = 0.06;
const NOTE_ATTACK = 0.015;
const NOTE_HOLD = 0.09;
const NOTE_RELEASE = 0.12;

const C5 = 523.25;
const E5 = 659.25;

let ctx: AudioContext | null = null;

/** One `AudioContext`, created on first use and kept for the app's lifetime.
 * A fresh context per chime leaks — browsers cap how many can exist, so
 * dictation would go silent after a handful of uses. */
function context(): AudioContext {
  if (!ctx) ctx = new AudioContext();
  return ctx;
}

function note(when: number, freq: number): void {
  const audio = context();
  const osc = audio.createOscillator();
  const gain = audio.createGain();
  osc.type = "sine";
  osc.frequency.value = freq;

  gain.gain.setValueAtTime(0, when);
  gain.gain.linearRampToValueAtTime(PEAK_GAIN, when + NOTE_ATTACK);
  gain.gain.setValueAtTime(PEAK_GAIN, when + NOTE_ATTACK + NOTE_HOLD);
  gain.gain.exponentialRampToValueAtTime(0.0001, when + NOTE_ATTACK + NOTE_HOLD + NOTE_RELEASE);

  osc.connect(gain).connect(audio.destination);
  osc.start(when);
  osc.stop(when + NOTE_ATTACK + NOTE_HOLD + NOTE_RELEASE + 0.02);
}

/** Total audible length of either chime, in milliseconds — callers that must
 * not overlap the chime with something else (the mic capturing itself, for
 * one) should wait this long first. */
export const CHIME_DURATION_MS = 260;

async function play(notes: [number, number][]): Promise<void> {
  const audio = context();
  if (audio.state === "suspended") await audio.resume();
  const base = audio.currentTime;
  for (const [freq, offset] of notes) note(base + offset, freq);
}

/** Plays before the microphone opens — see `CHIME_DURATION_MS` for why the
 * caller needs to wait rather than opening the stream immediately after. */
export function playStartChime(): Promise<void> {
  return play([
    [C5, 0],
    [E5, 0.07],
  ]);
}

/** Plays after transcription has finished and the mic is already closed, so
 * there is no self-recording risk here the way there is on start. */
export function playFinishChime(): Promise<void> {
  return play([
    [E5, 0],
    [C5, 0.07],
  ]);
}
