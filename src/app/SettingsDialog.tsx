// Settings dialog (⌘,): theme, length display units, default printer, history and recent files.

import { type Settings, type Theme, type UnitsDisplay, useSettingsStore } from '../state/settings';
import { useUiStore } from '../state/ui';
import { clearRecent, updateSettings } from './actions';

export function SettingsDialog() {
  const open = useUiStore((s) => s.settingsOpen);
  const settings = useSettingsStore((s) => s.settings);
  const printers = useSettingsStore((s) => s.printers);
  if (!open) return null;
  const close = () => useUiStore.setState({ settingsOpen: false });
  const set = (patch: Partial<Settings>) => void updateSettings(patch);

  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="dialog" role="dialog" aria-label="Settings">
        <header className="dialog-head">
          <h2>Settings</h2>
          <button type="button" className="icon-btn" aria-label="Close settings" onClick={close}>
            ×
          </button>
        </header>
        <div className="form-grid">
          <label htmlFor="set-theme">Theme</label>
          <select id="set-theme" value={settings.theme} onChange={(e) => set({ theme: e.currentTarget.value as Theme })}>
            <option value="system">System</option>
            <option value="dark">Dark</option>
            <option value="light">Light</option>
          </select>

          <label htmlFor="set-units">Show lengths in</label>
          <select id="set-units" value={settings.unitsDisplay} onChange={(e) => set({ unitsDisplay: e.currentTarget.value as UnitsDisplay })}>
            <option value="mm">Millimetres</option>
            <option value="cm">Centimetres</option>
            <option value="inch">Inches</option>
          </select>

          <label htmlFor="set-printer">Default printer</label>
          <select id="set-printer" value={settings.defaultPrinter} onChange={(e) => set({ defaultPrinter: e.currentTarget.value })}>
            {printers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name} ({p.bedMm.join(' × ')} mm)
              </option>
            ))}
          </select>

          <label htmlFor="set-undo">Undo steps</label>
          <input
            id="set-undo"
            type="number"
            min={10}
            max={2000}
            step={10}
            value={settings.undoDepth}
            onChange={(e) => Number.isFinite(e.currentTarget.valueAsNumber) && set({ undoDepth: e.currentTarget.valueAsNumber })}
          />

          <label htmlFor="set-recent">Recent projects kept</label>
          <input
            id="set-recent"
            type="number"
            min={0}
            max={50}
            value={settings.recentMax}
            onChange={(e) => Number.isFinite(e.currentTarget.valueAsNumber) && set({ recentMax: e.currentTarget.valueAsNumber })}
          />
        </div>
        <p className="note">Everything is stored and exported in millimetres; the unit setting only changes how lengths are shown.</p>
        <footer className="dialog-foot">
          <button type="button" onClick={() => void clearRecent()} disabled={settings.recentFiles.length === 0}>
            Clear recent ({settings.recentFiles.length})
          </button>
          <button type="button" onClick={() => set({ showHelpOnStart: true })}>
            Show help on next start
          </button>
          <button type="button" className="primary" onClick={close}>
            Done
          </button>
        </footer>
      </div>
    </div>
  );
}
