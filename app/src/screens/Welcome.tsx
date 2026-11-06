import { Mark } from "../ui";

// The first-run screen is the questions in FirstRun.tsx (decision #121).

/** The logo in a small framed square, over the first-run screens' titles. */
export function AppMark() {
  return (
    <span className="mb-3 flex size-10 items-center justify-center rounded-lg border border-line bg-panel">
      <Mark size={22} className="text-accent-text" />
    </span>
  );
}
