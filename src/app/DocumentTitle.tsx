// Document name, edited marker and quick buttons (palette, help) under the brand header.

import { CMD } from '../commands/ids';
import { commands } from '../commands/registry';
import { formatShortcut } from '../commands/shortcuts';
import { documentName, isDirty, useDocumentStore } from '../state/document';
import { useAppStore } from '../state/store';

export function DocumentTitle() {
  const path = useDocumentStore((s) => s.path);
  const savedParams = useDocumentStore((s) => s.savedParams);
  const params = useAppStore((s) => s.params);
  const dirty = isDirty(params, { path, savedParams });
  return (
    <div className="doc-title" title={path ?? 'Not saved yet'}>
      <span className="shell-actions">
        <button type="button" onClick={() => void commands.run(CMD.palette)} title={`Command palette (${formatShortcut('Mod+K')})`}>
          {formatShortcut('Mod+K')}
        </button>
        <button type="button" onClick={() => void commands.run(CMD.help)} title="Help">
          ?
        </button>
        <span>
          {documentName(path)}
          {dirty ? ' — Edited' : ''}
        </span>
      </span>
    </div>
  );
}
