// Command registry: one place that knows every action the app can run. The native menu, the
// command palette (⌘K) and keyboard shortcuts all dispatch through it, so a command behaves the
// same whichever way it is invoked.

import { isMacPlatform, isTextTarget, type KeyLike, matchesShortcut } from './shortcuts';

export interface Command {
  /** Stable id, e.g. "file.save" (see ids.ts). */
  id: string;
  /** Palette and menu title. */
  title: string;
  /** Palette group: "File", "Edit", "View", "Help", … */
  category: string;
  /** Shortcut in shortcuts.ts syntax ("Mod+S", "F"). */
  shortcut?: string;
  /** Extra words the palette search matches. */
  keywords?: string[];
  /**
   * The native menu owns this shortcut (it has a menu accelerator). In the desktop app the menu
   * fires it, so the keydown layer must not fire it again.
   */
  menu?: boolean;
  /** Fire the shortcut even while typing in a text field (e.g. ⌘S, ⌘K). */
  allowInInput?: boolean;
  /** Hidden from the palette (internal commands such as app.closeRequested). */
  hidden?: boolean;
  /** Greyed out when false. */
  enabled?: () => boolean;
  run: (arg?: string) => void | Promise<void>;
}

export interface SearchHit {
  command: Command;
  score: number;
}

/**
 * Subsequence fuzzy score: every query character must appear in order. Word starts and runs
 * score higher; null when the query does not match.
 */
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.toLowerCase().replace(/\s+/g, '');
  if (!q) return 0;
  const t = text.toLowerCase();
  let score = 0;
  let ti = 0;
  let prev = -2;
  for (const ch of q) {
    const found = t.indexOf(ch, ti);
    if (found < 0) return null;
    const wordStart = found === 0 || /[\s./\-_:(]/.test(t[found - 1]);
    score += 1 + (wordStart ? 3 : 0) + (found === prev + 1 ? 2 : 0);
    prev = found;
    ti = found + 1;
  }
  // Prefer shorter titles for equal matches.
  return score - t.length * 0.01;
}

export class CommandRegistry {
  private readonly commands = new Map<string, Command>();
  private readonly listeners = new Set<() => void>();
  private version = 0;

  register(cmd: Command): () => void {
    this.commands.set(cmd.id, cmd);
    this.changed();
    return () => {
      if (this.commands.get(cmd.id) === cmd) {
        this.commands.delete(cmd.id);
        this.changed();
      }
    };
  }

  registerAll(cmds: Command[]): () => void {
    const offs = cmds.map((c) => this.register(c));
    return () => offs.forEach((off) => off());
  }

  get(id: string): Command | undefined {
    return this.commands.get(id);
  }

  has(id: string): boolean {
    return this.commands.has(id);
  }

  list(): Command[] {
    return [...this.commands.values()];
  }

  isEnabled(cmd: Command): boolean {
    try {
      return cmd.enabled ? cmd.enabled() : true;
    } catch {
      return false;
    }
  }

  /** Runs a command; resolves false when it is unknown or disabled. Errors are rethrown. */
  async run(id: string, arg?: string): Promise<boolean> {
    const cmd = this.commands.get(id);
    if (!cmd || !this.isEnabled(cmd)) return false;
    await cmd.run(arg);
    return true;
  }

  /** Palette search: visible commands ranked by fuzzy score over title, category and keywords. */
  search(query: string): SearchHit[] {
    const hits: SearchHit[] = [];
    for (const command of this.commands.values()) {
      if (command.hidden) continue;
      const fields = [command.title, `${command.category} ${command.title}`, ...(command.keywords ?? [])];
      let best: number | null = null;
      for (const f of fields) {
        const s = fuzzyScore(query, f);
        if (s !== null && (best === null || s > best)) best = s;
      }
      if (best !== null) hits.push({ command, score: best });
    }
    hits.sort((a, b) => b.score - a.score || a.command.title.localeCompare(b.command.title));
    return hits;
  }

  /**
   * Finds the command a keydown should fire. `native` = running in the desktop shell, where
   * menu-owned shortcuts arrive as menu events instead.
   */
  commandForKey(e: KeyLike & { target?: EventTarget | null }, native: boolean, mac = isMacPlatform()): Command | null {
    const typing = isTextTarget(e.target ?? null);
    for (const cmd of this.commands.values()) {
      if (!cmd.shortcut) continue;
      if (native && cmd.menu) continue;
      if (typing && !cmd.allowInInput) continue;
      if (matchesShortcut(e, cmd.shortcut, mac)) return cmd;
    }
    return null;
  }

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  getVersion(): number {
    return this.version;
  }

  private changed(): void {
    this.version++;
    for (const fn of this.listeners) fn();
  }
}

/** The app's registry. */
export const commands = new CommandRegistry();
