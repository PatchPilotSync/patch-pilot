import type { ButtonHTMLAttributes, ReactNode } from "react";

export function Button({
  variant = "primary",
  className = "",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: "primary" | "quiet" }) {
  const look =
    variant === "primary"
      ? "bg-accent text-on-accent font-semibold hover:brightness-110"
      : "text-dim hover:bg-sel hover:text-fg";
  return (
    <button
      className={`inline-flex h-7 items-center justify-center rounded-md px-3 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${look} ${className}`}
      {...props}
    />
  );
}

export function Badge({ children, tone = "neutral" }: { children: ReactNode; tone?: "neutral" | "amber" }) {
  const look = tone === "amber" ? "bg-warn/15 text-warn" : "bg-sel text-dim";
  return (
    <span className={`inline-flex shrink-0 items-center rounded px-1.5 py-px text-[11px] font-medium ${look}`}>
      {children}
    </span>
  );
}

/** The pixel-plane logo (docs/brand/mark-mono.svg), tinted by the text colour. Keep sizes whole pixels. */
export function Mark({ size = 16, className = "" }: { size?: number; className?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" shapeRendering="crispEdges" aria-hidden="true" className={className}>
      <path
        fill="currentColor"
        d="M7 2h2v1h-2zM7 3h2v1h-2zM7 5h2v1h-2zM6 6h4v1h-4zM4 7h8v1h-8zM1 8h14v1h-14zM0 9h1v1h-1zM1 9h6v1h-6zM7 9h2v1h-2zM9 9h6v1h-6zM15 9h1v1h-1zM7 10h2v1h-2zM7 11h2v1h-2zM7 12h2v1h-2zM5 13h6v1h-6zM4 14h1v1h-1zM5 14h2v1h-2zM7 14h2v1h-2zM9 14h2v1h-2zM11 14h1v1h-1z"
      />
    </svg>
  );
}

/** Text colour for a parity score. */
export function scoreColor(score: number | null): string {
  if (score === null) return "text-dim";
  if (score >= 95) return "text-ok";
  if (score >= 75) return "text-warn";
  return "text-bad";
}

/** Background colour matching scoreColor, for bars and dots. */
export function scoreFill(score: number | null): string {
  if (score === null) return "bg-dim";
  if (score >= 95) return "bg-ok";
  if (score >= 75) return "bg-warn";
  return "bg-bad";
}

/** A thin track filled to `score` percent. */
export function ScoreBar({ score, className = "" }: { score: number | null; className?: string }) {
  return (
    <div className={`h-[3px] overflow-hidden rounded-full bg-track ${className}`} aria-hidden="true">
      <div className={`h-full rounded-full ${scoreFill(score)}`} style={{ width: `${score ?? 0}%` }} />
    </div>
  );
}

/** Small tab-like toggle used for filters and groupings. */
export function Pill({
  active,
  children,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { active: boolean }) {
  return (
    <button
      className={`h-6 rounded-[5px] px-2.5 text-xs transition-colors ${
        active ? "bg-sel font-medium text-fg" : "text-dim hover:text-fg"
      }`}
      {...props}
    >
      {children}
    </button>
  );
}

export const inputClass =
  "h-7 rounded-md border border-line bg-canvas px-2 text-xs text-fg outline-none placeholder:text-dim focus:border-accent";

export function timeAgo(unixSeconds: number): string {
  const s = Math.max(0, Date.now() / 1000 - unixSeconds);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  return new Date(unixSeconds * 1000).toLocaleDateString();
}
