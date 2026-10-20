// Preview-only settings: room light and the render light. None of these change the model.

import { useShallow } from 'zustand/react/shallow';
import { selectPreviewLight, useAppStore } from '../state/store';
import { PREVIEW_ELEVATION_MAX } from '../viewport/useLightDrag';
import { Slider } from './Slider';

export function ViewPanel() {
  const view = useAppStore((s) => s.view);
  const setView = useAppStore((s) => s.setView);
  const requestFit = useAppStore((s) => s.requestFit);
  const light = useAppStore(useShallow(selectPreviewLight));

  const setLight = (patch: { previewAzimuthDeg?: number; previewElevationDeg?: number }) =>
    setView({
      followDesignLight: false,
      previewAzimuthDeg: light.azimuthDeg,
      previewElevationDeg: light.elevationDeg,
      ...patch,
    });

  return (
    <section className="panel" aria-label="Preview settings">
      <header className="panel-head">
        <h2>Preview</h2>
      </header>
      <div className="group">
        <Slider
          param="ambient" label="Ambient light" unit="%" min={0} max={50} step={1}
          value={Math.round(view.ambient * 100)}
          hint="Room light as a share of the lamp. More of it washes the image out"
          onChange={(v) => setView({ ambient: v / 100 })}
        />
        <label className="check">
          <input
            type="checkbox"
            checked={view.followDesignLight}
            onChange={(e) => setView({ followDesignLight: e.currentTarget.checked })}
          />
          Light follows the design light
        </label>
        <Slider
          param="previewAzimuthDeg" label="Light azimuth" unit="°" min={0} max={359} step={1}
          value={Math.round(light.azimuthDeg)}
          hint="Where the preview lamp sits (Alt+drag in the view does the same)"
          onChange={(v) => setLight({ previewAzimuthDeg: v })}
        />
        <Slider
          param="previewElevationDeg" label="Light elevation" unit="°" min={0} max={PREVIEW_ELEVATION_MAX} step={0.5} digits={1}
          value={light.elevationDeg}
          hint="Raise it to see the image fade under ordinary room light"
          onChange={(v) => setLight({ previewElevationDeg: v })}
        />
        <div className="row-buttons">
          <button type="button" onClick={() => setView({ followDesignLight: true })} disabled={view.followDesignLight}>
            Reset light
          </button>
          <button type="button" onClick={requestFit}>
            Fit view
          </button>
        </div>
      </div>
    </section>
  );
}
