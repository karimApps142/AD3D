// Verify round 1 (M1 correctness review): a photo opened through File → Open / Import /
// drag-and-drop (actions.useImage) must be set up like one opened from the Image panel.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { imageSizePx } from '../panels/plateLayout';
import { useSquamaUi } from '../panels/squamaUi';
import { startParamsHistory } from '../state/paramsHistory';
import { DEFAULT_PARAMS, useAppStore } from '../state/store';
import * as actions from './actions';

const PHOTO = '/Users/me/Pictures/landscape.jpg';

vi.mock('../engine', () => ({
  getEngine: async () => ({
    isNative: true,
    imageInfo: async (path: string) => ({ path, widthPx: 4000, heightPx: 3000, workingWidthPx: 2048, workingHeightPx: 1536 }),
  }),
}));

let stop: () => void = () => {};
beforeEach(() => {
  useAppStore.setState({
    params: { ...DEFAULT_PARAMS, heightMm: 120, imageOpts: { crop: [0.1, 0.2, 0.6, 0.9], rotate: 90 } },
  });
  useSquamaUi.setState({ image: null });
  stop = startParamsHistory();
});
afterEach(() => stop());

describe('actions.useImage', () => {
  it('starts the new photo uncropped and unrotated, like ImagePanel.openPhoto', async () => {
    await actions.useImage(PHOTO);
    const p = useAppStore.getState().params;
    expect(p.image).toBe(PHOTO);
    // The previous photo's crop, rotation and fixed height silently carry over otherwise.
    expect(p.imageOpts).toBeUndefined();
    expect(p.heightMm).toBeUndefined();
  });

  it('loads the photo size, so the panels and the point lamp use the real plate size', async () => {
    await actions.useImage(PHOTO);
    // Without ImageInfo for this path, imageSizePx() falls back to the 500×630 mock size: the
    // predicted layout (and the lamp position derived from it) is for a portrait plate.
    await vi.waitFor(() => expect(useSquamaUi.getState().image?.path).toBe(PHOTO), { timeout: 500 });
    expect(imageSizePx(useAppStore.getState().params, useSquamaUi.getState().image)).toEqual([2048, 1536]);
  });
});
