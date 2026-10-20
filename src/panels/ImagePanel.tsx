// Image panel: open a photo (native dialog), crop it on the thumbnail (snapped to whole cells),
// rotate / mirror / invert, tone controls (gamma, contrast, brightness, auto-levels), and the 2D
// target and appearance views.

import { useEffect, useRef } from 'react';
import { useShallow } from 'zustand/react/shallow';
import { useImage } from '../app/actions';
import { getEngine } from '../engine';
import type { ImageOpts, SquamaParams } from '../engine/types';
import { useSquamaInfo } from '../engine/squamaInfo';
import { useAppStore } from '../state/store';
import { type Crop, isFullCrop, rotateCrop, snapCrop } from './cropMath';
import { CropBox } from './CropBox';
import { imageSizePx, predictPlate } from './plateLayout';
import { AUTO_LEVELS } from './squamaDto';
import { useSquamaUi } from './squamaUi';
import { Slider } from './Slider';
import { TargetView } from './TargetView';
import { useParamSetter } from './useParamSetter';
import './panels.css';

const THUMB_PX = 320;

function fileName(path: string): string {
  return path === 'synthetic' ? 'Built-in test pattern' : (path.split(/[\\/]/).pop() ?? path);
}

/** Fields that change the target grid (and nothing else). */
function targetKey(p: SquamaParams): string {
  return JSON.stringify([p.image, p.imageOpts ?? null, p.widthMm, p.heightMm ?? null, p.cellMm, p.layout ?? null]);
}

/** Loads the thumbnail and the target grid when the inputs change (latest request wins). */
function useImageData(): void {
  const params = useAppStore((s) => s.params);
  const rotate = params.imageOpts?.rotate ?? 0;
  const mirror = !!params.imageOpts?.mirror;
  const set = useSquamaUi((s) => s.set);
  const thumbSeq = useRef(0);
  const targetSeq = useRef(0);

  useEffect(() => {
    const seq = ++thumbSeq.current;
    getEngine()
      .then((e) => e.imageThumbnail(params.image, THUMB_PX, rotate, mirror))
      .then((thumbnail) => {
        if (seq === thumbSeq.current) set({ thumbnail, imageError: null });
      })
      .catch((err: unknown) => {
        if (seq === thumbSeq.current) set({ thumbnail: null, imageError: String(err instanceof Error ? err.message : err) });
      });
  }, [params.image, rotate, mirror, set]);

  // A finished final plate brings the core's simulated appearance (§15.1): fetch again then.
  const finalJob = useSquamaInfo((s) => (s.info?.quality === 'final' ? s.info.jobId : null));
  const key = `${targetKey(params)}|${finalJob}`;
  useEffect(() => {
    const seq = ++targetSeq.current;
    // A short delay coalesces slider drags; the target is a side view, not the drag-critical path.
    const timer = setTimeout(() => {
      getEngine()
        .then((e) => e.squamaTarget({ ...useAppStore.getState().params, jobId: 0 }))
        .then((target) => {
          if (seq === targetSeq.current) set({ target, targetError: null });
        })
        .catch((err: unknown) => {
          if (seq === targetSeq.current) set({ targetError: String(err instanceof Error ? err.message : err) });
        });
    }, 80);
    return () => clearTimeout(timer);
  }, [key, set]);
}

export function ImagePanel() {
  useImageData();
  const params = useAppStore((s) => s.params);
  const view = useAppStore(useShallow((s) => ({ ambient: s.view.ambient })));
  const engine = useAppStore((s) => s.engine);
  const ui = useSquamaUi(useShallow((s) => ({ image: s.image, thumbnail: s.thumbnail, imageError: s.imageError, target: s.target, targetError: s.targetError })));
  const setUi = useSquamaUi((s) => s.set);
  const imageLoading = useSquamaUi((s) => s.imageLoading);
  const { image, commit } = useParamSetter();
  const o: ImageOpts = params.imageOpts ?? {};
  const rotate = o.rotate ?? 0;
  const plate = predictPlate(params, ui.image);

  const openPhoto = async () => {
    try {
      const e = await getEngine();
      const path = await e.pickImage();
      if (!path) {
        if (!e.isNative) setUi({ imageError: 'Opening photos needs the desktop app; the browser mock uses the test pattern.' });
        return;
      }
      await useImage(path);
    } catch (err) {
      setUi({ imageError: String(err instanceof Error ? err.message : err) });
    }
  };

  const commitCrop = (c: Crop) => {
    const [w, h] = imageSizePx(params, ui.image);
    const snapped = snapCrop(c, w, h, params.widthMm, params.cellMm, params.heightMm);
    image({ crop: isFullCrop(snapped.crop) ? undefined : snapped.crop });
  };

  const turn = (cw: boolean) => {
    // With the mirror on, a clockwise turn of the source is a counter-clockwise turn on screen.
    const quarters = cw !== !!o.mirror ? 1 : 3;
    const next = ((rotate + (cw ? 90 : 270)) % 360) as ImageOpts['rotate'];
    image({ rotate: next, crop: o.crop ? rotateCrop(o.crop, quarters, false) : undefined });
  };

  return (
    <section className="panel" aria-label="Image">
      <header className="panel-head">
        <h2>Image</h2>
      </header>
      <div className="group">
        <div className="row-buttons">
          <button type="button" onClick={openPhoto} title={engine?.isNative ? 'Open a photo' : 'Needs the desktop app'}>
            Open photo…
          </button>
          <button type="button" onClick={() => void useImage('synthetic')} disabled={params.image === 'synthetic'}>
            Test pattern
          </button>
        </div>
        <p className="note" title={params.image}>
          {fileName(params.image)}
          {imageLoading && ' · opening photo…'}
          {ui.image && ui.image.path === params.image && ` · ${ui.image.widthPx} × ${ui.image.heightPx} px`}
        </p>
        {ui.thumbnail && (
          <CropBox thumbnail={ui.thumbnail} crop={o.crop} nx={plate.nx} ny={plate.ny} onCommit={commitCrop} />
        )}
        {ui.imageError && <p className="error">{ui.imageError}</p>}
        <div className="row-buttons">
          <button type="button" onClick={() => turn(false)} title="Rotate 90° counter-clockwise">⟲</button>
          <button type="button" onClick={() => turn(true)} title="Rotate 90° clockwise">⟳</button>
          <button
            type="button"
            className={o.mirror ? 'active' : ''}
            onClick={() => image({ mirror: !o.mirror, crop: o.crop ? rotateCrop(o.crop, 0, true) : undefined })}
            title="Mirror left ↔ right"
          >
            Mirror
          </button>
          <button type="button" onClick={() => image({ crop: undefined })} disabled={!o.crop}>
            Full frame
          </button>
        </div>
      </div>

      <div className="group">
        <h3>Tone</h3>
        <label className="check">
          <input type="checkbox" checked={!!o.invert} onChange={(e) => image({ invert: e.currentTarget.checked })} />
          Invert
        </label>
        <label className="check">
          <input type="checkbox" checked={!!o.autoLevels} onChange={(e) => image({ autoLevels: e.currentTarget.checked ? AUTO_LEVELS : null })} />
          Auto levels (1st–99th percentile)
        </label>
        <Slider
          param="brightness" label="Brightness" min={-1} max={1} step={0.01} digits={2}
          value={o.brightness ?? 0}
          onChange={(v, ph) => image({ brightness: v }, ph)}
          onCommit={commit}
        />
        <Slider
          param="contrast" label="Contrast" min={0} max={3} step={0.01} digits={2}
          value={o.contrast ?? 1}
          onChange={(v, ph) => image({ contrast: v }, ph)}
          onCommit={commit}
        />
        <Slider
          param="gamma" label="Gamma" min={0.2} max={5} step={0.01} digits={2}
          value={o.gamma ?? 1}
          hint="v ← v^(1/γ): above 1 brightens mid-tones"
          onChange={(v, ph) => image({ gamma: v }, ph)}
          onCommit={commit}
        />
        <div className="row-buttons">
          <button
            type="button"
            onClick={() => image({ brightness: 0, contrast: 1, gamma: 1, invert: false, autoLevels: null })}
          >
            Reset tone
          </button>
        </div>
      </div>

      <div className="group">
        <h3>What you will see</h3>
        {ui.target && (
          <TargetView target={ui.target} elevationDeg={params.lightElevationDeg} maxTiltDeg={params.maxTiltDeg} ambient={view.ambient} />
        )}
        {ui.targetError && <p className="error">{ui.targetError}</p>}
      </div>
    </section>
  );
}
