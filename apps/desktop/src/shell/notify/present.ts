import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";
import { isTauri } from "../../app/platform";

/**
 * Getting a notification off the screen it was born on (§49).
 *
 * Three channels, and they are not alternatives — they answer three different
 * situations, and which one reaches the person is not something this code gets
 * to know:
 *
 * | Channel | For |
 * |---|---|
 * | The in-app toast | The window is in front, on another surface |
 * | The Windows toast | The window is behind something, or minimised |
 * | The taskbar flash | The Windows toast was missed, or is switched off |
 *
 * ## What a Windows toast can and cannot do here
 *
 * Verified against a real installed build before any of this was written, and
 * two of the three answers were not what a reasonable person would assume:
 *
 * * **It appears, attributed correctly.** The AppUserModelID comes from the
 *   bundle identifier, and the NSIS installer writes it onto the Start Menu
 *   shortcut — checked on this machine, which also has an unrelated
 *   `jarvis.exe` with a shortcut of its own. Ours shows our own mark and name.
 * * **It does not appear from `pnpm dev`.** The plugin only sets the AUMID when
 *   the executable is *not* under `target/debug` or `target/release`, and an
 *   unpackaged binary has no shortcut to be identified by. Not a bug to chase.
 * * **Clicking it does nothing.** `tauri-plugin-notification` 2.3.3 exposes
 *   activation callbacks on mobile only; there is no desktop equivalent. So the
 *   Windows toast is an **alert**, and the notification centre is the thing you
 *   click. Building the feature the other way round would have produced a toast
 *   that looks interactive and is not, which is worse than one that is plainly
 *   just an alert.
 */

export interface SystemToast {
  title: string;
  body: string;
}

/**
 * Whether the desktop has granted permission to post notifications.
 *
 * Asked once and cached. On Windows the answer is effectively always yes, but
 * asking is what makes this work unchanged if the product ever runs anywhere
 * that says no.
 */
let granted: boolean | null = null;

async function ensurePermission(): Promise<boolean> {
  if (granted !== null) return granted;
  try {
    const { isPermissionGranted, requestPermission } = await import(
      "@tauri-apps/plugin-notification"
    );
    granted = (await isPermissionGranted()) || (await requestPermission()) === "granted";
  } catch {
    granted = false;
  }
  return granted;
}

/** Post a desktop notification. Silent about its own failures, deliberately. */
export async function systemToast(toast: SystemToast): Promise<void> {
  if (!isTauri()) return;
  try {
    if (!(await ensurePermission())) return;
    const { sendNotification } = await import("@tauri-apps/plugin-notification");
    sendNotification({ title: toast.title, body: toast.body });
  } catch {
    // A desktop that will not show a toast is a fact about the machine, not an
    // error in the work. The in-app centre still has it.
  }
}

/**
 * Make the taskbar button ask for attention.
 *
 * The quiet fallback for a Windows toast that was missed, dismissed, or
 * switched off in the OS. `Informational` rather than `Critical`: critical
 * flashes until the window is focused, which is the behaviour of an
 * application that thinks it is more important than whatever you are doing.
 */
export async function flashTaskbar(): Promise<void> {
  if (!isTauri()) return;
  try {
    const win = getCurrentWindow();
    if (await win.isFocused()) return;
    await win.requestUserAttention(UserAttentionType.Informational);
  } catch {
    // Not every platform has a taskbar.
  }
}

/**
 * A short, quiet sound.
 *
 * Synthesised rather than shipped as a file: two sine tones through
 * `AudioContext` weigh nothing, need no asset pipeline, and cannot be the
 * reason a build is 400KB heavier. Two notes falling for something finished
 * and rising for something waiting, so the two are distinguishable from
 * another room — which is the only situation a sound is for.
 *
 * Deliberately very quiet. This plays while somebody is working.
 */
let audio: AudioContext | null = null;

export function chime(kind: "waiting" | "done"): void {
  try {
    audio ??= new AudioContext();
    if (audio.state === "suspended") void audio.resume();

    const now = audio.currentTime;
    const notes = kind === "waiting" ? [587.33, 880.0] : [880.0, 587.33];

    notes.forEach((frequency, index) => {
      const at = now + index * 0.11;
      const osc = audio!.createOscillator();
      const gain = audio!.createGain();
      osc.type = "sine";
      osc.frequency.value = frequency;
      // An envelope, not a switch: a square-edged gate on a sine wave clicks,
      // and a click is exactly the cheap sound this is trying not to make.
      gain.gain.setValueAtTime(0.0001, at);
      gain.gain.exponentialRampToValueAtTime(0.05, at + 0.012);
      gain.gain.exponentialRampToValueAtTime(0.0001, at + 0.22);
      osc.connect(gain).connect(audio!.destination);
      osc.start(at);
      osc.stop(at + 0.24);
    });
  } catch {
    // No audio device, or a browser that will not start a context without a
    // gesture. Silence is an acceptable outcome for a sound.
  }
}
