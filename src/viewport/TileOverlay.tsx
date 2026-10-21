// Tile layout overlay: dashed tile outlines just above the plate's front face and a label per
// part (tile) at its centre. Uses the core's plan when present, else the fallback planner.

import { Html } from '@react-three/drei';
import { useEffect, useMemo } from 'react';
import * as THREE from 'three';
import { useAppStore } from '../state/store';
import { layoutFromDisplay } from './plateLayout';
import { planTilesFallback, type TilePlanOverlay } from './tiles';
import { useViewportStore } from './viewportStore';

const LINE_COLOR = '#4fd1ff';

export function useTilePlan(): TilePlanOverlay | null {
  const mesh = useAppStore((s) => s.mesh);
  const fromCore = useViewportStore((s) => s.tilePlan);
  const state = useViewportStore((s) => s.tilePlanState);
  return useMemo(() => {
    if (fromCore) return fromCore;
    // The CR-6 estimate only stands in for a missing core planner (browser mock): never for an
    // export that is one piece, nor for a plan the core refused.
    if (state !== 'estimate') return null;
    const layout = layoutFromDisplay(mesh);
    return layout ? planTilesFallback(layout) : null;
  }, [mesh, fromCore, state]);
}

export function TileOverlay() {
  const show = useViewportStore((s) => s.showTiles);
  const plan = useTilePlan();
  const zTop = useAppStore((s) => (s.mesh ? s.mesh.payload.bboxMax[2] : 0));

  const lines = useMemo(() => {
    if (!plan) return null;
    const z = zTop + 0.05;
    const pts: number[] = [];
    for (const t of plan.tiles) {
      const [u0, v0, u1, v1] = t.rectMm;
      pts.push(u0, v0, z, u1, v0, z, u1, v0, z, u1, v1, z, u1, v1, z, u0, v1, z, u0, v1, z, u0, v0, z);
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(pts, 3));
    const m = new THREE.LineDashedMaterial({ color: LINE_COLOR, dashSize: 4, gapSize: 2.5, toneMapped: false, transparent: true, opacity: 0.95 });
    const seg = new THREE.LineSegments(g, m);
    seg.computeLineDistances();
    seg.name = 'tile-outlines';
    seg.renderOrder = 2;
    seg.raycast = () => {};
    return seg;
  }, [plan, zTop]);

  useEffect(
    () => () => {
      if (!lines) return;
      lines.geometry.dispose();
      (lines.material as THREE.Material).dispose();
    },
    [lines],
  );

  if (!show || !plan || !lines) return null;
  return (
    <group name="tile-overlay">
      <primitive object={lines} />
      {plan.tiles.map((t) => {
        const [u0, v0, u1, v1] = t.rectMm;
        return (
          <Html key={t.label} position={[(u0 + u1) / 2, (v0 + v1) / 2, zTop + 0.1]} center zIndexRange={[20, 10]}>
            <div className="tile-label">
              <strong>{t.label}</strong>
              <span>
                {(u1 - u0).toFixed(0)} × {(v1 - v0).toFixed(0)} mm
              </span>
            </div>
          </Html>
        );
      })}
    </group>
  );
}
