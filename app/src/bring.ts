import { fileKey, type BringResult, type FileToBring } from "./api";

/**
 * Files that only work together (decision #62): a script and the files it
 * loads, a ReaPack package, a theme and its images. Ticking one ticks what it
 * needs; unticking one unticks what needs it, so nothing arrives half there.
 */
export class Needs {
  private needs = new Map<string, string[]>();
  private neededBy = new Map<string, string[]>();

  constructor(files: FileToBring[]) {
    for (const f of files) {
      const k = fileKey(f);
      this.needs.set(k, f.needs);
      for (const n of f.needs) this.neededBy.set(n, [...(this.neededBy.get(n) ?? []), k]);
    }
  }

  /** `keys` and everything they need, directly or through others. */
  withNeeds(keys: Iterable<string>): Set<string> {
    return walk(keys, (k) => this.needs.get(k));
  }

  /** `keys` and everything that needs them. */
  withNeededBy(keys: Iterable<string>): Set<string> {
    return walk(keys, (k) => this.neededBy.get(k));
  }
}

function walk(start: Iterable<string>, next: (k: string) => string[] | undefined): Set<string> {
  const seen = new Set<string>();
  const todo = [...start];
  while (todo.length) {
    const k = todo.pop()!;
    if (seen.has(k)) continue;
    seen.add(k);
    todo.push(...(next(k) ?? []));
  }
  return seen;
}

const files = (n: number) => `${n} file${n === 1 ? "" : "s"}`;

/**
 * What a bring-over did, in words, with why anything was left out. `from` is
 * where the files came from ("the Primary machine", "Studio PC").
 */
export function bringSummary(r: BringResult, from: string): { text: string; warn: string | null } {
  let text: string;
  if (r.written > 0) {
    text = `Brought over ${files(r.written)}`;
    if (r.companions > 0) text += `, including ${r.companions} that the ones you picked need`;
    text += ".";
    if (r.backed_up > 0) text += ` ${files(r.backed_up)} replaced here ${r.backed_up === 1 ? "was" : "were"} saved first.`;
    text += " You can undo this in History.";
  } else if (r.same > 0) {
    text = `Already here: this machine had the same ${files(r.same)}, so nothing needed copying.`;
  } else {
    text = "Nothing was brought over.";
  }
  const warn: string[] = [];
  if (r.unavailable.length > 0)
    warn.push(
      `${files(r.unavailable.length)} not available yet (${short(r.unavailable)}): open Patch Pilot on ${from} so it can send ${r.unavailable.length === 1 ? "it" : "them"}.`,
    );
  if (r.held_back.length > 0)
    warn.push(
      `${files(r.held_back.length)} left behind (${short(r.held_back)}) because a file ${r.held_back.length === 1 ? "it needs" : "they need"} didn't arrive; ${r.held_back.length === 1 ? "it" : "they"} wouldn't work without it.`,
    );
  return { text, warn: warn.length ? warn.join(" ") : null };
}

/** "a.lua, b.lua and 3 more" */
function short(paths: string[]): string {
  const names = paths.map((p) => p.slice(p.lastIndexOf("/") + 1));
  return names.length <= 2 ? names.join(" and ") : `${names.slice(0, 2).join(", ")} and ${names.length - 2} more`;
}
