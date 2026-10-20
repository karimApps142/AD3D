// Command ids shared by the native menu (src-tauri/src/menu.rs MENU_IDS), the command palette
// and keyboard shortcuts. A test checks that every id the menu can emit is registered.

export const CMD = {
  about: 'help.about',
  settings: 'app.settings',
  quit: 'app.quit',
  closeRequested: 'app.closeRequested',
  newProject: 'file.new',
  open: 'file.open',
  openRecent: 'file.openRecent',
  clearRecent: 'file.clearRecent',
  save: 'file.save',
  saveAs: 'file.saveAs',
  importImage: 'file.importImage',
  export: 'file.export',
  close: 'file.close',
  undo: 'edit.undo',
  redo: 'edit.redo',
  palette: 'view.commandPalette',
  frame: 'view.frame',
  themeSystem: 'view.theme.system',
  themeDark: 'view.theme.dark',
  themeLight: 'view.theme.light',
  help: 'help.open',
  shortcuts: 'help.shortcuts',
  helpSquama: 'help.squama',
  helpChecklist: 'help.checklist',
} as const;

export type CommandId = (typeof CMD)[keyof typeof CMD];
