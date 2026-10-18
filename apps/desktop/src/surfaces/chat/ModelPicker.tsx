import { useState } from "react";
import { Check, ChevronUp } from "lucide-react";
import type { ChatModel } from "../../app/chat";
import { useT, type Translate } from "../../app/i18n";
import { Popover } from "../../design/Popover";

const KNOWN_EFFORTS = new Set(["low", "medium", "high", "xhigh", "max", "ultra"]);

/** An effort in the person's words; one the CLI invents later shows as sent. */
export function effortLabel(t: Translate, effort: string): string {
  return KNOWN_EFFORTS.has(effort) ? t(`chat.effort.${effort}` as "chat.effort.low") : effort;
}

/**
 * Model and effort, inside the text box (M29). The list and each model's
 * efforts come from the CLI (§26); what the button says is what the CLI
 * confirms is applied (§28), so a change shows once it has taken.
 */
export function ModelPicker({
  models,
  model,
  effort,
  disabled,
  onModel,
  onEffort,
}: {
  models: ChatModel[];
  model: string | null;
  effort: string | null;
  disabled: boolean;
  onModel: (id: string) => void;
  onEffort: (effort: string) => void;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [anchor, setAnchor] = useState<HTMLButtonElement | null>(null);

  const current = models.find((m) => m.id === model);
  const name = current?.name ?? model ?? t("chat.model.default");
  const efforts = current?.efforts ?? [];

  return (
    <>
      <button
        ref={setAnchor}
        type="button"
        className="chat-picker"
        onClick={() => setOpen((value) => !value)}
        disabled={disabled}
        aria-expanded={open}
        aria-label={t("chat.model.label")}
        title={t("chat.model.label")}
      >
        <span className="chat-picker__name">{name}</span>
        {effort && <span className="chat-picker__effort">· {effortLabel(t, effort)}</span>}
        <ChevronUp size={12} strokeWidth={2} aria-hidden="true" />
      </button>

      <Popover anchor={anchor} open={open} onClose={() => setOpen(false)} placement="above">
        <div className="chat-models">
          <p className="popover__heading">{t("chat.model.models")}</p>
          {models.map((option) => (
            <button
              key={option.id}
              type="button"
              role="menuitemradio"
              aria-checked={option.id === model}
              className="chat-models__item"
              onClick={() => {
                setOpen(false);
                if (option.id !== model) onModel(option.id);
              }}
            >
              <span className="chat-models__text">
                <span className="chat-models__name">{option.name}</span>
                {option.description && <span className="chat-models__help">{option.description}</span>}
              </span>
              {option.id === model && <Check size={13} strokeWidth={2.2} aria-hidden="true" />}
            </button>
          ))}

          {efforts.length > 0 && (
            <>
              <p className="popover__heading">{t("chat.model.effort")}</p>
              <div className="chat-models__efforts" role="radiogroup" aria-label={t("chat.model.effort")}>
                {efforts.map((option) => (
                  <button
                    key={option}
                    type="button"
                    role="radio"
                    aria-checked={option === effort}
                    data-active={option === effort || undefined}
                    className="chat-models__effort"
                    onClick={() => {
                      setOpen(false);
                      if (option !== effort) onEffort(option);
                    }}
                  >
                    {effortLabel(t, option)}
                  </button>
                ))}
              </div>
            </>
          )}
        </div>
      </Popover>
    </>
  );
}
