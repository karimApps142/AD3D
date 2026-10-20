// Shell UI state: overlays (palette, help, settings), the unsaved-changes prompt and toasts.

import { create } from 'zustand';

export type HelpTab = 'squama' | 'shortcuts' | 'checklist';
export type UnsavedChoice = 'save' | 'discard' | 'cancel';

export interface Toast {
  id: number;
  kind: 'info' | 'error';
  text: string;
}

export interface UiState {
  paletteOpen: boolean;
  helpOpen: boolean;
  helpTab: HelpTab;
  settingsOpen: boolean;
  /** Drag of files over the window in progress. */
  dropActive: boolean;
  /** Pending unsaved-changes question, if any. */
  unsaved: { docName: string; resolve: (c: UnsavedChoice) => void } | null;
  toasts: Toast[];
}

export const useUiStore = create<UiState>()(() => ({
  paletteOpen: false,
  helpOpen: false,
  helpTab: 'squama',
  settingsOpen: false,
  dropActive: false,
  unsaved: null,
  toasts: [],
}));

let toastId = 0;

export function toast(text: string, kind: Toast['kind'] = 'info', ms = 4000): void {
  const id = ++toastId;
  useUiStore.setState((s) => ({ toasts: [...s.toasts.slice(-3), { id, kind, text }] }));
  if (ms > 0) setTimeout(() => dismissToast(id), ms);
}

export function dismissToast(id: number): void {
  useUiStore.setState((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
}

/** Asks Save / Don't Save / Cancel. A second question while one is open cancels the first. */
export function askUnsaved(docName: string): Promise<UnsavedChoice> {
  return new Promise((resolve) => {
    const prev = useUiStore.getState().unsaved;
    prev?.resolve('cancel');
    useUiStore.setState({
      unsaved: {
        docName,
        resolve: (c) => {
          useUiStore.setState({ unsaved: null });
          resolve(c);
        },
      },
    });
  });
}

export function openHelp(tab: HelpTab = 'squama'): void {
  useUiStore.setState({ helpOpen: true, helpTab: tab, paletteOpen: false });
}
