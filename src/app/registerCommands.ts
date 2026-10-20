// Registers every app-shell command. Menu ids come from src-tauri/src/menu.rs (MENU_IDS).

import { CMD } from '../commands/ids';
import { type Command, type CommandRegistry, commands } from '../commands/registry';
import { isTextTarget } from '../commands/shortcuts';
import { paramsHistory } from '../state/paramsHistory';
import { useAppStore } from '../state/store';
import { openHelp, useUiStore } from '../state/ui';
import * as actions from './actions';

/** Undo/redo inside a focused text field edits the text, not the parameters. */
function textUndo(kind: 'undo' | 'redo'): boolean {
  if (typeof document === 'undefined' || !isTextTarget(document.activeElement)) return false;
  document.execCommand(kind);
  return true;
}

export function shellCommands(): Command[] {
  return [
    { id: CMD.newProject, title: 'New Project', category: 'File', shortcut: 'Mod+N', menu: true, allowInInput: true, run: actions.newProject },
    { id: CMD.open, title: 'Open…', category: 'File', shortcut: 'Mod+O', menu: true, allowInInput: true, keywords: ['project', 'load'], run: actions.openDialog },
    { id: CMD.openRecent, title: 'Open Recent Project', category: 'File', hidden: true, run: actions.openRecent },
    { id: CMD.clearRecent, title: 'Clear Recent Projects', category: 'File', run: actions.clearRecent },
    { id: CMD.save, title: 'Save', category: 'File', shortcut: 'Mod+S', menu: true, allowInInput: true, run: async () => void (await actions.saveProject()) },
    { id: CMD.saveAs, title: 'Save As…', category: 'File', shortcut: 'Mod+Shift+S', menu: true, allowInInput: true, run: async () => void (await actions.saveProjectAs()) },
    { id: CMD.importImage, title: 'Import Image…', category: 'File', shortcut: 'Mod+I', menu: true, allowInInput: true, keywords: ['photo', 'picture'], run: actions.importImageDialog },
    { id: CMD.export, title: 'Export…', category: 'File', shortcut: 'Mod+E', menu: true, allowInInput: true, keywords: ['stl', '3mf', 'print'], run: actions.exportModel },
    { id: CMD.close, title: 'Close Window', category: 'File', shortcut: 'Mod+W', menu: true, allowInInput: true, run: actions.requestClose },
    { id: CMD.quit, title: 'Quit AD3D', category: 'App', shortcut: 'Mod+Q', menu: true, allowInInput: true, run: actions.requestClose },
    { id: CMD.closeRequested, title: 'Close requested', category: 'App', hidden: true, run: actions.requestClose },
    {
      id: CMD.undo,
      title: 'Undo',
      category: 'Edit',
      shortcut: 'Mod+Z',
      menu: true,
      enabled: () => paramsHistory.canUndo || (typeof document !== 'undefined' && isTextTarget(document.activeElement)),
      run: () => void (textUndo('undo') || paramsHistory.undo()),
    },
    {
      id: CMD.redo,
      title: 'Redo',
      category: 'Edit',
      shortcut: 'Mod+Shift+Z',
      menu: true,
      enabled: () => paramsHistory.canRedo || (typeof document !== 'undefined' && isTextTarget(document.activeElement)),
      run: () => void (textUndo('redo') || paramsHistory.redo()),
    },
    {
      id: CMD.palette,
      title: 'Command Palette…',
      category: 'View',
      shortcut: 'Mod+K',
      menu: true,
      allowInInput: true,
      hidden: true,
      run: () => useUiStore.setState((s) => ({ paletteOpen: !s.paletteOpen })),
    },
    { id: CMD.frame, title: 'Frame Model', category: 'View', shortcut: 'F', keywords: ['fit', 'zoom'], run: () => useAppStore.getState().requestFit() },
    { id: CMD.themeSystem, title: 'Theme: System', category: 'View', keywords: ['appearance'], run: () => actions.setTheme('system') },
    { id: CMD.themeDark, title: 'Theme: Dark', category: 'View', keywords: ['appearance'], run: () => actions.setTheme('dark') },
    { id: CMD.themeLight, title: 'Theme: Light', category: 'View', keywords: ['appearance'], run: () => actions.setTheme('light') },
    { id: CMD.settings, title: 'Settings…', category: 'App', shortcut: 'Mod+,', menu: true, allowInInput: true, keywords: ['preferences', 'printer', 'units'], run: () => useUiStore.setState({ settingsOpen: true, paletteOpen: false }) },
    { id: CMD.about, title: 'About AD3D', category: 'Help', run: () => openHelp('squama') },
    { id: CMD.help, title: 'Help', category: 'Help', shortcut: 'Mod+Shift+/', menu: true, allowInInput: true, keywords: ['docs', 'manual'], run: () => toggleHelp('squama') },
    { id: CMD.shortcuts, title: 'Keyboard Shortcuts', category: 'Help', shortcut: 'Shift+/', keywords: ['keys', 'hotkeys'], run: () => openHelp('shortcuts') },
    { id: CMD.helpSquama, title: 'Shadow Plate Parameters', category: 'Help', keywords: ['squama', 'docs'], run: () => openHelp('squama') },
    { id: CMD.helpChecklist, title: 'Print and Light Checklist', category: 'Help', keywords: ['squama', 'printing'], run: () => openHelp('checklist') },
    {
      id: 'app.escape',
      title: 'Close overlay',
      category: 'App',
      shortcut: 'Escape',
      hidden: true,
      allowInInput: true,
      enabled: () => {
        const s = useUiStore.getState();
        return s.paletteOpen || s.helpOpen || s.settingsOpen;
      },
      run: () => useUiStore.setState({ paletteOpen: false, helpOpen: false, settingsOpen: false }),
    },
  ];
}

function toggleHelp(tab: 'squama' | 'shortcuts' | 'checklist'): void {
  const s = useUiStore.getState();
  if (s.helpOpen && s.helpTab === tab) useUiStore.setState({ helpOpen: false });
  else openHelp(tab);
}

export function registerShellCommands(registry: CommandRegistry = commands): () => void {
  return registry.registerAll(shellCommands());
}
