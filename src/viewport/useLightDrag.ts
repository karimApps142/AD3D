// Alt/Option + drag in the viewport swings the preview light: horizontal = azimuth (like turning a
// dial), vertical = elevation. The handler listens in the capture phase on the viewport wrapper and
// stops the event there, so OrbitControls never starts an orbit for that drag.

import { type RefObject, useEffect } from 'react';
import { wrapDeg } from '../engine/squamaMath';
import { selectPreviewLight, useAppStore } from '../state/store';

/** Degrees of azimuth per pixel of horizontal drag. */
const AZIMUTH_PER_PX = 0.4;
/** Degrees of elevation per pixel of vertical drag. */
const ELEVATION_PER_PX = 0.15;
export const PREVIEW_ELEVATION_MAX = 90;

export function useLightDrag(ref: RefObject<HTMLElement | null>): void {
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let drag: { id: number; x: number; y: number; az: number; elev: number } | null = null;

    const setAlt = (on: boolean) => el.classList.toggle('alt-held', on);
    const onKey = (e: KeyboardEvent) => setAlt(e.altKey);
    const onBlur = () => setAlt(false);

    const down = (e: PointerEvent) => {
      if (!e.altKey || e.button !== 0) return;
      e.preventDefault();
      e.stopPropagation();
      const light = selectPreviewLight(useAppStore.getState());
      drag = { id: e.pointerId, x: e.clientX, y: e.clientY, az: light.azimuthDeg, elev: light.elevationDeg };
      el.setPointerCapture(e.pointerId);
      useAppStore.getState().setView({
        followDesignLight: false,
        previewAzimuthDeg: light.azimuthDeg,
        previewElevationDeg: light.elevationDeg,
        lightDragging: true,
      });
    };
    const move = (e: PointerEvent) => {
      if (!drag || e.pointerId !== drag.id) return;
      e.stopPropagation();
      const az = wrapDeg(drag.az - (e.clientX - drag.x) * AZIMUTH_PER_PX);
      const elev = Math.min(PREVIEW_ELEVATION_MAX, Math.max(0, drag.elev - (e.clientY - drag.y) * ELEVATION_PER_PX));
      useAppStore.getState().setView({
        previewAzimuthDeg: Math.round(az * 10) / 10,
        previewElevationDeg: Math.round(elev * 10) / 10,
      });
    };
    const up = (e: PointerEvent) => {
      if (!drag || e.pointerId !== drag.id) return;
      e.stopPropagation();
      if (el.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId);
      drag = null;
      useAppStore.getState().setView({ lightDragging: false });
    };

    el.addEventListener('pointerdown', down, { capture: true });
    el.addEventListener('pointermove', move, { capture: true });
    el.addEventListener('pointerup', up, { capture: true });
    el.addEventListener('pointercancel', up, { capture: true });
    window.addEventListener('keydown', onKey);
    window.addEventListener('keyup', onKey);
    window.addEventListener('blur', onBlur);
    return () => {
      el.removeEventListener('pointerdown', down, { capture: true });
      el.removeEventListener('pointermove', move, { capture: true });
      el.removeEventListener('pointerup', up, { capture: true });
      el.removeEventListener('pointercancel', up, { capture: true });
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('keyup', onKey);
      window.removeEventListener('blur', onBlur);
    };
  }, [ref]);
}
