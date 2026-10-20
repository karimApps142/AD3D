// Startup: pick the engine, start the live preview, and run the spike harness when asked to
// (native launch flag, or ?spike=1 in a browser).

import { useEffect } from 'react';
import { getEngine } from '../engine';
import { startSpikeOnce } from '../spike/runSpike';
import { startPreview } from '../state/preview';
import { useAppStore } from '../state/store';

export function useEngineBoot(): void {
  useEffect(() => {
    getEngine()
      .then(async (engine) => {
        useAppStore.setState({ engine: { name: engine.name, isNative: engine.isNative } });
        startPreview(engine);
        let spike = false;
        try {
          spike = (await engine.getLaunchFlags()).spike;
        } catch (err) {
          console.warn('[boot] get_launch_flags failed', err);
        }
        if (new URLSearchParams(window.location.search).get('spike') === '1') spike = true;
        if (spike) startSpikeOnce(engine);
      })
      .catch((err: unknown) => {
        console.error('[boot] engine failed to start', err);
        useAppStore.setState({ engineError: err instanceof Error ? err.message : String(err) });
      });
  }, []);
}
