// Everything the app shell draws over the workspace: palette, help, settings, the unsaved-changes
// prompt, the drop target and toasts.

import { useEffect, useRef } from 'react';
import { dismissToast, useUiStore } from '../state/ui';
import { CommandPalette } from './CommandPalette';
import { HelpDrawer } from './HelpDrawer';
import { SettingsDialog } from './SettingsDialog';
import './shell.css';

function UnsavedPrompt() {
  const pending = useUiStore((s) => s.unsaved);
  const saveBtn = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (pending) saveBtn.current?.focus();
  }, [pending]);
  if (!pending) return null;
  return (
    <div className="overlay">
      <div
        className="dialog small"
        role="alertdialog"
        aria-labelledby="unsaved-title"
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            e.stopPropagation();
            pending.resolve('cancel');
          }
        }}
      >
        <h2 id="unsaved-title">Save changes to “{pending.docName}”?</h2>
        <p className="note">Your changes will be lost if you don’t save them.</p>
        <footer className="dialog-foot">
          <button type="button" onClick={() => pending.resolve('discard')}>
            Don’t Save
          </button>
          <span className="spacer" />
          <button type="button" onClick={() => pending.resolve('cancel')}>
            Cancel
          </button>
          <button type="button" className="primary" ref={saveBtn} onClick={() => pending.resolve('save')}>
            Save
          </button>
        </footer>
      </div>
    </div>
  );
}

function DropTarget() {
  const active = useUiStore((s) => s.dropActive);
  if (!active) return null;
  return (
    <div className="drop-target" aria-hidden>
      <div>Drop an image or an .ad3d project</div>
    </div>
  );
}

function Toasts() {
  const toasts = useUiStore((s) => s.toasts);
  if (toasts.length === 0) return null;
  return (
    <div className="toasts" role="status">
      {toasts.map((t) => (
        <div key={t.id} className={`toast ${t.kind}`} onClick={() => dismissToast(t.id)}>
          {t.text}
        </div>
      ))}
    </div>
  );
}

export function ShellOverlays() {
  return (
    <>
      <HelpDrawer />
      <CommandPalette />
      <SettingsDialog />
      <UnsavedPrompt />
      <DropTarget />
      <Toasts />
    </>
  );
}
