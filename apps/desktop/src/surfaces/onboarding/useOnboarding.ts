import { create } from "zustand";
import { invoke, isTauri } from "../../app/platform";

interface OnboardingState {
  /** `null` means not yet known — nothing renders off of it until it is. */
  seen: boolean | null;
  load: () => Promise<void>;
  /** Mark the welcome screen seen, locally and on disk. */
  complete: () => Promise<void>;
}

export const useOnboarding = create<OnboardingState>((set) => ({
  seen: null,

  load: async () => {
    if (!isTauri()) {
      set({ seen: true });
      return;
    }
    try {
      set({ seen: await invoke<boolean>("onboarding_status") });
    } catch {
      // A first run must never be the reason the window stays hidden — see
      // `onboarding::onboarding_status`'s own doc comment. Whatever failed
      // here, the rest of the app must still open.
      set({ seen: true });
    }
  },

  complete: async () => {
    set({ seen: true });
    if (!isTauri()) return;
    try {
      await invoke("onboarding_mark_seen");
    } catch {
      // Best-effort: worst case this machine sees the screen again next
      // launch, which is a much smaller cost than blocking on it now.
    }
  },
}));
