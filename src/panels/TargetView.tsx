// 2D views next to the 3D preview: the cell-averaged target (what the generator aims for) and the
// predicted appearance under the design light with the preview ambient. The appearance comes from
// the core's forward model when it sends one (§15.1), else from a Lambert estimate without cast
// shadows, which is labelled as such.

import { useEffect, useMemo, useRef } from 'react';
import { simulateLambert } from '../engine/squamaChecks';
import type { TargetGrid } from '../engine/types';
import { gridToRgba, paint } from './gridImage';

export interface TargetViewProps {
  target: TargetGrid;
  elevationDeg: number;
  maxTiltDeg: number;
  ambient: number;
}

export function TargetView({ target, elevationDeg, maxTiltDeg, ambient }: TargetViewProps) {
  const tCanvas = useRef<HTMLCanvasElement>(null);
  const aCanvas = useRef<HTMLCanvasElement>(null);
  const sim = useMemo(() => simulateLambert(target.g, elevationDeg, maxTiltDeg, ambient), [target, elevationDeg, maxTiltDeg, ambient]);

  useEffect(() => {
    paint(tCanvas.current, gridToRgba(target.g, target.nx, target.ny), target.nx, target.ny);
  }, [target]);
  useEffect(() => {
    // Appearance values are linear radiance relative to the brightest facet.
    const values = target.sim ?? sim.values;
    paint(aCanvas.current, gridToRgba(values, target.nx, target.ny), target.nx, target.ny);
  }, [target, sim]);

  const aspect = `${target.nx} / ${target.ny}`;
  const contrast = Number.isFinite(sim.contrast) ? `${sim.contrast.toFixed(1)} : 1` : '∞ : 1';
  return (
    <div className="target-views">
      <figure>
        <canvas ref={tCanvas} className="grid-canvas" style={{ aspectRatio: aspect }} />
        <figcaption>Target ({target.nx} × {target.ny})</figcaption>
      </figure>
      <figure>
        <canvas ref={aCanvas} className="grid-canvas" style={{ aspectRatio: aspect }} />
        <figcaption title={target.sim ? 'Core forward model with cast shadows' : 'Lambert estimate, no cast shadows'}>
          Appearance · {contrast}
          {!target.sim && ' · est.'}
        </figcaption>
      </figure>
    </div>
  );
}
