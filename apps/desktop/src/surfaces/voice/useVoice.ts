import { Channel, invoke as tauriInvoke } from "@tauri-apps/api/core";
import { create } from "zustand";
import { invoke, isTauri } from "../../app/platform";
import { CHIME_DURATION_MS, playFinishChime, playStartChime } from "./sound";

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** Mirrors `voice::model::DownloadProgress` on the Rust side. */
type DownloadEvent =
  | { state: "downloading"; received: number; total: number }
  | { state: "verifying" }
  | { state: "done" }
  | { state: "error"; message: string };

/** Mirrors `voice::stream::StreamEvent` on the Rust side. */
type StreamEvent = { state: "partial"; committed: string; tail: string };

export type MicState = "idle" | "recording" | "transcribing" | "error";
export type DownloadState = "idle" | "downloading" | "verifying" | "error";

interface VoiceState {
  /** `null` means not yet checked. */
  modelPresent: boolean | null;
  downloadState: DownloadState;
  downloadPercent: number;
  downloadError: string | null;

  micState: MicState;
  micError: string | null;
  /** The most recently typed transcript, shown briefly then cleared. */
  lastTranscript: string | null;

  /**
   * Live captions while recording (§54 streaming, D30) — settled text that
   * will not change, and a volatile tail that may still be rewritten by the
   * next poll. Both reset to empty whenever a recording starts, stops, or is
   * cancelled; neither is ever what gets typed into the terminal — that
   * still comes from `stopRecording`'s own, separate, complete pass.
   */
  captionCommitted: string;
  captionTail: string;

  checkModel: () => Promise<void>;
  downloadModel: () => Promise<void>;
  startRecording: (projectId: string, locale: string) => Promise<void>;
  stopRecording: (projectId: string, sessionId: string, locale: string) => Promise<void>;
  cancelRecording: () => Promise<void>;
  clearTranscript: () => void;
}

export const useVoice = create<VoiceState>((set, get) => ({
  modelPresent: null,
  downloadState: "idle",
  downloadPercent: 0,
  downloadError: null,

  micState: "idle",
  micError: null,
  lastTranscript: null,

  captionCommitted: "",
  captionTail: "",

  checkModel: async () => {
    if (!isTauri()) {
      set({ modelPresent: true });
      return;
    }
    try {
      const status = await invoke<{ present: boolean }>("voice_model_status");
      set({ modelPresent: status.present });
    } catch {
      set({ modelPresent: false });
    }
  },

  downloadModel: async () => {
    if (!isTauri()) return;
    set({ downloadState: "downloading", downloadPercent: 0, downloadError: null });

    const channel = new Channel<DownloadEvent>();
    channel.onmessage = (event) => {
      if (event.state === "downloading") {
        const percent = event.total > 0 ? Math.min(100, Math.round((event.received / event.total) * 100)) : 0;
        set({ downloadState: "downloading", downloadPercent: percent });
      } else if (event.state === "verifying") {
        set({ downloadState: "verifying" });
      } else if (event.state === "done") {
        set({ downloadState: "idle", downloadPercent: 100, modelPresent: true });
      } else if (event.state === "error") {
        set({ downloadState: "error", downloadError: event.message });
      }
    };

    await tauriInvoke("voice_download_model", { channel });
  },

  startRecording: async (projectId, locale) => {
    if (!isTauri()) return;
    set({ micState: "recording", micError: null, captionCommitted: "", captionTail: "" });
    try {
      // The chime has to finish — and the mic has to still be closed while
      // it plays — before the stream opens, or the recording captures its
      // own start-of-listening cue and whisper transcribes it back.
      await playStartChime();
      await wait(CHIME_DURATION_MS);

      const channel = new Channel<StreamEvent>();
      channel.onmessage = (event) => {
        if (event.state === "partial") {
          set({ captionCommitted: event.committed, captionTail: event.tail });
        }
      };
      await tauriInvoke("voice_start_recording", { projectId, locale, channel });
    } catch (err) {
      set({ micState: "error", micError: String(err) });
    }
  },

  stopRecording: async (projectId, sessionId, locale) => {
    if (!isTauri()) return;
    if (get().micState !== "recording") return;
    set({ micState: "transcribing", captionCommitted: "", captionTail: "" });
    try {
      const result = await tauriInvoke<{ text: string }>("voice_stop_recording", {
        projectId,
        sessionId,
        locale,
      });
      set({ micState: "idle", lastTranscript: result.text || null });
      // The mic is already closed by now, so unlike the start chime this one
      // has no self-recording risk to wait out.
      void playFinishChime();
    } catch (err) {
      set({ micState: "error", micError: String(err) });
    }
  },

  cancelRecording: async () => {
    if (!isTauri()) return;
    if (get().micState !== "recording") return;
    set({ micState: "idle", captionCommitted: "", captionTail: "" });
    try {
      await tauriInvoke("voice_cancel_recording");
    } catch {
      // Best-effort — the recording thread will still stop on its own.
    }
  },

  clearTranscript: () => set({ lastTranscript: null }),
}));
