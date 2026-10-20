import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './app/App';
import './app/app.css';
import { perf } from './state/perf';
import { useAppStore } from './state/store';
import { viewportBus } from './viewport/viewportBus';

// Debug handle for the dev server and headless checks (not in production builds).
if (import.meta.env.DEV) {
  (window as unknown as Record<string, unknown>).__AD3D__ = { store: useAppStore, viewport: viewportBus, perf };
}

const root = document.getElementById('root');
if (!root) throw new Error('#root missing from index.html');

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
