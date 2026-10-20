// The core's tile plan (squama_tile_plan → squama::plan_tiles, §14.1) for the export panel and the
// viewport overlay. Debounced and latest-wins; null while unavailable (browser mock, errors), so
// callers fall back to the TypeScript planner.

import { useEffect, useState } from 'react';
import { getEngine } from '../engine';
import type { SquamaParams, TilePlan } from '../engine/types';
import { tileLabel, type TilePlanOverlay } from '../viewport/tiles';
import { useViewportStore } from '../viewport/viewportStore';
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
  const key = JSON.stringify([params, settings]);
  useEffect(() => {
    let live = true;
    const t = setTimeout(async () => {
      try {
        const engine = await getEngine();
        if (!engine.squamaTilePlan || !settings.tile) {
          if (live) setPlan(null);
          return;
        }
        const p = await engine.squamaTilePlan(buildExportDto(params, settings, '/plan/plate.3mf'));
        if (live) setPlan(p);
      } catch {
        if (live) setPlan(null);
      }
    }, DEBOUNCE_MS);
    return () => {
      live = false;
      clearTimeout(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  useEffect(() => {
    useViewportStore.getState().set({ tilePlan: plan ? overlayFromPlan(plan) : null });
  }, [plan]);
  return plan;
}
