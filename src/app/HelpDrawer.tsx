// Help drawer: Shadow Plate parameter docs, keyboard shortcuts and the print checklist.

import { useMemo, useState } from 'react';
import { commands } from '../commands/registry';
import { formatShortcut } from '../commands/shortcuts';
import { type HelpTab, useUiStore } from '../state/ui';
import { SQUAMA_PARAMS, SQUAMA_PRINT_CHECKLIST, SQUAMA_SUMMARY, SQUAMA_VIEW_NOTES } from './help/squamaDocs';

const TABS: { id: HelpTab; label: string }[] = [
  { id: 'squama', label: 'Parameters' },
  { id: 'shortcuts', label: 'Shortcuts' },
  { id: 'checklist', label: 'Print checklist' },
];

export function HelpDrawer() {
  const open = useUiStore((s) => s.helpOpen);
  const tab = useUiStore((s) => s.helpTab);
  if (!open) return null;
  return (
    <aside className="help-drawer" aria-label="Help">
      <header className="help-head">
        <div className="segmented" role="tablist">
          {TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              role="tab"
              aria-selected={tab === t.id}
              className={tab === t.id ? 'active' : ''}
              onClick={() => useUiStore.setState({ helpTab: t.id })}
            >
              {t.label}
            </button>
          ))}
        </div>
        <button type="button" className="icon-btn" aria-label="Close help" onClick={() => useUiStore.setState({ helpOpen: false })}>
          ×
        </button>
      </header>
      <div className="help-body">
        {tab === 'squama' && <ParamsHelp />}
        {tab === 'shortcuts' && <ShortcutsHelp />}
        {tab === 'checklist' && <Section title={SQUAMA_PRINT_CHECKLIST.title} items={SQUAMA_PRINT_CHECKLIST.body} list />}
      </div>
    </aside>
  );
}

function Section({ title, items, list }: { title: string; items: string[]; list?: boolean }) {
  return (
    <section className="help-section">
      <h3>{title}</h3>
      {list ? (
        <ul>
          {items.map((t) => (
            <li key={t}>{t}</li>
          ))}
        </ul>
      ) : (
        items.map((t) => <p key={t}>{t}</p>)
      )}
    </section>
  );
}

function ParamsHelp() {
  const [filter, setFilter] = useState('');
  const shown = useMemo(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return SQUAMA_PARAMS;
    return SQUAMA_PARAMS.filter((p) => `${p.label} ${p.key} ${p.meaning} ${(p.tips ?? []).join(' ')}`.toLowerCase().includes(q));
  }, [filter]);
  return (
    <>
      <Section title={SQUAMA_SUMMARY.title} items={SQUAMA_SUMMARY.body} />
      <input
        className="help-filter"
        type="search"
        placeholder="Filter parameters…"
        aria-label="Filter parameters"
        value={filter}
        onChange={(e) => setFilter(e.currentTarget.value)}
      />
      {shown.map((p) => (
        <section className="help-param" key={p.key}>
          <h4>
            {p.label}
            {p.unit && <span className="unit"> ({p.unit})</span>}
            {p.planned && <span className="badge">planned</span>}
          </h4>
          <p>{p.meaning}</p>
          <dl>
            <dt>Default</dt>
            <dd>{p.default}</dd>
            <dt>Range</dt>
            <dd>{p.range}</dd>
          </dl>
          {p.tips && (
            <ul>
              {p.tips.map((t) => (
                <li key={t}>{t}</li>
              ))}
            </ul>
          )}
        </section>
      ))}
      <Section title={SQUAMA_VIEW_NOTES.title} items={SQUAMA_VIEW_NOTES.body} />
    </>
  );
}

function ShortcutsHelp() {
  const rows = commands
    .list()
    .filter((c) => c.shortcut && !c.hidden)
    .concat(commands.list().filter((c) => c.id === 'view.commandPalette'))
    .sort((a, b) => a.category.localeCompare(b.category) || a.title.localeCompare(b.title));
  return (
    <section className="help-section">
      <h3>Keyboard shortcuts</h3>
      <table className="shortcut-table">
        <tbody>
          {rows.map((c) => (
            <tr key={c.id}>
              <td className="muted">{c.category}</td>
              <td>{c.title}</td>
              <td>
                <kbd>{formatShortcut(c.shortcut!)}</kbd>
              </td>
            </tr>
          ))}
          <tr>
            <td className="muted">View</td>
            <td>Swing the preview light</td>
            <td>
              <kbd>⌥ drag</kbd>
            </td>
          </tr>
        </tbody>
      </table>
    </section>
  );
}
