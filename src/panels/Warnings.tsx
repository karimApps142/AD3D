// Inline list of core and predicted warnings.

import type { CoreWarning } from '../engine/types';

export function Warnings({ items, label = 'Warnings' }: { items: readonly CoreWarning[]; label?: string }) {
  if (!items.length) return null;
  return (
    <ul className="warnings" aria-label={label}>
      {items.map((w) => (
        <li key={w.code} data-code={w.code}>
          {w.message}
        </li>
      ))}
    </ul>
  );
}
