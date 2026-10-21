// Display controls drawn over the 3D view (top left): render mode, light model, lamp distance,
// "lock preview light to design light", ambient room light, front viewer, tile overlay and
// "Save preview image". None of these change the model.

import { useState } from 'react';
import { useAppStore } from '../state/store';
import { designPointLight } from './previewLight';
import { savePreviewImage } from './savePreview';
import { useTilePlan } from './TileOverlay';
import './viewport.css';
import { LAMP_DISTANCE_RANGE, type PreviewLightKind, type RenderMode, useViewportStore } from './viewportStore';

const MODES: { id: RenderMode; label: string; title: string }[] = [
  { id: 'analytic', label: 'Shadows', title: 'Lambert shading with exact cast shadows ray-marched over the cell planes' },
  { id: 'shadowMap', label: 'Shadow map', title: 'Fallback: three.js shadow-mapped light' },
  { id: 'relief', label: 'Relief', title: 'Facets coloured by height; walls, back, keys and stand by kind' },
];

const LIGHTS: { id: PreviewLightKind; label: string; title: string }[] = [
  { id: 'directional', label: 'Sun', title: 'Directional light (parallel rays, as designed)' },
  { id: 'point', label: 'Lamp', title: 'Point lamp at a real distance (Alt+wheel changes it)' },
];

function Segmented<T extends string>({ value, items, onChange, name }: { value: T; items: { id: T; label: string; title: string }[]; onChange: (v: T) => void; name: string }) {
  return (
    <div className="vp-seg" role="radiogroup" aria-label={name}>
      {items.map((it) => (
        <button
          key={it.id}
          type="button"
          role="radio"
          aria-checked={value === it.id}
          className={value === it.id ? 'on' : undefined}
          title={it.title}
          data-vp={`${name}-${it.id}`}
          onClick={() => onChange(it.id)}
        >
          {it.label}
        </button>
      ))}
    </div>
  );
}

export function ViewportToolbar() {
  const vp = useViewportStore();
  const follow = useAppStore((s) => s.view.followDesignLight);
  const ambient = useAppStore((s) => s.view.ambient);
  const setView = useAppStore((s) => s.setView);
  const designLamp = useAppStore((s) => designPointLight(s.params) !== null);
  const plan = useTilePlan();
  const planState = useViewportStore((s) => s.tilePlanState);
  const planError = useViewportStore((s) => s.tilePlanError);
  const tilesTitle = plan
    ? `Tile layout (${plan.source === 'core' ? 'from the core' : 'estimated for a CR-6 SE bed'})`
    : planState === 'off'
      ? 'Exported in one piece: turn on "Split to the bed" in Tiling & export to see tiles'
      : planState === 'error'
        ? `No tile plan: ${planError ?? 'the core could not plan the tiles'}`
        : 'No plate';
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const lampActive = (follow && designLamp) || vp.lightKind === 'point';

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      await savePreviewImage();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="vp-toolbar" onPointerDown={(e) => e.stopPropagation()}>
      <Segmented name="render" value={vp.renderMode} items={MODES} onChange={(renderMode) => vp.set({ renderMode })} />
      <Segmented
        name="light"
        value={follow && designLamp ? 'point' : vp.lightKind}
        items={LIGHTS}
        onChange={(lightKind) => vp.set({ lightKind })}
      />
      {lampActive && !(follow && designLamp) && (
        <label className="vp-range" title="Lamp distance from the plate centre (Alt+wheel in the view)">
          <span>Lamp</span>
          <input
            type="range"
            data-vp="lamp-distance"
            min={Math.log(LAMP_DISTANCE_RANGE[0])}
            max={Math.log(LAMP_DISTANCE_RANGE[1])}
            step={0.01}
            value={Math.log(vp.lampDistanceMm)}
            onChange={(e) => vp.set({ lampDistanceMm: Math.round(Math.exp(Number(e.currentTarget.value))) })}
          />
          <output>{vp.lampDistanceMm} mm</output>
        </label>
      )}
      <label className="vp-range" title="Room light as a share of the lamp. More of it washes the image out">
        <span>Ambient</span>
        <input
          type="range"
          data-vp="ambient"
          min={0}
          max={50}
          step={1}
          value={Math.round(ambient * 100)}
          onChange={(e) => setView({ ambient: Number(e.currentTarget.value) / 100 })}
        />
        <output>{Math.round(ambient * 100)}%</output>
      </label>
      <label className="vp-check" title="Preview light sits where the design light is; Alt+drag frees it">
        <input type="checkbox" data-vp="lock-light" checked={follow} onChange={(e) => setView({ followDesignLight: e.currentTarget.checked })} />
        Lock to design light
      </label>
      <div className="vp-row">
        <button type="button" data-vp="front" className={vp.frontView ? 'on' : undefined} aria-pressed={vp.frontView} title="Look straight at the plate, as it hangs on a wall (orthographic)" onClick={() => vp.set({ frontView: !vp.frontView })}>
          Front view
        </button>
        <button type="button" data-vp="tiles" className={vp.showTiles ? 'on' : undefined} aria-pressed={vp.showTiles} disabled={!plan} title={tilesTitle} onClick={() => vp.set({ showTiles: !vp.showTiles })}>
          Tiles{plan && plan.tiles.length > 1 ? ` (${plan.tiles.length})` : ''}
        </button>
        <button type="button" data-vp="save-image" disabled={saving} title="Save the view as a PNG" onClick={save}>
          {saving ? 'Saving…' : 'Save image'}
        </button>
      </div>
      {(error || vp.lastSaved) && <div className={`vp-status${error ? ' err' : ''}`}>{error ?? `Saved ${vp.lastSaved}`}</div>}
    </div>
  );
}
