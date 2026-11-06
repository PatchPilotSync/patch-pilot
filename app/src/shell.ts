import { createContext, useContext } from "react";

/**
 * What a part deep in a screen can ask of the main window, without each
 * screen passing it down: open the Link dialog, go to a tab. Also what the
 * window already knows about the machines, so such a part needn't load it.
 */
export interface Shell {
  /** This machine is linked to another one. */
  linked: boolean;
  /** The latest Primary Rig was made from this machine, so its files are the ones others bring over. */
  isPrimary: boolean;
  /** Opens the Link a machine dialog. */
  link: () => void;
  /** This machine, with its Sync & backup line open. */
  setUpSync: () => void;
  /** The Settings tab. */
  settings: () => void;
  /** This machine's Get Up To Speed, or null when it has none (no Primary Rig, or this is its machine). */
  upToSpeed: (() => void) | null;
}

export const ShellContext = createContext<Shell | null>(null);

/** The main window's {@link Shell}; null in the tray popover. */
export function useShell(): Shell | null {
  return useContext(ShellContext);
}
