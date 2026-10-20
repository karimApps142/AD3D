// Squama parameters: every LuBan control (width/height with snapping, cell size, light side,
// light angle, surface angle, thickness) plus ours (free azimuth, square/hex layout, point lamp,
// optimiser, response model, max relief, preview ambient). Dragging regenerates at preview quality;
// releasing regenerates at final quality (§13). Range inputs keep their spike data-param names.

import { useEffect, useMemo, useState } from 'react';
import { useShallow } from 'zustand/react/shallow';
import { ledgeWarning, mergeWarnings, predictWarnings, reliefMm } from '../engine/squamaChecks';
import { useSquamaInfo } from '../engine/squamaInfo';
import { wrapDeg } from '../engine/squamaMath';
import type { LayoutSpec } from '../engine/types';
import { useAppStore } from '../state/store';
import { plateLayout } from './plateLayout';
import { lampElevationDeg, lampPosition, parseMeasuredTable } from './squamaDto';
import { useSquamaUi } from './squamaUi';
import { lightAlongPrintZ, quarterTurns } from './tilePlan';
import { Slider } from './Slider';
import { useParamSetter } from './useParamSetter';
import { Warnings } from './Warnings';
import './panels.css';

export const LIGHT_SIDES = [
  { label: 'Left', azimuth: 180 },
  { label: 'Right', azimuth: 0 },
  { label: 'Top', azimuth: 90 },
  { label: 'Bottom', azimuth: 270 },
] as const;

export function sideName(azimuth: number): string | null {
  const a = wrapDeg(azimuth);
  for (const side of LIGHT_SIDES) {
    const d = Math.abs(((a - side.azimuth + 540) % 360) - 180);
    if (d < 0.5) return side.label.toLowerCase();
  }
  return null;
}

const LAYOUTS: ReadonlyArray<{ label: string; spec: LayoutSpec }> = [
  { label: 'Square', spec: { kind: 'square' } },
  { label: 'Hex ⬡', spec: { kind: 'hex', orientation: 'pointy' } },
  { label: 'Hex ⬢', spec: { kind: 'hex', orientation: 'flat' } },
];

function sameLayout(a: LayoutSpec | undefined, b: LayoutSpec): boolean {
  const x = a ?? { kind: 'square' };
  return x.kind === b.kind && (x.kind === 'square' || (b.kind === 'hex' && x.orientation === b.orientation));
}

const DEFAULT_TABLE = '# tilt°, brightness 0–1 (from the calibration tile)\n-10, 0.00\n0, 0.35\n10, 1.00';

export function ParamsPanel() {
  const params = useAppStore((s) => s.params);
  const ambient = useAppStore((s) => s.view.ambient);
  const setView = useAppStore((s) => s.setView);
  const busy = useAppStore((s) => s.previewBusy);
  const error = useAppStore((s) => s.previewError);
  const info = useSquamaInfo((s) => s.info);
  const { image, lamp, orientation } = useSquamaUi(useShallow((s) => ({ image: s.image, lamp: s.lamp, orientation: s.export.orientation })));
  const setUi = useSquamaUi((s) => s.set);
  const { set, patch, commit } = useParamSetter();
  const [table, setTable] = useState(DEFAULT_TABLE);

  const layout = plateLayout(params, image, info);
  const side = sideName(params.lightAzimuthDeg);
  const pointLight = params.light?.kind === 'point';
  const relief = reliefMm(layout.cellMm, params.lightAzimuthDeg, params.maxTiltDeg);
  const heightAuto = params.heightMm === undefined;

  // Keep the lamp position in step with the plate size and azimuth.
  useEffect(() => {
    if (!pointLight) return;
    const posMm = lampPosition(layout.wMm, layout.hMm, params.lightAzimuthDeg, lamp.distanceMm, lamp.heightMm);
    const cur = useAppStore.getState().params.light?.posMm;
    if (!cur || cur.some((v, k) => v !== posMm[k])) patch({ light: { kind: 'point', posMm } });
  }, [pointLight, layout.wMm, layout.hMm, params.lightAzimuthDeg, lamp.distanceMm, lamp.heightMm, patch]);

  const warnings = useMemo(() => {
    const k = quarterTurns(orientation, params.lightAzimuthDeg);
    const ledge = ledgeWarning(layout.cellMm, params.maxTiltDeg, k !== null, lightAlongPrintZ(params.lightAzimuthDeg, k));
    const fresh = info && info.layout.nx === layout.nx ? info.warnings : [];
    return mergeWarnings(fresh, predictWarnings(params), ledge ? [ledge] : []);
  }, [info, params, layout.cellMm, layout.nx, orientation]);

  const measured = params.response?.kind === 'measured';
  const parsed = measured ? parseMeasuredTable(table) : null;

  return (
    <section className="panel" aria-label="Squama parameters">
      <header className="panel-head">
        <h2>Squama</h2>
        <span className={`status-dot${busy ? ' busy' : ''}`} title={busy ? 'Generating…' : 'Up to date'} />
      </header>

      <div className="group">
        <h3>Plate</h3>
        <Slider
          param="widthMm" label="Width" unit="mm" min={20} max={800} step={1}
          value={params.widthMm}
          extra={`→ ${layout.wMm.toFixed(layout.wMm % 1 ? 1 : 0)}`}
          hint="Snapped to whole cells (LuBan: 200 at cell 6 gives 198)"
          onChange={(v, ph) => set('widthMm', v, ph)}
          onCommit={commit}
        />
        <label className="check">
          <input
            type="checkbox"
            checked={heightAuto}
            onChange={(e) => patch({ heightMm: e.currentTarget.checked ? undefined : Math.round(layout.hMm) })}
          />
          Height from the image crop
        </label>
        <Slider
          param="heightMm" label="Height" unit="mm" min={20} max={800} step={1}
          value={heightAuto ? Math.round(layout.hMm) : (params.heightMm ?? layout.hMm)}
          extra={`→ ${layout.hMm.toFixed(layout.hMm % 1 ? 1 : 0)}`}
          disabled={heightAuto}
          hint="Untick 'from the image crop' to set it; the crop then keeps the plate's aspect"
          onChange={(v, ph) => set('heightMm', v, ph)}
          onCommit={commit}
        />
        <p className="derived" data-testid="plate-size">
          {layout.nx} × {layout.ny} cells · {layout.wMm.toFixed(0)} × {layout.hMm.toFixed(0)} mm · relief ≈ {relief.toFixed(2)} mm
        </p>
        <Slider
          param="cellMm" label="Cell size" unit="mm" min={1} max={20} step={0.5} digits={1}
          value={params.cellMm}
          hint="Side of one square facet (hex: width across flats)"
          onChange={(v, ph) => set('cellMm', v, ph)}
          onCommit={commit}
        />
        <Slider
          param="baseMm" label="Thickness" unit="mm" min={0.4} max={10} step={0.1} digits={1}
          value={params.baseMm}
          hint="Minimum material behind any facet (LuBan: thickness)"
          onChange={(v, ph) => set('baseMm', v, ph)}
          onCommit={commit}
        />
        <div className="segmented" role="group" aria-label="Cell layout">
          {LAYOUTS.map((l) => (
            <button
              key={l.label}
              type="button"
              className={sameLayout(params.layout, l.spec) ? 'active' : ''}
              onClick={() => patch({ layout: l.spec.kind === 'square' ? undefined : l.spec })}
            >
              {l.label}
            </button>
          ))}
        </div>
      </div>

      <div className="group">
        <h3>Design light</h3>
        <div className="segmented" role="group" aria-label="Light side">
          {LIGHT_SIDES.map((s) => (
            <button
              key={s.label}
              type="button"
              className={side === s.label.toLowerCase() ? 'active' : ''}
              onClick={() => set('lightAzimuthDeg', s.azimuth)}
            >
              {s.label}
            </button>
          ))}
        </div>
        <Slider
          param="lightAzimuthDeg" label={side ? `Azimuth (from ${side})` : 'Azimuth'} unit="°"
          min={0} max={359} step={1}
          value={params.lightAzimuthDeg}
          hint="Direction the light comes from: 0 right, 90 top, 180 left, 270 bottom"
          onChange={(v, ph) => set('lightAzimuthDeg', v, ph)}
          onCommit={commit}
        />
        <Slider
          param="lightElevationDeg" label="Light angle" unit="°" min={0} max={45} step={0.5} digits={1}
          value={params.lightElevationDeg}
          hint="Grazing angle of the lamp above the plate (LuBan: light angle, 0–20; 0 means 1°)"
          onChange={(v, ph) => set('lightElevationDeg', v, ph)}
          onCommit={commit}
        />
        <div className="segmented" role="group" aria-label="Light model">
          <button type="button" className={pointLight ? '' : 'active'} onClick={() => patch({ light: undefined })}>
            Directional
          </button>
          <button
            type="button"
            className={pointLight ? 'active' : ''}
            onClick={() =>
              patch({ light: { kind: 'point', posMm: lampPosition(layout.wMm, layout.hMm, params.lightAzimuthDeg, lamp.distanceMm, lamp.heightMm) } })
            }
          >
            Point lamp
          </button>
        </div>
        {pointLight && (
          <>
            <Slider
              param="lampDistanceMm" label="Lamp distance" unit="mm" min={100} max={3000} step={10}
              value={lamp.distanceMm}
              hint="From the plate centre, along the azimuth"
              onChange={(v) => setUi({ lamp: { ...lamp, distanceMm: v } })}
            />
            <Slider
              param="lampHeightMm" label="Lamp height" unit="mm" min={5} max={1000} step={5}
              value={lamp.heightMm}
              hint="Out of the plate's front face"
              onChange={(v) => setUi({ lamp: { ...lamp, heightMm: v } })}
            />
            <p className="derived">
              Lamp at {params.light?.posMm.map((v) => v.toFixed(0)).join(', ')} mm · {lampElevationDeg(lamp.distanceMm, lamp.heightMm).toFixed(1)}° at the centre
            </p>
          </>
        )}
      </div>

      <div className="group">
        <h3>Facets</h3>
        <Slider
          param="maxTiltDeg" label="Surface angle" unit="°" min={1} max={20} step={0.1} digits={1}
          value={params.maxTiltDeg}
          hint="Largest facet tilt (LuBan: surface angle). More tilt, more contrast"
          onChange={(v, ph) => set('maxTiltDeg', v, ph)}
          onCommit={commit}
        />
        <label className="check">
          <input
            type="checkbox"
            checked={params.optimiser?.kind === 'shadow'}
            onChange={(e) => patch({ optimiser: e.currentTarget.checked ? { kind: 'shadow', maxSweeps: 8 } : undefined })}
          />
          Shadow-aware optimiser (final quality only)
        </label>
        {params.optimiser?.kind === 'shadow' && (
          <>
            <Slider
              param="maxSweeps" label="Optimiser sweeps" min={1} max={32} step={1}
              value={params.optimiser.maxSweeps ?? 8}
              onChange={(v) => patch({ optimiser: { kind: 'shadow', maxSweeps: v } })}
            />
            <Slider
              param="maxReliefMm" label="Max relief" unit="mm" min={0.2} max={5} step={0.05} digits={2}
              value={params.maxReliefMm ?? Math.max(0.2, Math.round(relief * 20) / 20)}
              hint="Height budget above the thickness for the optimiser"
              onChange={(v, ph) => set('maxReliefMm', v, ph)}
              onCommit={commit}
            />
          </>
        )}
        <div className="segmented" role="group" aria-label="Response model">
          <button type="button" className={measured ? '' : 'active'} onClick={() => patch({ response: undefined })}>
            Lambert
          </button>
          <button
            type="button"
            className={measured ? 'active' : ''}
            onClick={() => {
              const t = parseMeasuredTable(table);
              if (!('error' in t)) patch({ response: { kind: 'measured', ...t } });
            }}
          >
            Measured
          </button>
        </div>
        {measured && (
          <>
            <textarea
              className="table-input"
              aria-label="Measured response table"
              rows={5}
              value={table}
              onChange={(e) => {
                const text = e.currentTarget.value;
                setTable(text);
                const t = parseMeasuredTable(text);
                if (!('error' in t)) patch({ response: { kind: 'measured', ...t } });
              }}
            />
            {parsed && 'error' in parsed && <p className="error">{parsed.error}</p>}
          </>
        )}
        <Slider
          param="ambient" label="Ambient (preview)" unit="%" min={0} max={50} step={1}
          value={Math.round(ambient * 100)}
          hint="Room light as a share of the lamp; changes only the preview and the appearance map"
          onChange={(v) => setView({ ambient: v / 100 })}
        />
      </div>

      <Warnings items={warnings} />
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
