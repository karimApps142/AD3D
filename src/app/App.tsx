// Layout: parameters on the left, the 3D view filling the rest.

import { useEffect } from 'react';
import { ParamsPanel } from '../panels/ParamsPanel';
import { ViewPanel } from '../panels/ViewPanel';
import { useAppStore } from '../state/store';
import { Viewport } from '../viewport/Viewport';
import { SpikeBanner } from './SpikeBanner';
import { useEngineBoot } from './useEngineBoot';

function useShortcuts(): void {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') && (target as HTMLInputElement).type !== 'range') return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === 'f' || e.key === 'F') useAppStore.getState().requestFit();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
}

function EngineBadge() {
  const engine = useAppStore((s) => s.engine);
  if (!engine) return <span className="badge">starting…</span>;
  return <span className={`badge${engine.isNative ? ' native' : ''}`}>{engine.isNative ? 'Rust core' : 'browser mock'}</span>;
}

export function App() {
  useEngineBoot();
  useShortcuts();
  return (
    <div className="app">
      <aside className="sidebar">
        <header className="brand">
          <h1>AD3D</h1>
          <EngineBadge />
        </header>
        <ParamsPanel />
        <ViewPanel />
      </aside>
      <main className="main">
        <Viewport />
        <SpikeBanner />
      </main>
    </div>
  );
}
