import { Glyph, TONE_TINT, type GlyphName } from "./ui";

/**
 * The three ways linked machines share files (decisions #70, #73, #94), with
 * what each is good at and what it costs, in one place so the first-run
 * questions and any explainer elsewhere say the same thing (decision #121).
 */
export type SyncMethod = "lan" | "cloud" | "github";

export interface MethodInfo {
  id: SyncMethod;
  name: string;
  icon: GlyphName;
  good: string;
  limit: string;
}

export const SYNC_METHODS: MethodInfo[] = [
  {
    id: "cloud",
    name: "A cloud folder (Dropbox, OneDrive, iCloud…)",
    icon: "cloud",
    good: "Works anywhere, and machines don't need to be on at the same time. The easiest if you already use one.",
    limit: "Uses that service's space, and the same service has to be on every machine.",
  },
  {
    id: "lan",
    name: "Your home network, no cloud",
    icon: "link",
    good: "Machines copy straight from each other, encrypted. No account, nothing stored online.",
    limit: "Both machines must be on and on the same network at once. Big files still need a cloud folder.",
  },
  {
    id: "github",
    name: "A private GitHub repo",
    icon: "code",
    good: "Keeps every version of every file, and works anywhere.",
    limit: "Needs a GitHub account and a token. Files over 95 MB are left out.",
  },
];

/** A row of choices, one picked: an icon tile, a name, and two dim lines. */
export function ChoiceRow({
  icon,
  name,
  lines,
  checked,
  onPick,
  type = "radio",
  group,
}: {
  icon: GlyphName;
  name: string;
  lines: string[];
  checked: boolean;
  onPick: (on: boolean) => void;
  type?: "radio" | "checkbox";
  group?: string;
}) {
  return (
    <label
      className={`flex cursor-pointer items-start gap-2.5 rounded-md px-2 py-1.5 transition-colors has-[:focus-visible]:ring-1 has-[:focus-visible]:ring-accent ${
        checked ? "bg-sel" : "hover:bg-sel/50"
      }`}
    >
      <input
        type={type}
        name={group}
        className="sr-only"
        checked={checked}
        onChange={(e) => onPick(type === "radio" ? true : e.target.checked)}
      />
      <span
        className={`mt-0.5 flex size-[22px] shrink-0 items-center justify-center rounded-md ${
          checked ? TONE_TINT.accent : TONE_TINT.dim
        }`}
      >
        <Glyph name={checked ? "check" : icon} />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block text-xs font-medium">{name}</span>
        {lines.map((l) => (
          <span key={l} className="block text-[11px] leading-snug text-dim">
            {l}
          </span>
        ))}
      </span>
    </label>
  );
}
