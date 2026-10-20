// The plate layout the panels show: predicted from the params and the photo (§2.1) so it updates
// instantly, then the core's report once it agrees on the same width and cell.

import { MOCK_IMAGE_PX, orientedSize } from '../engine/mockTarget';
import { predictLayout, type PredictedLayout } from '../engine/squamaChecks';
import type { ImageInfo, SquamaJobInfo, SquamaParams } from '../engine/types';

/** Oriented size (px) of the image the params point at. */
export function imageSizePx(params: SquamaParams, image: ImageInfo | null): [number, number] {
  const base: [number, number] =
    params.image !== 'synthetic' && image && image.path === params.image
      ? [image.workingWidthPx, image.workingHeightPx]
      : MOCK_IMAGE_PX;
  return orientedSize(base[0], base[1], params.imageOpts?.rotate ?? 0);
}

export function predictPlate(params: SquamaParams, image: ImageInfo | null): PredictedLayout {
  const [w, h] = imageSizePx(params, image);
  const c = params.imageOpts?.crop ?? [0, 0, 1, 1];
  return predictLayout(params.widthMm, params.cellMm, (c[2] - c[0]) * w, (c[3] - c[1]) * h, params.heightMm);
}

/** Core report when it matches the current snapping (a stale report never wins), else the prediction. */
export function plateLayout(params: SquamaParams, image: ImageInfo | null, info: SquamaJobInfo | null): PredictedLayout {
  const predicted = predictPlate(params, image);
  if (info && info.layout.nx === predicted.nx && info.layout.ny === predicted.ny && Math.abs(info.layout.cellMm - predicted.cellMm) < 1e-9) {
    const { nx, ny, wMm, hMm, cellMm } = info.layout;
    return { nx, ny, wMm, hMm, cellMm };
  }
  return predicted;
}
