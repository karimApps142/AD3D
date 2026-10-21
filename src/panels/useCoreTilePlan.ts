// The core's tile plan (squama_tile_plan → squama::plan_tiles, §14.1) for the export panel and the
// viewport overlay. Debounced and latest-wins; null while unavailable (browser mock, errors), so
// callers fall back to the TypeScript planner.

import { useEffect, useRef, useState } from 'react';
import { getEngine } from '../engine';
import type { SquamaParams, TilePlan } from '../engine/types';
import { tileLabel, type TilePlanOverlay } from '../viewport/tiles';
import { useViewportStore } from '../viewport/viewportStore';
import { useAppStore } from '../state/store';
import { buildExportDto } from './squamaDto';
import type { ExportSettings } from './squamaDto';

const DEBOUNCE_MS = 150;

/** Overlay rectangles of a core plan (rows from the plate bottom, like the core's tile ids). */
export function overlayFromPlan(plan: TilePlan): TilePlanOverlay {
  const tiles: TilePlanOverlay['tiles'] = [];
  for (let row = 0; row + 1 < plan.vEdgesMm.length; row++) {
    for (let col = 0; col + 1 < plan.uEdgesMm.length; col++) {
      tiles.push({
        id: [col, row],
        rectMm: [plan.uEdgesMm[col], plan.vEdgesMm[row], plan.uEdgesMm[col + 1], plan.vEdgesMm[row + 1]],
        label: tileLabel(col, row),
      });
    }
  }
  const span = (e: number[]) => e.slice(1).reduce((m, v, i) => Math.max(m, v - e[i]), 0);
  return { tiles, source: 'core', maxTileMm: [span(plan.uEdgesMm), span(plan.vEdgesMm)] };
}

export function useCoreTilePlan(params: SquamaParams, settings: ExportSettings): TilePlan | null {
  const [plan, setPlan] = useState<TilePlan | null>(null);
  // Quality is preview-loop state: a drag must not re-plan (the core plan runs a final solve).
  const { quality, ...doc } = params;
  const key = JSON.stringify([doc, settings]);
  // Plan only once the plate has settled: not while a slider drags (quality 'preview') and not
  // while the final plate is being generated, whose heights the core plan then reuses.
  const busy = useAppStore((s) => s.previewBusy);
  const settled = quality !== 'preview' && !busy;
  const planned = useRef<string | null>(null);
  useEffect(() => {
    if (!settled || planned.current === key) return;
    let live = true;
    const t = setTimeout(async () => {
      const vp = useViewportStore.getState();
      try {
        const engine = await getEngine();
        if (!engine.squamaTilePlan) {
          if (live) {
            setPlan(null);
            vp.set({ tilePlanState: 'estimate', tilePlanError: null });
          }
          return;
        }
        if (!settings.tile) {
          if (live) {
            planned.current = key;
            setPlan(null);
            vp.set({ tilePlanState: 'off', tilePlanError: null });
          }
          return;
        }
        const p = await engine.squamaTilePlan(buildExportDto(useAppStore.getState().params, settings, '/plan/plate.3mf'));
        if (live) {
          planned.current = key;
          setPlan(p);
          vp.set({ tilePlanState: 'core', tilePlanError: null });
        }
      } catch (err) {
        if (live) {
          setPlan(null);
          const msg = err instanceof Error ? err.message : String(err);
          // A superseded plan is not an error to show.
          if (!/cancel/i.test(msg)) vp.set({ tilePlanState: 'error', tilePlanError: msg });
        }
      }
    }, DEBOUNCE_MS);
    return () => {
      live = false;
      clearTimeout(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, settled]);
  useEffect(() => {
    useViewportStore.getState().set({ tilePlan: plan ? overlayFromPlan(plan) : null });
  }, [plan]);
  return plan;
}
