import { ArrowLeftRight, Bell, BookText, FolderGit2, Gauge, KeyRound, Palette, SquareTerminal } from "lucide-react";
import type { MessageKey } from "@jarvis/i18n";

/** What an account carries (M20, widened by M27), in the order somebody meets
 *  it. One list for the auth screen and Settings -> Account. */
export const CARRIES: { icon: typeof Palette; label: MessageKey }[] = [
  { icon: Palette, label: "identity.carries.appearance" },
  { icon: SquareTerminal, label: "identity.carries.terminal" },
  { icon: Gauge, label: "identity.carries.autonomy" },
  { icon: Bell, label: "identity.carries.notifications" },
  { icon: BookText, label: "identity.carries.notebook" },
  { icon: FolderGit2, label: "identity.carries.projects" },
  { icon: KeyRound, label: "identity.carries.accounts" },
  { icon: ArrowLeftRight, label: "identity.carries.rotation" },
];
