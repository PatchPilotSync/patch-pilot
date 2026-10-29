import { Component, useEffect, useState, type ErrorInfo, type ReactNode } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { Button, Mark } from "./ui";
import { UpdateBar } from "./Update";

// A render error unmounts the whole tree, which used to leave a blank window
// (0.4.48-0.4.50). This catches it and says what broke, with a way to copy
// the details and to try again. The main window keeps its update offer, so a
// build that crashes can still update itself to one that doesn't.

type Props = { children: ReactNode; updates?: boolean };
type State = { error: Error | null; stack: string };

export class CrashBoundary extends Component<Props, State> {
  state: State = { error: null, stack: "" };

  static getDerivedStateFromError(error: unknown): Partial<State> {
    return { error: error instanceof Error ? error : new Error(String(error)) };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    console.error("Patch Pilot crashed", error, info.componentStack);
    this.setState({ stack: info.componentStack ?? "" });
  }

  render() {
    const { error, stack } = this.state;
    if (!error) return this.props.children;
    return (
      <div className="flex h-full flex-col">
        {this.props.updates && <UpdateBar />}
        <Crashed error={error} componentStack={stack} onRetry={() => this.setState({ error: null, stack: "" })} />
      </div>
    );
  }
}

function Crashed({ error, componentStack, onRetry }: { error: Error; componentStack: string; onRetry: () => void }) {
  const [version, setVersion] = useState("");
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    getVersion().then(setVersion, () => {});
  }, []);
  const details = [
    `Patch Pilot ${version}`.trim(),
    // Chromium's stack starts with the message; WebKit's doesn't.
    error.stack?.includes(error.message) ? error.stack : `${error.name}: ${error.message}\n${error.stack ?? ""}`,
    componentStack.trim() ? `Component stack:${componentStack}` : "",
  ]
    .filter(Boolean)
    .join("\n");

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-auto bg-canvas p-4 text-xs">
      <div className="flex items-center gap-2">
        <Mark className="text-accent-text" />
        <span className="font-semibold">Patch Pilot hit a problem</span>
      </div>
      <p className="text-dim">
        This screen couldn't be drawn. Your rig and files are untouched. Try again, or copy the details and send them
        to us.
      </p>
      <p className="font-medium text-bad">{error.message || error.name}</p>
      <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap rounded-md border border-line bg-panel p-2 text-[11px] text-dim select-text">
        {details}
      </pre>
      <div className="flex shrink-0 items-center justify-end gap-1">
        <Button
          variant="quiet"
          onClick={() =>
            navigator.clipboard.writeText(details).then(
              () => setCopied(true),
              () => {},
            )
          }
        >
          {copied ? "Copied" : "Copy details"}
        </Button>
        <Button variant="quiet" onClick={() => location.reload()}>
          Reload
        </Button>
        <Button onClick={onRetry}>Try again</Button>
      </div>
    </div>
  );
}
