// Squama parameters. Every change regenerates the plate live through the preview loop.

import { gridFromBBox, snapGrid, SYNTHETIC_ASPECT, wrapDeg } from '../engine/squamaMath';
import { type DisplayMesh, useAppStore } from '../state/store';
import { Slider } from './Slider';

const LIGHT_SIDES = [
  { label: 'Left', azimuth: 180 },
  { label: 'Right', azimuth: 0 },
  { label: 'Top', azimuth: 90 },
  { label: 'Bottom', azimuth: 270 },
] as const;

function sideName(azimuth: number): string | null {
  const a = wrapDeg(azimuth);
  for (const side of LIGHT_SIDES) {
    const d = Math.abs(((a - side.azimuth + 540) % 360) - 180);
    if (d < 0.5) return side.label.toLowerCase();
  }
  return null;
}

function meshGridLabel(mesh: DisplayMesh | null): string {
  if (!mesh || mesh.kind !== 'squama' || !mesh.params) return '';
  const { bboxMin, bboxMax } = mesh.payload;
  const { nx, ny } = gridFromBBox(bboxMin, bboxMax, mesh.params.cellMm);
  const w = bboxMax[0] - bboxMin[0];
  const h = bboxMax[1] - bboxMin[1];
  const d = bboxMax[2] - bboxMin[2];
  return `${nx} × ${ny} cells · ${w.toFixed(0)} × ${h.toFixed(0)} × ${d.toFixed(1)} mm`;
}

export function ParamsPanel() {
  const params = useAppStore((s) => s.params);
  const setParam = useAppStore((s) => s.setParam);
  const meshLabel = useAppStore((s) => meshGridLabel(s.mesh));
  const busy = useAppStore((s) => s.previewBusy);
  const error = useAppStore((s) => s.previewError);
  const predicted = snapGrid(params.widthMm, params.cellMm, SYNTHETIC_ASPECT);
  const side = sideName(params.lightAzimuthDeg);

  return (
    <section className="panel" aria-label="Squama parameters">
      <header className="panel-head">
        <h2>Squama</h2>
        <span className={`status-dot${busy ? ' busy' : ''}`} title={busy ? 'Generating…' : 'Up to date'} />
      </header>

      <div className="group">
        <h3>Image</h3>
        <p className="note">Built-in test pattern (photo input arrives in M1).</p>
      </div>

      <div className="group">
        <h3>Plate</h3>
        <Slider
          param="widthMm" label="Width" unit="mm" min={20} max={800} step={1}
          value={params.widthMm}
          hint="Snapped to whole cells; the height follows the image aspect"
          onChange={(v) => setParam('widthMm', v)}
        />
        <p className="derived">
          {meshLabel || `${predicted.nx} × ${predicted.ny} cells · ${predicted.widthMm.toFixed(0)} × ${predicted.heightMm.toFixed(0)} mm`}
        </p>
        <Slider
          param="cellMm" label="Cell size" unit="mm" min={1} max={20} step={0.5} digits={1}
          value={params.cellMm}
          hint="Side of one square facet"
          onChange={(v) => setParam('cellMm', v)}
        />
        <Slider
          param="baseMm" label="Base thickness" unit="mm" min={0.6} max={10} step={0.1} digits={1}
          value={params.baseMm}
          hint="Minimum material behind any facet"
          onChange={(v) => setParam('baseMm', v)}
        />
      </div>

      <div className="group">
        <h3>Design light</h3>
        <div className="segmented" role="group" aria-label="Light side">
          {LIGHT_SIDES.map((s) => (
            <button
              key={s.label}
              type="button"
              className={side === s.label.toLowerCase() ? 'active' : ''}
              onClick={() => setParam('lightAzimuthDeg', s.azimuth)}
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
          onChange={(v) => setParam('lightAzimuthDeg', v)}
        />
        <Slider
          param="lightElevationDeg" label="Elevation" unit="°" min={0} max={20} step={0.5} digits={1}
          value={params.lightElevationDeg}
          hint="Grazing angle of the lamp above the plate (LuBan: light angle)"
          onChange={(v) => setParam('lightElevationDeg', v)}
        />
      </div>

      <div className="group">
        <h3>Facets</h3>
        <Slider
          param="maxTiltDeg" label="Max tilt" unit="°" min={1} max={20} step={0.1} digits={1}
          value={params.maxTiltDeg}
          hint="Largest facet tilt (LuBan: surface angle). More tilt, more contrast"
          onChange={(v) => setParam('maxTiltDeg', v)}
        />
      </div>

      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
