// Alt/Option + drag in the viewport swings the preview light: horizontal = azimuth (like turning a
// dial), vertical = elevation. For the point lamp it orbits the plate centre at a fixed distance;
// Alt + wheel changes that distance. The handlers listen in the capture phase on the viewport
// wrapper and stop the event there, so OrbitControls never orbits or zooms for those gestures.

import { type RefObject, useEffect } from 'react';
import { wrapDeg } from '../engine/squamaMath';
import type { Vec3 } from '../engine/types';
import { selectPreviewLight, useAppStore } from '../state/store';
import { designPointLight, lampAngles } from './previewLight';
import { viewportBus } from './viewportBus';
import { LAMP_DISTANCE_RANGE, useViewportStore } from './viewportStore';

/** Degrees of azimuth per pixel of horizontal drag. */
const AZIMUTH_PER_PX = 0.4;
/** Degrees of elevation per pixel of vertical drag. */
const ELEVATION_PER_PX = 0.15;
export const PREVIEW_ELEVATION_MAX = 90;
/** Lamp distance factor per wheel pixel (Alt + wheel). */
const DISTANCE_PER_WHEEL_PX = 0.0015;

function plateCentre(): Vec3 {
  const b = viewportBus.modelBox;
  return [(b.min.x + b.max.x) / 2, (b.min.y + b.max.y) / 2, b.max.z];
}

/** Starting angles for a drag; a locked design lamp hands its position over to the free lamp. */
function dragStart(): { az: number; elev: number } {
  const app = useAppStore.getState();
  const lamp = designPointLight(app.params);
  if (app.view.followDesignLight && lamp) {
    const a = lampAngles(lamp, plateCentre());
    useViewportStore.getState().set({ lightKind: 'point', lampDistanceMm: Math.round(a.distanceMm) });
    return { az: a.azimuthDeg, elev: Math.max(0, a.elevationDeg) };
  }
  const light = selectPreviewLight(app);
  return { az: light.azimuthDeg, elev: light.elevationDeg };
}

/** New lamp distance after a wheel step, clamped to LAMP_DISTANCE_RANGE. */
export function wheelDistance(current: number, deltaY: number): number {
  const d = current * Math.exp(deltaY * DISTANCE_PER_WHEEL_PX);
  return Math.round(Math.min(LAMP_DISTANCE_RANGE[1], Math.max(LAMP_DISTANCE_RANGE[0], d)));
}

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
      const start = dragStart();
      drag = { id: e.pointerId, x: e.clientX, y: e.clientY, az: start.az, elev: start.elev };
      el.setPointerCapture(e.pointerId);
      useAppStore.getState().setView({
        followDesignLight: false,
        previewAzimuthDeg: start.az,
        previewElevationDeg: start.elev,
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

    const wheel = (e: WheelEvent) => {
      if (!e.altKey) return;
      const vp = useViewportStore.getState();
      if (vp.lightKind !== 'point') return;
      e.preventDefault();
      e.stopPropagation();
      if (useAppStore.getState().view.followDesignLight && designPointLight(useAppStore.getState().params)) {
        const start = dragStart();
        useAppStore.getState().setView({ followDesignLight: false, previewAzimuthDeg: start.az, previewElevationDeg: start.elev });
      }
      vp.set({ lampDistanceMm: wheelDistance(useViewportStore.getState().lampDistanceMm, e.deltaY) });
    };

    el.addEventListener('wheel', wheel, { capture: true, passive: false });
    el.addEventListener('pointerdown', down, { capture: true });
    el.addEventListener('pointermove', move, { capture: true });
    el.addEventListener('pointerup', up, { capture: true });
    el.addEventListener('pointercancel', up, { capture: true });
    window.addEventListener('keydown', onKey);
    window.addEventListener('keyup', onKey);
    window.addEventListener('blur', onBlur);
    return () => {
      el.removeEventListener('wheel', wheel, { capture: true });
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
