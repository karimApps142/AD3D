// Photo thumbnail with an interactive crop rectangle. Drag inside to move, drag an edge or corner
// to resize, drag outside to draw a new one; on release the crop snaps to whole cells and the
// plate regenerates at final quality. The cell grid is drawn over the crop when it is legible.

import { useEffect, useMemo, useRef, useState } from 'react';
import type { GrayImage } from '../engine/types';
import { type Crop, type CropHandle, dragCrop, FULL_CROP, hitCrop } from './cropMath';
import { grayToRgba, paint } from './gridImage';

export interface CropBoxProps {
  thumbnail: GrayImage;
  crop: Crop | undefined;
  /** Plate grid inside the crop, for the overlay. */
  nx: number;
  ny: number;
  onCommit: (crop: Crop) => void;
}

const CURSORS: Record<CropHandle, string> = {
  move: 'move',
  new: 'crosshair',
  n: 'ns-resize',
  s: 'ns-resize',
  e: 'ew-resize',
  w: 'ew-resize',
  nw: 'nwse-resize',
  se: 'nwse-resize',
  ne: 'nesw-resize',
  sw: 'nesw-resize',
};

export function CropBox({ thumbnail, crop, nx, ny, onCommit }: CropBoxProps) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const [draft, setDraft] = useState<Crop | null>(null);
  const [hover, setHover] = useState<CropHandle>('move');
  const drag = useRef<{ handle: CropHandle; start: Crop; sx: number; sy: number; id: number } | null>(null);
  const shown = draft ?? crop ?? FULL_CROP;

  useEffect(() => {
    paint(canvas.current, grayToRgba(thumbnail), thumbnail.width, thumbnail.height);
  }, [thumbnail]);

  const point = (e: React.PointerEvent): [number, number, number, number] => {
    const r = box.current!.getBoundingClientRect();
    const x = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
    const y = Math.min(1, Math.max(0, (e.clientY - r.top) / r.height));
    return [x, y, 8 / r.width, 8 / r.height];
  };

  const gridLines = useMemo(() => {
    const [x0, y0, x1, y1] = shown;
    // Skip the grid when cells would be under 3 px on screen (about 300 px wide thumbnails).
    if (nx > 100 || ny > 100 || draft) return null;
    const v: number[] = [];
    const h: number[] = [];
    for (let i = 1; i < nx; i++) v.push(x0 + ((x1 - x0) * i) / nx);
    for (let j = 1; j < ny; j++) h.push(y0 + ((y1 - y0) * j) / ny);
    return { v, h };
  }, [shown, nx, ny, draft]);

  const [x0, y0, x1, y1] = shown;
  const pct = (v: number) => `${(v * 100).toFixed(3)}%`;

  return (
    <div
      ref={box}
      className="crop-box"
      style={{ aspectRatio: `${thumbnail.width} / ${thumbnail.height}`, cursor: CURSORS[drag.current?.handle ?? hover] }}
      onPointerDown={(e) => {
        const [x, y, tx, ty] = point(e);
        const handle = hitCrop(shown, x, y, tx, ty);
        drag.current = { handle, start: shown, sx: x, sy: y, id: e.pointerId };
        e.currentTarget.setPointerCapture(e.pointerId);
      }}
      onPointerMove={(e) => {
        const [x, y, tx, ty] = point(e);
        const d = drag.current;
        if (!d) {
          setHover(hitCrop(shown, x, y, tx, ty));
          return;
        }
        setDraft(dragCrop(d.start, d.handle, d.sx, d.sy, x, y));
      }}
      onPointerUp={(e) => {
        const d = drag.current;
        drag.current = null;
        if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
        if (d && draft) onCommit(draft);
        setDraft(null);
      }}
      onPointerCancel={() => {
        drag.current = null;
        setDraft(null);
      }}
      onDoubleClick={() => onCommit(FULL_CROP)}
      title="Drag to crop (snaps to whole cells); double-click for the full image"
    >
      <canvas ref={canvas} className="crop-canvas" />
      <svg className="crop-overlay" viewBox="0 0 1 1" preserveAspectRatio="none" aria-hidden="true">
        <path
          className="crop-shade"
          fillRule="evenodd"
          d={`M0 0H1V1H0Z M${x0} ${y0}V${y1}H${x1}V${y0}Z`}
        />
        {gridLines?.v.map((x) => <line key={`v${x}`} className="crop-grid" x1={x} x2={x} y1={y0} y2={y1} />)}
        {gridLines?.h.map((y) => <line key={`h${y}`} className="crop-grid" x1={x0} x2={x1} y1={y} y2={y} />)}
      </svg>
      <div className="crop-rect" style={{ left: pct(x0), top: pct(y0), width: pct(x1 - x0), height: pct(y1 - y0) }}>
        {(['nw', 'ne', 'sw', 'se'] as const).map((h) => (
          <span key={h} className={`crop-handle ${h}`} />
        ))}
      </div>
    </div>
  );
}
