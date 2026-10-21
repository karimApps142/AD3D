// Boots the app shell once: commands, settings, undo history, menu events, keyboard shortcuts,
// slider-gesture tracking (for undo coalescing), file drops, theme and the window title.

import { useEffect } from 'react';
import { commands } from '../commands/registry';
import { documentName, isDirty, pathKind, useDocumentStore, windowTitle } from '../state/document';
import { paramsHistory, startParamsHistory, useHistoryStatus } from '../state/paramsHistory';
import { effectiveTheme, useSettingsStore } from '../state/settings';
import { useAppStore } from '../state/store';
import { openHelp, toast, useUiStore } from '../state/ui';
import * as actions from './actions';
import { type DropEvent, getHost } from './host';
import { registerShellCommands } from './registerCommands';

/** Pointer-down on any range input (or an element marked data-history-gesture) starts a gesture. */
function isGestureTarget(t: EventTarget | null): boolean {
  const el = t as HTMLElement | null;
  if (!el || typeof el.closest !== 'function') return false;
  return !!el.closest('input[type="range"], [data-history-gesture]');
}

function applyTheme(): () => void {
  const media = typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: dark)') : null;
  const apply = () => {
    const theme = useSettingsStore.getState().settings.theme;
    document.documentElement.dataset.theme = effectiveTheme(theme, media?.matches ?? true);
  };
  apply();
  const off = useSettingsStore.subscribe((s, p) => {
    if (s.settings.theme !== p.settings.theme) apply();
  });
  media?.addEventListener('change', apply);
  return () => {
    off();
    media?.removeEventListener('change', apply);
  };
}

function syncMenuState(): () => void {
  let queued = false;
  let last = '';
  const push = () => {
    queued = false;
    const doc = useDocumentStore.getState();
    const dirty = isDirty(useAppStore.getState().params, doc);
    const { canUndo, canRedo } = useHistoryStatus.getState();
    const state = { canUndo, canRedo, title: windowTitle(doc.path, dirty) };
    const key = JSON.stringify(state);
    if (key === last) return;
    last = key;
    getHost()
      .setMenuState(state)
      .catch((err) => console.warn('[shell] menu state', err));
  };
  const schedule = () => {
    if (queued) return;
    queued = true;
    queueMicrotask(push);
  };
  const offs = [
    useAppStore.subscribe((s, p) => s.params !== p.params && schedule()),
    useDocumentStore.subscribe(schedule),
    useHistoryStatus.subscribe(schedule),
  ];
  push();
  return () => offs.forEach((o) => o());
}

async function handleDrop(e: DropEvent): Promise<void> {
  if (e.type === 'enter' || e.type === 'over') {
    if (!useUiStore.getState().dropActive) useUiStore.setState({ dropActive: true });
    return;
  }
  useUiStore.setState({ dropActive: false });
  if (e.type !== 'drop') return;
  const host = getHost();
  if (!host.native) {
    toast('Dropping files needs the desktop app (a browser does not expose file paths).');
    return;
  }
  const project = e.paths.find((p) => pathKind(p) === 'project');
  const image = e.paths.find((p) => pathKind(p) === 'image');
  if (project) await actions.openPath(project);
  else if (image) await actions.useImage(image);
  else if (e.paths.length) toast(`${documentName(e.paths[0])}: drop an image or an .ad3d project`, 'error');
}

export function useAppShell(): void {
  useEffect(() => {
    const host = getHost();
    const cleanups: Array<() => void> = [];
    let disposed = false;
    const later = (p: Promise<() => void>) =>
      p.then((off) => (disposed ? off() : cleanups.push(off))).catch((err) => console.warn('[shell]', err));

    cleanups.push(registerShellCommands());
    cleanups.push(startParamsHistory({ limit: useSettingsStore.getState().settings.undoDepth }));
    cleanups.push(actions.startImageSync());
    paramsHistory.reset();
    useDocumentStore.setState({ savedParams: useAppStore.getState().params });
    cleanups.push(applyTheme());

    // Menu events → commands. menu_set_state (in syncMenuState) tells the host the UI is ready
    // to answer close requests, so start it only once the listener is in place.
    later(
      host
        .onMenu((m) => {
          commands.run(m.id, m.arg).catch((err) => toast(String(err), 'error'));
        })
        .then((off) => {
          if (!disposed) cleanups.push(syncMenuState());
          return off;
        }),
    );
    later(host.onDrop((e) => void handleDrop(e)));

    actions.loadSettings().then(() => {
      const s = useSettingsStore.getState().settings;
      if (s.showHelpOnStart && !disposed) {
        openHelp('squama');
        void actions.updateSettings({ showHelpOnStart: false });
      }
    });

    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.isComposing) return;
      const cmd = commands.commandForKey(e, host.native);
      if (!cmd || !commands.isEnabled(cmd)) return;
      e.preventDefault();
      commands.run(cmd.id).catch((err) => toast(String(err), 'error'));
    };
    const onDown = (e: PointerEvent) => {
      if (isGestureTarget(e.target)) paramsHistory.beginGesture();
    };
    const onUp = () => paramsHistory.endGesture();
    // Browser only: the native host asks through app.closeRequested instead.
    const onBeforeUnload = (e: BeforeUnloadEvent) => {
      if (!host.native && actions.currentDirty()) e.preventDefault();
    };
    window.addEventListener('keydown', onKey);
    window.addEventListener('pointerdown', onDown, true);
    window.addEventListener('pointerup', onUp, true);
    window.addEventListener('pointercancel', onUp, true);
    window.addEventListener('blur', onUp);
    window.addEventListener('beforeunload', onBeforeUnload);
    cleanups.push(() => {
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('pointerdown', onDown, true);
      window.removeEventListener('pointerup', onUp, true);
      window.removeEventListener('pointercancel', onUp, true);
      window.removeEventListener('blur', onUp);
      window.removeEventListener('beforeunload', onBeforeUnload);
    });

    return () => {
      disposed = true;
      cleanups.splice(0).forEach((off) => off());
    };
  }, []);
}
