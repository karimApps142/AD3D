// Keyboard shortcut strings: "Mod+Shift+Z", "Mod+K", "F", "Shift+/".
// "Mod" is ⌘ on macOS and Ctrl elsewhere. Pure functions, so they are unit-tested.

export interface Chord {
  mod: boolean;
  shift: boolean;
  alt: boolean;
  /** Lower-case key name: "z", "k", ",", "/", "f", "escape", "enter". */
  key: string;
}

/** Minimal shape of a KeyboardEvent, so tests need no DOM. */
export interface KeyLike {
  key: string;
  code?: string;
  metaKey: boolean;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}

export function isMacPlatform(): boolean {
  if (typeof navigator === 'undefined') return true;
  return /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
}

export function parseShortcut(s: string): Chord {
  const parts = s.split('+').map((p) => p.trim());
  // "Mod++" would split into an empty tail; keep "+" as the key.
  const key = parts.pop() || '+';
  const chord: Chord = { mod: false, shift: false, alt: false, key: key.toLowerCase() };
  for (const p of parts) {
    const m = p.toLowerCase();
    if (m === 'mod' || m === 'cmd' || m === 'ctrl' || m === 'cmdorctrl') chord.mod = true;
    else if (m === 'shift') chord.shift = true;
    else if (m === 'alt' || m === 'option') chord.alt = true;
    else if (m !== '') throw new Error(`unknown modifier ${JSON.stringify(p)} in ${JSON.stringify(s)}`);
  }
  return chord;
}

/** Shifted punctuation on a US layout, so "Shift+/" also matches an event whose key is "?". */
const SHIFTED: Record<string, string> = { '?': '/', '<': ',', '>': '.', '{': '[', '}': ']', '+': '=', _: '-' };

function eventKey(e: KeyLike): string {
  // With Alt held, macOS reports the composed character (Alt+F = "ƒ"); use the physical key.
  if (e.altKey && e.code?.startsWith('Key')) return e.code.slice(3).toLowerCase();
  const k = e.key.toLowerCase();
  return SHIFTED[k] ?? k;
}

export function matchesChord(e: KeyLike, c: Chord, mac = isMacPlatform()): boolean {
  const mod = mac ? e.metaKey : e.ctrlKey;
  const otherMod = mac ? e.ctrlKey : e.metaKey;
  if (mod !== c.mod || otherMod) return false;
  if (e.altKey !== c.alt) return false;
  if (e.shiftKey !== c.shift) return false;
  return eventKey(e) === c.key;
}

export function matchesShortcut(e: KeyLike, shortcut: string, mac = isMacPlatform()): boolean {
  return matchesChord(e, parseShortcut(shortcut), mac);
}

const KEY_GLYPHS: Record<string, string> = {
  escape: 'Esc',
  enter: '↩',
  backspace: '⌫',
  delete: '⌦',
  arrowup: '↑',
  arrowdown: '↓',
  arrowleft: '←',
  arrowright: '→',
  ' ': 'Space',
};

/** "Mod+Shift+Z" → "⇧⌘Z" on macOS, "Ctrl+Shift+Z" elsewhere. */
export function formatShortcut(s: string, mac = isMacPlatform()): string {
  const c = parseShortcut(s);
  const key = KEY_GLYPHS[c.key] ?? c.key.toUpperCase();
  if (mac) return `${c.alt ? '⌥' : ''}${c.shift ? '⇧' : ''}${c.mod ? '⌘' : ''}${key}`;
  return [c.mod ? 'Ctrl' : '', c.alt ? 'Alt' : '', c.shift ? 'Shift' : '', key].filter(Boolean).join('+');
}

/** True when the event target is a text field (typing there must not trigger plain-key shortcuts). */
export function isTextTarget(target: EventTarget | null): boolean {
  const el = target as (HTMLElement & { type?: string }) | null;
  if (!el || typeof el.tagName !== 'string') return false;
  if (el.isContentEditable) return true;
  if (el.tagName === 'TEXTAREA' || el.tagName === 'SELECT') return true;
  if (el.tagName !== 'INPUT') return false;
  const t = (el.type || 'text').toLowerCase();
  return !['range', 'checkbox', 'radio', 'button', 'submit', 'color'].includes(t);
}
