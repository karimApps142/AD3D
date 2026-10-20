// Layout: parameters on the left, the 3D view filling the rest; shell overlays (palette, help,
// settings, prompts) on top.

import { ExportPanel } from '../panels/ExportPanel';
import { ImagePanel } from '../panels/ImagePanel';
import { ParamsPanel } from '../panels/ParamsPanel';
import { ViewPanel } from '../panels/ViewPanel';
import { useAppStore } from '../state/store';
import { Viewport } from '../viewport/Viewport';
import { DocumentTitle } from './DocumentTitle';
import { ShellOverlays } from './ShellOverlays';
import { SpikeBanner } from './SpikeBanner';
import { useAppShell } from './useAppShell';
import { useEngineBoot } from './useEngineBoot';

function EngineBadge() {
  const engine = useAppStore((s) => s.engine);
  if (!engine) return <span className="badge">starting…</span>;
  return <span className={`badge${engine.isNative ? ' native' : ''}`}>{engine.isNative ? 'Rust core' : 'browser mock'}</span>;
}

export function App() {
  useEngineBoot();
  useAppShell();
  return (
    <div className="app">
      <aside className="sidebar">
        <header className="brand">
          <h1>AD3D</h1>
          <EngineBadge />
        </header>
        <DocumentTitle />
        <ParamsPanel />
        <ImagePanel />
        <ViewPanel />
        <ExportPanel />
      </aside>
      <main className="main">
        <Viewport />
        <SpikeBanner />
      </main>
      <ShellOverlays />
    </div>
  );
}
