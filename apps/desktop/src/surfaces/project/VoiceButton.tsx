import { useEffect, useState } from "react";
import { Mic, MicOff } from "lucide-react";
import { useT } from "../../app/i18n";
import { Popover } from "../../design/Popover";
import { LiveCaption } from "../voice/LiveCaption";
import { useVoice } from "../voice/useVoice";

interface VoiceButtonProps {
  projectId: string;
  sessionId: string;
  locale: string;
}

/**
 * Dictation into the live session (§54).
 *
 * One button, four states: idle, recording, transcribing, error — each its
 * own colour and motion rather than a spinner, the same way the rest of this
 * product prefers a state to a generic "loading" tell. Recording uses the
 * cool blue already reserved for "waiting" (§6): amber is agent work, and a
 * person dictating is the opposite of that.
 *
 * The transcript is typed into the prompt, never submitted — see
 * `voice_stop_recording` on the Rust side for why, and `session::typing` for
 * how it avoids the character-loss failure a raw paste would hit.
 */
export function VoiceButton({ projectId, sessionId, locale }: VoiceButtonProps) {
  const t = useT();
  const modelPresent = useVoice((s) => s.modelPresent);
  const checkModel = useVoice((s) => s.checkModel);
  const downloadState = useVoice((s) => s.downloadState);
  const downloadPercent = useVoice((s) => s.downloadPercent);
  const downloadError = useVoice((s) => s.downloadError);
  const downloadModel = useVoice((s) => s.downloadModel);
  const micState = useVoice((s) => s.micState);
  const micError = useVoice((s) => s.micError);
  const startRecording = useVoice((s) => s.startRecording);
  const stopRecording = useVoice((s) => s.stopRecording);
  const captionCommitted = useVoice((s) => s.captionCommitted);
  const captionTail = useVoice((s) => s.captionTail);

  const [panelOpen, setPanelOpen] = useState(false);
  const [anchor, setAnchor] = useState<HTMLButtonElement | null>(null);

  useEffect(() => {
    if (modelPresent === null) void checkModel();
  }, [modelPresent, checkModel]);

  // The download finished while the panel was open — there is nothing left
  // for it to show, so it closes itself rather than reverting to "Baixar"
  // as if nothing had happened.
  useEffect(() => {
    if (modelPresent) setPanelOpen(false);
  }, [modelPresent]);

  // An error is shown briefly, then the button returns to normal — nothing
  // here needs a person to dismiss it, and nothing should sit red forever.
  useEffect(() => {
    if (micState !== "error") return;
    const id = window.setTimeout(() => useVoice.setState({ micState: "idle", micError: null }), 3500);
    return () => window.clearTimeout(id);
  }, [micState]);

  const handleClick = () => {
    if (modelPresent === false) {
      setPanelOpen((open) => !open);
      return;
    }
    if (micState === "idle") {
      void startRecording(projectId, locale);
    } else if (micState === "recording") {
      void stopRecording(projectId, sessionId, locale);
    }
  };

  const label =
    micState === "recording"
      ? t("voice.recording")
      : micState === "transcribing"
        ? t("voice.transcribing")
        : micState === "error"
          ? micError ?? t("voice.error")
          : modelPresent === false
            ? t("voice.downloadNeeded")
            : t("voice.start");

  return (
    <div className="workspace__mic">
      <button
        ref={setAnchor}
        type="button"
        className="workspace__mic-button"
        data-state={micState}
        data-model-missing={modelPresent === false || undefined}
        onClick={handleClick}
        disabled={micState === "transcribing"}
        aria-label={label}
        title={label}
      >
        {micState === "error" ? (
          <MicOff size={13} strokeWidth={2} aria-hidden="true" />
        ) : (
          <Mic size={13} strokeWidth={2} aria-hidden="true" />
        )}
      </button>

      <Popover anchor={anchor} open={panelOpen} onClose={() => setPanelOpen(false)}>
        <ModelDownloadPanel
          state={downloadState}
          percent={downloadPercent}
          error={downloadError}
          onDownload={() => void downloadModel()}
        />
      </Popover>

      <LiveCaption
        anchor={anchor}
        open={micState === "recording"}
        committed={captionCommitted}
        tail={captionTail}
      />
    </div>
  );
}

function ModelDownloadPanel({
  state,
  percent,
  error,
  onDownload,
}: {
  state: "idle" | "downloading" | "verifying" | "error";
  percent: number;
  error: string | null;
  onDownload: () => void;
}) {
  const t = useT();
  return (
    <div className="voice-download">
      <p className="voice-download__title">{t("voice.download.title")}</p>
      <p className="voice-download__body">{t("voice.download.body")}</p>

      {state === "idle" && (
        <button type="button" className="voice-download__action" onClick={onDownload}>
          {t("voice.download.action")}
        </button>
      )}

      {(state === "downloading" || state === "verifying") && (
        <div className="voice-download__progress">
          <div className="voice-download__track">
            <div
              className="voice-download__fill"
              style={{ width: `${state === "verifying" ? 100 : percent}%` }}
            />
          </div>
          <span className="voice-download__percent">
            {state === "verifying" ? t("voice.download.verifying") : `${percent}%`}
          </span>
        </div>
      )}

      {state === "error" && (
        <div className="voice-download__error">
          <p>{error}</p>
          <button type="button" className="voice-download__action" onClick={onDownload}>
            {t("voice.download.retry")}
          </button>
        </div>
      )}
    </div>
  );
}
