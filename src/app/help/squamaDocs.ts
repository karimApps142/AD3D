// Help drawer content for the Shadow Plate (Squama) generator, condensed by hand from
// docs/spec/FEATURES.md → PROJ-SQUAMA, PROJ-SQUAMA-TILE and PROJ-SQUAMA-CAL. Our own words; keep
// it in step with the spec when parameters change.

export interface ParamDoc {
  /** Store key (camelCase, matches SquamaParams). */
  key: string;
  label: string;
  unit?: string;
  default: string;
  range: string;
  meaning: string;
  tips?: string[];
  /** Parameter is planned for M1 but not yet in the panel. */
  planned?: boolean;
}

export interface HelpSection {
  title: string;
  body: string[];
}

export const SQUAMA_SUMMARY: HelpSection = {
  title: 'Shadow Plate',
  body: [
    'One photo becomes one solid plate covered with small flat facets, each tilted by a different amount.',
    'Under a single low, grazing lamp from the designed side each facet reads lighter or darker by its tilt, and the photo appears. Under room light alone the plate looks almost blank.',
    'Print it standing up, like a lithophane, and light it in a dark room from the side it was designed for.',
  ],
};

export const SQUAMA_PARAMS: ParamDoc[] = [
  {
    key: 'image',
    label: 'Image',
    default: 'built-in test pattern',
    range: 'PNG, JPEG, BMP, TIFF, WebP',
    meaning: 'The photo to reproduce. Any subject works; strong, simple shapes read best at small sizes.',
    tips: ['Drop an image on the window or use File → Import Image… (⌘I).', 'The plate height follows the image aspect.'],
  },
  {
    key: 'widthMm',
    label: 'Width',
    unit: 'mm',
    default: '300',
    range: '20–800, snapped to whole cells',
    meaning: 'Plate width. It is always a whole number of cells: the typed value is rounded to the nearest multiple of the cell size.',
    tips: [
      'The CR-6 SE bed is 235 × 235 × 250 mm. A 4:5 photo at cell 4 fits upright at about 196 × 244 mm; larger plates need tiling.',
      'More cells give a sharper picture: 80 mm at cell 4 is soft, 200 mm is much better.',
    ],
  },
  {
    key: 'cellMm',
    label: 'Cell size',
    unit: 'mm',
    default: '6 (suggest 4)',
    range: '1–20',
    meaning: 'Edge length of one facet. Smaller cells mean more facets and a sharper image, but less relief per facet.',
    tips: ['Facet relief is only about cell/2 × tan(max tilt): 0.35–0.7 mm at cell 4 and 10°. Cells under about 3 mm may not read in print (unconfirmed until the calibration tile is printed).'],
  },
  {
    key: 'baseMm',
    label: 'Base thickness',
    unit: 'mm',
    default: '3',
    range: '0.6–10',
    meaning: 'Minimum material behind any facet. The relief sits on top of it.',
    tips: ['Keys between tiles need at least 2.8 mm.'],
  },
  {
    key: 'lightAzimuthDeg',
    label: 'Light azimuth',
    unit: '°',
    default: '180 (from the left)',
    range: '0–360: 0 right, 90 top, 180 left, 270 bottom',
    meaning: 'Which side the lamp shines from, in the plane of the plate. It sets the tilt axis of the facets.',
    tips: ['Left/Right light tilts facets in vertical columns; Top/Bottom light tilts them in horizontal rows, which may need support when printed upright.'],
  },
  {
    key: 'lightElevationDeg',
    label: 'Light elevation',
    unit: '°',
    default: '5 (suggest 10)',
    range: '0–20',
    meaning: 'Height of the lamp above the plate plane. 0 means the light skims along the surface.',
    tips: [
      'A dark room and 10–20° matter most for contrast.',
      'True black needs max tilt ≥ elevation; otherwise the darkest facet still catches light.',
    ],
  },
  {
    key: 'maxTiltDeg',
    label: 'Max tilt',
    unit: '°',
    default: '10',
    range: '1–20',
    meaning: 'Largest facet tilt, in both directions. More tilt gives more contrast and deeper steps.',
    tips: ['At 1° the plate looks blank. Above 20° the steps between rows start to need support.'],
  },
  {
    key: 'layout',
    label: 'Cell shape',
    default: 'square',
    range: 'square / hex (pointy or flat)',
    meaning: 'Square grid like LuBan, or hexagonal cells where only three cells meet at each corner.',
    planned: true,
  },
  {
    key: 'imageOpts',
    label: 'Image tone',
    default: 'gamma 1, contrast 1, brightness 0, no invert, auto-levels off',
    range: 'gamma 0.2–5, contrast 0–3, brightness −1…1',
    meaning: 'Crop and tone controls applied in linear light before the photo is averaged per facet.',
    planned: true,
  },
  {
    key: 'response',
    label: 'Response model',
    default: 'lambert',
    range: 'lambert / measured',
    meaning: 'How tilt maps to brightness: the ideal Lambert model, or the curve measured from a printed calibration tile.',
    planned: true,
  },
  {
    key: 'light',
    label: 'Point lamp',
    default: 'off (directional light)',
    range: 'lamp position in mm',
    meaning: 'Model a real lamp at a known distance so each facet gets its own light direction and falloff.',
    planned: true,
  },
];

export const SQUAMA_VIEW_NOTES: HelpSection = {
  title: 'Preview',
  body: [
    'The plate regenerates while you drag a slider (preview quality) and refines when you let go.',
    'The preview light follows the design light. Unlock it in the View panel to check the plate from other angles; Alt+drag in the viewport swings the light.',
    'Ambient is the room light as a fraction of the lamp. Raise it to see how much room light washes the image out.',
  ],
};

export const SQUAMA_PRINT_CHECKLIST: HelpSection = {
  title: 'Print and light checklist',
  body: [
    'Material: matte white or light-grey PLA. Gloss hides the shading.',
    'Orientation: stand the plate up with the facet tilt axis vertical and the face parallel to the bed motion (Y on the CR-6 SE). Use the export’s auto orientation.',
    'Layer height about half the nozzle diameter (0.2 mm for a 0.4 mm nozzle); print slowly, 30–50 mm/s.',
    'Belts tight, extruder clean; add a brim or the stabiliser ribs for tall plates.',
    'Tiles: print each tile in the same orientation and settings; join them with the printed pegs.',
    'Lighting: dark room, one small lamp close to the plate at the design elevation, from the design side.',
    'If facets do not read, print the calibration tile and switch the response model to measured.',
  ],
};
