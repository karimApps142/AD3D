import { describe, expect, it, vi } from 'vitest';
import { shellCommands } from '../app/registerCommands';
import { CommandRegistry, fuzzyScore } from './registry';
import menuRs from '../../src-tauri/src/menu.rs?raw';
import { formatShortcut, isTextTarget, matchesShortcut, parseShortcut } from './shortcuts';

const key = (k: string, mods: Partial<{ meta: boolean; ctrl: boolean; shift: boolean; alt: boolean }> = {}, code?: string) => ({
  key: k,
  code,
  metaKey: !!mods.meta,
  ctrlKey: !!mods.ctrl,
  shiftKey: !!mods.shift,
  altKey: !!mods.alt,
});

describe('shortcuts', () => {
  it('parses chords', () => {
    expect(parseShortcut('Mod+Shift+Z')).toEqual({ mod: true, shift: true, alt: false, key: 'z' });
    expect(parseShortcut('F')).toEqual({ mod: false, shift: false, alt: false, key: 'f' });
    expect(parseShortcut('Mod++').key).toBe('+');
    expect(() => parseShortcut('Hyper+K')).toThrow();
  });

  it('matches ⌘ on macOS and Ctrl elsewhere', () => {
    expect(matchesShortcut(key('k', { meta: true }), 'Mod+K', true)).toBe(true);
    expect(matchesShortcut(key('k', { ctrl: true }), 'Mod+K', true)).toBe(false);
    expect(matchesShortcut(key('k', { ctrl: true }), 'Mod+K', false)).toBe(true);
    expect(matchesShortcut(key('Z', { meta: true, shift: true }), 'Mod+Shift+Z', true)).toBe(true);
    expect(matchesShortcut(key('z', { meta: true }), 'Mod+Shift+Z', true)).toBe(false);
    expect(matchesShortcut(key('f'), 'F', true)).toBe(true);
    expect(matchesShortcut(key('f', { meta: true }), 'F', true)).toBe(false);
  });

  it('handles shifted punctuation and Alt-composed keys', () => {
    expect(matchesShortcut(key('?', { shift: true }), 'Shift+/', true)).toBe(true);
    expect(matchesShortcut(key('?', { meta: true, shift: true }), 'Mod+Shift+/', true)).toBe(true);
    expect(matchesShortcut(key('ƒ', { alt: true }, 'KeyF'), 'Alt+F', true)).toBe(true);
  });

  it('formats for each platform', () => {
    expect(formatShortcut('Mod+Shift+Z', true)).toBe('⇧⌘Z');
    expect(formatShortcut('Mod+Shift+Z', false)).toBe('Ctrl+Shift+Z');
    expect(formatShortcut('Escape', true)).toBe('Esc');
  });

  it('knows text targets', () => {
    expect(isTextTarget({ tagName: 'INPUT', type: 'text' } as unknown as EventTarget)).toBe(true);
    expect(isTextTarget({ tagName: 'INPUT', type: 'range' } as unknown as EventTarget)).toBe(false);
    expect(isTextTarget({ tagName: 'TEXTAREA' } as unknown as EventTarget)).toBe(true);
    expect(isTextTarget({ tagName: 'DIV' } as unknown as EventTarget)).toBe(false);
    expect(isTextTarget(null)).toBe(false);
  });
});

describe('CommandRegistry', () => {
  it('registers, runs and unregisters', async () => {
    const r = new CommandRegistry();
    const run = vi.fn();
    const off = r.register({ id: 'a.b', title: 'Thing', category: 'X', run });
    expect(await r.run('a.b', 'arg')).toBe(true);
    expect(run).toHaveBeenCalledWith('arg');
    off();
    expect(await r.run('a.b')).toBe(false);
  });

  it('does not run disabled commands', async () => {
    const r = new CommandRegistry();
    const run = vi.fn();
    r.register({ id: 'x', title: 'X', category: 'X', enabled: () => false, run });
    expect(await r.run('x')).toBe(false);
    expect(run).not.toHaveBeenCalled();
  });

  it('fuzzy search ranks word starts first and skips hidden commands', () => {
    const r = new CommandRegistry();
    r.register({ id: '1', title: 'Save As…', category: 'File', run: () => {} });
    r.register({ id: '2', title: 'Save', category: 'File', run: () => {} });
    r.register({ id: '3', title: 'Show Settings', category: 'App', run: () => {} });
    r.register({ id: '4', title: 'Secret', category: 'App', hidden: true, run: () => {} });
    const ids = r.search('sa').map((h) => h.command.id);
    expect(ids.slice(0, 2)).toEqual(['2', '1']);
    expect(ids).not.toContain('4');
    expect(r.search('zzz')).toEqual([]);
    expect(r.search('').length).toBe(3);
    expect(r.search('file save').map((h) => h.command.id)).toContain('2');
  });

  it('fuzzyScore needs an in-order subsequence', () => {
    expect(fuzzyScore('cp', 'Command Palette')).not.toBeNull();
    expect(fuzzyScore('pc', 'Command Palette')).toBeNull();
  });

  it('keydown dispatch respects text fields and native menus', () => {
    const r = new CommandRegistry();
    r.register({ id: 'frame', title: 'Frame', category: 'View', shortcut: 'F', run: () => {} });
    r.register({ id: 'save', title: 'Save', category: 'File', shortcut: 'Mod+S', menu: true, allowInInput: true, run: () => {} });
    const input = { tagName: 'INPUT', type: 'text' } as unknown as EventTarget;
    expect(r.commandForKey({ ...key('f'), target: null }, false, true)?.id).toBe('frame');
    expect(r.commandForKey({ ...key('f'), target: input }, false, true)).toBeNull();
    expect(r.commandForKey({ ...key('s', { meta: true }), target: input }, false, true)?.id).toBe('save');
    // In the desktop app ⌘S arrives from the menu, so the keydown layer leaves it alone.
    expect(r.commandForKey({ ...key('s', { meta: true }), target: null }, true, true)).toBeNull();
  });
});

describe('shell commands', () => {
  it('cover every id the native menu emits', () => {
    const src = menuRs;
    const block = src.slice(src.indexOf('pub const MENU_IDS'), src.indexOf('];', src.indexOf('pub const MENU_IDS')));
    const ids = [...block.matchAll(/"([a-zA-Z.]+)"/g)].map((m) => m[1]);
    ids.push('app.closeRequested'); // referenced through the CLOSE_REQUESTED constant
    expect(ids.length).toBeGreaterThan(15);
    const registered = new Set(shellCommands().map((c) => c.id));
    for (const id of ids) expect(registered, id).toContain(id);
  });

  it('have unique ids and valid shortcuts', () => {
    const cmds = shellCommands();
    expect(new Set(cmds.map((c) => c.id)).size).toBe(cmds.length);
    const seen = new Set<string>();
    for (const c of cmds) {
      if (!c.shortcut) continue;
      const k = JSON.stringify(parseShortcut(c.shortcut));
      expect(seen, `${c.id} ${c.shortcut}`).not.toContain(k);
      seen.add(k);
    }
  });

  it('every menu accelerator in menu.rs matches the command shortcut', () => {
    const src = menuRs;
    const byId = new Map(shellCommands().map((c) => [c.id, c]));
    const re = /item\(app, "([^"]+)", "[^"]*", Some\("([^"]+)"\)\)/g;
    let n = 0;
    for (const m of src.matchAll(re)) {
      n++;
      const cmd = byId.get(m[1]);
      expect(cmd, m[1]).toBeDefined();
      expect(cmd!.menu, m[1]).toBe(true);
      expect(parseShortcut(cmd!.shortcut!), m[1]).toEqual(parseShortcut(m[2]));
    }
    expect(n).toBeGreaterThan(8);
  });
});
