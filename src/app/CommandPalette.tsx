// ⌘K command palette: fuzzy search over every registered command.

import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react';
import { commands } from '../commands/registry';
import { formatShortcut } from '../commands/shortcuts';
import { toast, useUiStore } from '../state/ui';

export function CommandPalette() {
  const open = useUiStore((s) => s.paletteOpen);
  if (!open) return null;
  return <PaletteBody />;
}

function PaletteBody() {
  const [query, setQuery] = useState('');
  const [index, setIndex] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const version = useSyncExternalStore(
    (fn) => commands.subscribe(fn),
    () => commands.getVersion(),
  );
  const hits = useMemo(() => commands.search(query), [query, version]);
  const close = () => useUiStore.setState({ paletteOpen: false });

  useEffect(() => input.current?.focus(), []);
  useEffect(() => setIndex(0), [query]);
  useEffect(() => {
    list.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
  }, [index]);

  const runAt = (i: number) => {
    const hit = hits[i];
    if (!hit || !commands.isEnabled(hit.command)) return;
    close();
    commands.run(hit.command.id).catch((err) => toast(String(err), 'error'));
  };

  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="palette" role="dialog" aria-label="Command palette">
        <input
          ref={input}
          className="palette-input"
          placeholder="Type a command…"
          value={query}
          aria-label="Command"
          onChange={(e) => setQuery(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === 'ArrowDown') {
              e.preventDefault();
              setIndex((i) => Math.min(hits.length - 1, i + 1));
            } else if (e.key === 'ArrowUp') {
              e.preventDefault();
              setIndex((i) => Math.max(0, i - 1));
            } else if (e.key === 'Enter') {
              e.preventDefault();
              runAt(index);
            } else if (e.key === 'Escape') {
              e.preventDefault();
              e.stopPropagation();
              close();
            }
          }}
        />
        <ul className="palette-list" role="listbox" ref={list}>
          {hits.length === 0 && <li className="palette-empty">No matching command</li>}
          {hits.map((h, i) => {
            const enabled = commands.isEnabled(h.command);
            return (
              <li
                key={h.command.id}
                role="option"
                aria-selected={i === index}
                aria-disabled={!enabled}
                className={`palette-item${i === index ? ' active' : ''}${enabled ? '' : ' disabled'}`}
                onMouseEnter={() => setIndex(i)}
                onClick={() => runAt(i)}
              >
                <span className="palette-cat">{h.command.category}</span>
                <span className="palette-title">{h.command.title}</span>
                {h.command.shortcut && <kbd>{formatShortcut(h.command.shortcut)}</kbd>}
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}
