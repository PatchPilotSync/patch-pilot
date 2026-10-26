import { useCallback, useState, type ButtonHTMLAttributes, type ReactNode } from "react";

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

/**
 * Open/closed state for a set of collapsible folders, remembered across launches.
 * `scope` keeps lists apart (e.g. "rig.vendor"); null forgets on close, for short-lived
 * lists. Folders not yet toggled use `fallback`.
 */
export function useFolders(scope: string | null, fallback: (id: string) => boolean = () => false) {
  const key = scope && `pp.folders.${scope}`;
  const [saved, setSaved] = useState<Record<string, boolean>>(() => {
    if (!key) return {};
    try {
      return JSON.parse(localStorage.getItem(key) ?? "{}");
    } catch {
      return {};
    }
  });
  const save = useCallback(
    (update: (prev: Record<string, boolean>) => Record<string, boolean>) =>
      setSaved((prev) => {
        const next = update(prev);
        try {
          if (key) localStorage.setItem(key, JSON.stringify(next));
        } catch {
          // Not remembered; still works for this session.
        }
        return next;
      }),
    [key],
  );
  return {
    isOpen: (id: string) => saved[id] ?? fallback(id),
    toggle: (id: string) => save((prev) => ({ ...prev, [id]: !(prev[id] ?? fallback(id)) })),
    setAll: (ids: string[], open: boolean) =>
      save((prev) => ({ ...prev, ...Object.fromEntries(ids.map((id) => [id, open])) })),
  };
}

/** A collapsible group: a one-line header (chevron, title, count, badges) over its rows. */
export function Folder({
  title,
  count,
  badges,
  actions,
  open,
  onToggle,
  className = "px-4",
  indent,
  children,
}: {
  title: ReactNode;
  count: number;
  badges?: ReactNode;
  /** Buttons or a checkbox at the header's end, outside the toggle. */
  actions?: ReactNode;
  open: boolean;
  onToggle: () => void;
  /** Horizontal padding of the header, to line up with the rows. */
  className?: string;
  /** Left padding in pixels, for nested folders (overrides the className's). */
  indent?: number;
  children: ReactNode;
}) {
  return (
    <section>
      <div
        className={`flex h-7 w-full items-center gap-1.5 hover:bg-sel/60 ${className}`}
        style={indent === undefined ? undefined : { paddingLeft: indent }}
      >
        <button
          onClick={onToggle}
          aria-expanded={open}
          className="flex h-full min-w-0 flex-1 items-center gap-1.5 text-left"
        >
          <svg
            width="10"
            height="10"
            viewBox="0 0 10 10"
            aria-hidden="true"
            className={`shrink-0 text-dim transition-transform ${open ? "rotate-90" : ""}`}
          >
            <path d="M3.5 2 6.5 5 3.5 8" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
          <span className="min-w-0 truncate font-medium">{title}</span>
          <span className="tabular-nums text-xs text-dim">{count}</span>
          <span className="flex-1" />
          {badges}
        </button>
        {actions}
      </div>
      {open && children}
    </section>
  );
}

/** "Expand all · Collapse all" for a list of folders. */
export function FolderControls({
  ids,
  folders,
  className = "px-3",
}: {
  ids: string[];
  folders: ReturnType<typeof useFolders>;
  className?: string;
}) {
  if (ids.length < 2) return null;
  return (
    <div className={`flex justify-end gap-1 ${className}`}>
      <Button variant="quiet" className="h-6 px-2" onClick={() => folders.setAll(ids, true)}>
        Expand all
      </Button>
      <Button variant="quiet" className="h-6 px-2" onClick={() => folders.setAll(ids, false)}>
        Collapse all
      </Button>
    </div>
  );
}

/** One folder of a path tree: subfolders first, then the items directly inside. */
export interface TreeNode<T> {
  name: string;
  /** The folder's path from the tree's root, for remembering it open. */
  id: string;
  dirs: TreeNode<T>[];
  items: T[];
  count: number;
}

/**
 * Items nested by their folder path (`dirsOf` gives the folder names, outermost
 * first), A to Z. A folder holding only one subfolder merges with it
 * ("ReaTeam Scripts / Items Editing") so deep paths don't cost a click each.
 */
export function buildTree<T>(items: T[], dirsOf: (item: T) => string[], id = ""): TreeNode<T> {
  const root: TreeNode<T> = { name: "", id, dirs: [], items: [], count: 0 };
  for (const item of items) {
    let node = root;
    node.count++;
    for (const dir of dirsOf(item)) {
      let next = node.dirs.find((d) => d.name === dir);
      if (!next) node.dirs.push((next = { name: dir, id: `${node.id}/${dir}`, dirs: [], items: [], count: 0 }));
      next.count++;
      node = next;
    }
    node.items.push(item);
  }
  const tidy = (n: TreeNode<T>): TreeNode<T> => {
    let dirs = n.dirs.map(tidy);
    dirs = dirs.map((d) =>
      d.items.length === 0 && d.dirs.length === 1 ? { ...d.dirs[0], name: `${d.name} / ${d.dirs[0].name}` } : d,
    );
    dirs.sort((a, b) => a.name.localeCompare(b.name));
    return { ...n, dirs };
  };
  return tidy(root);
}

/** Every item in a folder and its subfolders. */
export function treeItems<T>(node: TreeNode<T>): T[] {
  return [...node.items, ...node.dirs.flatMap(treeItems)];
}

/** A path tree as nested folders, each level indented `step` pixels more. */
export function FolderTree<T>({
  node,
  folders,
  indent,
  step = 14,
  renderItems,
  actions,
}: {
  node: TreeNode<T>;
  folders: ReturnType<typeof useFolders>;
  /** Left padding of this level's folder headers, in pixels. */
  indent: number;
  step?: number;
  /** The items directly in a folder, given the left padding for their rows. */
  renderItems: (items: T[], indent: number) => ReactNode;
  /** Header buttons for a folder, e.g. "Bring over 12". */
  actions?: (node: TreeNode<T>) => ReactNode;
}) {
  return (
    <>
      {node.dirs.map((d) => (
        <Folder
          key={d.id}
          title={d.name}
          count={d.count}
          className="rounded-[5px] pr-2"
          indent={indent}
          open={folders.isOpen(d.id)}
          onToggle={() => folders.toggle(d.id)}
          actions={actions?.(d)}
        >
          <FolderTree
            node={d}
            folders={folders}
            indent={indent + step}
            step={step}
            renderItems={renderItems}
            actions={actions}
          />
        </Folder>
      ))}
      {node.items.length > 0 && renderItems(node.items, indent + 16)}
    </>
  );
}

/** Dropbox, Google Drive and OneDrive name the copies they make when two machines change one file like this. */
export function isConflictCopy(path: string): boolean {
  return /conflicted copy|conflict copy|\(conflict\)|-conflict-/i.test(path);
}
