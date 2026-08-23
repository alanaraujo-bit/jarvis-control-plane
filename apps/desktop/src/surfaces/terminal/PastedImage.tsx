import { useEffect, useRef, useState } from "react";
import { ImageIcon, X } from "lucide-react";

import { useT } from "../../app/i18n";
import { readAttachment, type Attachment } from "../../app/sessions";
import "./PastedImage.css";

interface PastedImageProps {
  sessionId: string;
  attachment: Attachment;
  onRemove: () => void;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * A pasted image, as a chip over the terminal with the picture on hover (§22).
 *
 * A bare filename is what this exists to avoid — Alan's own requirement. The
 * chip says an image is attached; hovering shows *which* image, which is the
 * only question a person actually has when they have pasted two screenshots
 * and cannot remember the order.
 *
 * The preview is fetched **once, on first hover**, not when the chip appears.
 * A 10 MB screenshot turned into a base64 `data:` URL is ~13 MB of string, and
 * paying that for every paste — including the ones nobody looks at — would be
 * a real cost for a picture the user just chose and already remembers.
 */
export function PastedImage({ sessionId, attachment, onRemove }: PastedImageProps) {
  const t = useT();
  const [preview, setPreview] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const requested = useRef(false);

  // A blob: URL rather than data: — no base64 inflation, and the CSP already
  // allows blob: for images. It has to be revoked or the bytes stay held for
  // the lifetime of the document.
  useEffect(() => {
    return () => {
      if (preview) URL.revokeObjectURL(preview);
    };
  }, [preview]);

  const load = () => {
    if (requested.current) return;
    requested.current = true;
    void readAttachment(sessionId, attachment.path)
      .then((bytes) => {
        const blob = new Blob([bytes as BlobPart], { type: attachment.mime });
        setPreview(URL.createObjectURL(blob));
      })
      .catch(() => setFailed(true));
  };

  return (
    <div className="pasted" onMouseEnter={load} onFocus={load}>
      <span className="pasted__chip" tabIndex={0}>
        <ImageIcon size={12} strokeWidth={1.9} aria-hidden="true" />
        <span className="pasted__name">{t("terminal.paste.attached")}</span>
        <span className="pasted__size">{formatSize(attachment.bytes)}</span>
      </span>

      <button
        type="button"
        className="pasted__remove"
        onClick={onRemove}
        aria-label={t("terminal.paste.remove")}
        title={t("terminal.paste.remove")}
      >
        <X size={11} strokeWidth={2.2} aria-hidden="true" />
      </button>

      {/* Rendered only once there is something to show. An empty frame that
          fills in a moment later is a flash of nothing, which reads as a bug
          rather than as loading. */}
      {preview && !failed && (
        <div className="pasted__preview" role="presentation">
          <img src={preview} alt={t("terminal.paste.attached")} />
        </div>
      )}
    </div>
  );
}
