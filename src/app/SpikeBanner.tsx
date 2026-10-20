// Shows spike progress over the viewport, then the one-line result.

import { useAppStore } from '../state/store';

export function SpikeBanner() {
  const spike = useAppStore((s) => s.spike);
  const engineError = useAppStore((s) => s.engineError);
  if (engineError) {
    return (
      <div className="banner error" role="alert">
        Engine failed to start: {engineError}
      </div>
    );
  }
  if (!spike) return null;
  return (
    <div className={`banner${spike.running ? ' running' : ''}`} role="status">
      <strong>Spike</strong> {spike.running ? spike.phase : `${spike.phase}: ${spike.summary ?? ''}`}
    </div>
  );
}
