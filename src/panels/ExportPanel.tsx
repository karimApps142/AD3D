// Tiling and export: printer profile (CR-6 SE default), bed fit, tile layout preview, keys, stand,
// calibration tile, formats and orientation; then a native save dialog, progress streamed over a
// Tauri channel, cancel, and "Reveal in Finder" when done.

import { useMemo } from 'react';
import { useShallow } from 'zustand/react/shallow';
import { getEngine } from '../engine';
import { mergeWarnings } from '../engine/squamaChecks';
import { useSquamaInfo } from '../engine/squamaInfo';
import type { CoreWarning, ExportFormat, Vec3 } from '../engine/types';
import { useAppStore } from '../state/store';
import { plateLayout } from './plateLayout';
import { buildExportDto, defaultExportName } from './squamaDto';
import { type ExportStatus, IDLE_EXPORT, reduceExportEvent, useSquamaUi } from './squamaUi';
import { fitsBed, maxTileExtent, ORIENTATIONS, planTiles, PRINTERS, printSize, quarterTurns } from './tilePlan';
import { Warnings } from './Warnings';
import './panels.css';

function TilePreview({ cols, rows, wMm, hMm }: { cols: number[]; rows: number[]; wMm: number; hMm: number }) {
  const nx = cols.reduce((a, b) => a + b, 0);
  const ny = rows.reduce((a, b) => a + b, 0);
  const rects: Array<{ x: number; y: number; w: number; h: number; label: string }> = [];
  let y = 0;
  // rows are bottom → top; SVG y runs down, so place the top row first.
  for (let r = rows.length - 1; r >= 0; r--) {
    let x = 0;
    for (let c = 0; c < cols.length; c++) {
      rects.push({ x: x / nx, y: y / ny, w: cols[c] / nx, h: rows[r] / ny, label: `r${rows.length - r}c${c + 1}` });
      x += cols[c];
    }
    y += rows[r];
  }
  return (
    <svg className="tile-preview" viewBox={`0 0 ${wMm} ${hMm}`} role="img" aria-label={`${cols.length} × ${rows.length} tiles`}>
      {rects.map((t) => (
        <g key={t.label}>
          <rect x={t.x * wMm} y={t.y * hMm} width={t.w * wMm} height={t.h * hMm} />
          <text x={(t.x + t.w / 2) * wMm} y={(t.y + t.h / 2) * hMm} fontSize={Math.min(wMm, hMm) / 10}>
            {cols.length * rows.length > 1 ? t.label : 'one piece'}
          </text>
        </g>
      ))}
    </svg>
  );
}

export function ExportPanel() {
  const params = useAppStore((s) => s.params);
  const meshDepth = useAppStore((s) => (s.mesh?.kind === 'squama' ? s.mesh.payload.bboxMax[2] - s.mesh.payload.bboxMin[2] : null));
  const engine = useAppStore((s) => s.engine);
  const info = useSquamaInfo((s) => s.info);
  const { image, settings, status } = useSquamaUi(useShallow((s) => ({ image: s.image, settings: s.export, status: s.exportStatus })));
  const setExport = useSquamaUi((s) => s.setExport);
  const setStatus = useSquamaUi((s) => s.setExportStatus);

  const layout = plateLayout(params, image, info);
  const k = quarterTurns(settings.orientation, params.lightAzimuthDeg);
  const depth = meshDepth ?? params.baseMm + 1;
  const wholeSize = printSize(layout.wMm, layout.hMm, depth, k);
  const wholeFits = fitsBed(wholeSize, settings.bedMm, settings.marginMm);
  const plan = useMemo(
    () => planTiles(layout.nx, layout.ny, layout.cellMm, maxTileExtent(settings.orientation, settings.bedMm, settings.marginMm, params.lightAzimuthDeg)),
    [layout.nx, layout.ny, layout.cellMm, settings.orientation, settings.bedMm, settings.marginMm, params.lightAzimuthDeg],
  );
  const tiles = settings.tile ? plan : null;
  const running = status.phase === 'running';

  const warnings = useMemo(() => {
    const w: CoreWarning[] = [];
    if (settings.keys === 'pegs' && params.baseMm < 2.8) {
      w.push({ code: 'keys_need_thickness', message: `Alignment keys need thickness ≥ 2.8 mm (now ${params.baseMm.toFixed(1)} mm).` });
    }
    if (!settings.tile && !wholeFits) w.push({ code: 'plate_exceeds_bed', message: 'The plate does not fit the bed in one piece; turn on tiling.' });
    if (plan.error) w.push({ code: 'tile_plan', message: plan.error });
    if (settings.orientation === 'flat') w.push({ code: 'flat_print', message: 'Printed flat, tilted facets come out poorly; standing is recommended.' });
    if (!settings.formats.length) w.push({ code: 'no_format', message: 'Choose STL and/or 3MF.' });
    return mergeWarnings(w, status.warnings);
  }, [settings, params.baseMm, wholeFits, plan.error, status.warnings]);

  const toggleFormat = (f: ExportFormat, on: boolean) =>
    setExport({ formats: on ? Array.from(new Set([...settings.formats, f])) : settings.formats.filter((x) => x !== f) });

  const setBed = (i: number, v: number) => {
    const bed = [...settings.bedMm] as Vec3;
    bed[i] = v;
    setExport({ bedMm: bed, printerId: 'custom' });
  };

  const startExport = async () => {
    try {
      const e = await getEngine();
      const path = await e.pickSavePath(defaultExportName(params, settings.formats));
      if (!path) return;
      const dto = buildExportDto(params, settings, path);
      let st: ExportStatus = { ...IDLE_EXPORT, phase: 'running', stage: 'starting' };
      setStatus(st);
      const result = await e.exportSquama(dto, (ev) => {
        st = reduceExportEvent(st, ev);
        setStatus(st);
      });
      setStatus({ ...st, phase: 'done', fraction: 1, result, warnings: mergeWarnings(st.warnings, result.warnings) });
    } catch (err) {
      const msg = String(err instanceof Error ? err.message : err);
      setStatus({ phase: msg === 'cancelled' ? 'cancelled' : 'error', error: msg === 'cancelled' ? null : msg });
    }
  };

  const reveal = async (path: string) => {
    try {
      await (await getEngine()).revealInFinder(path);
    } catch (err) {
      setStatus({ error: String(err instanceof Error ? err.message : err) });
    }
  };

  const printer = PRINTERS.find((p) => p.id === settings.printerId) ?? PRINTERS[PRINTERS.length - 1];

  return (
    <section className="panel" aria-label="Tiling and export">
      <header className="panel-head">
        <h2>Tiling &amp; export</h2>
      </header>
      <div className="group">
        <h3>Printer</h3>
        <select
          aria-label="Printer profile"
          value={printer.id}
          onChange={(e) => {
            const p = PRINTERS.find((x) => x.id === e.currentTarget.value)!;
            setExport({ printerId: p.id, bedMm: [...p.bedMm] as Vec3 });
          }}
        >
          {PRINTERS.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
              {p.id !== 'custom' ? ` (${p.bedMm.join(' × ')} mm)` : ''}
            </option>
          ))}
        </select>
        <div className="bed-row">
          {(['X', 'Y', 'Z'] as const).map((axis, i) => (
            <label key={axis}>
              {axis}
              <input
                type="number"
                className="num"
                min={20}
                max={2000}
                value={settings.bedMm[i]}
                onChange={(e) => {
                  const v = e.currentTarget.valueAsNumber;
                  if (Number.isFinite(v) && v > 0) setBed(i, v);
                }}
              />
            </label>
          ))}
          <label>
            Margin
            <input
              type="number"
              className="num"
              min={0}
              max={30}
              value={settings.marginMm}
              onChange={(e) => {
                const v = e.currentTarget.valueAsNumber;
                if (Number.isFinite(v) && v >= 0) setExport({ marginMm: v });
              }}
            />
          </label>
        </div>
        {printer.bedMotion && <p className="note">Bed moves in {printer.bedMotion.toUpperCase()}: keep tall tiles short and use a brim.</p>}
        <select
          aria-label="Print orientation"
          value={settings.orientation}
          onChange={(e) => setExport({ orientation: e.currentTarget.value as typeof settings.orientation })}
        >
          {ORIENTATIONS.map((o) => (
            <option key={o.id} value={o.id}>
              {o.label}
            </option>
          ))}
        </select>
        <p className={`fit ${wholeFits ? 'ok' : 'over'}`} data-testid="bed-fit">
          {wholeFits ? '✓ Fits the bed in one piece' : '✗ Larger than the bed'} · {wholeSize.map((v) => v.toFixed(0)).join(' × ')} mm
        </p>
      </div>

      <div className="group">
        <h3>Tiles</h3>
        <label className="check">
          <input type="checkbox" checked={settings.tile} onChange={(e) => setExport({ tile: e.currentTarget.checked })} />
          Split to the bed
        </label>
        {tiles && !tiles.error && (
          <>
            <TilePreview cols={tiles.cols} rows={tiles.rows} wMm={layout.wMm} hMm={layout.hMm} />
            <p className="derived">
              {tiles.cols.length} × {tiles.rows.length} tiles · columns {tiles.cols.join(' + ')} · rows {tiles.rows.join(' + ')} cells
            </p>
          </>
        )}
        <div className="segmented" role="group" aria-label="Alignment keys">
          <button type="button" className={settings.keys === 'none' ? 'active' : ''} onClick={() => setExport({ keys: 'none' })}>
            No keys
          </button>
          <button type="button" className={settings.keys === 'pegs' ? 'active' : ''} onClick={() => setExport({ keys: 'pegs' })}>
            Peg keys
          </button>
        </div>
        <label className="check">
          <input type="checkbox" checked={settings.stand} onChange={(e) => setExport({ stand: e.currentTarget.checked })} />
          Stand
        </label>
        <label className="check">
          <input type="checkbox" checked={settings.calibrationTile} onChange={(e) => setExport({ calibrationTile: e.currentTarget.checked })} />
          Calibration tile (grey ramp; print this first)
        </label>
      </div>

      <div className="group">
        <h3>Export</h3>
        <div className="row-buttons">
          <label className="check">
            <input type="checkbox" checked={settings.formats.includes('3mf')} onChange={(e) => toggleFormat('3mf', e.currentTarget.checked)} />
            3MF
          </label>
          <label className="check">
            <input type="checkbox" checked={settings.formats.includes('stl')} onChange={(e) => toggleFormat('stl', e.currentTarget.checked)} />
            STL
          </label>
        </div>
        <div className="row-buttons">
          <button
            type="button"
            className="primary"
            disabled={running || !settings.formats.length || !engine?.isNative}
            title={engine?.isNative ? 'Choose where to save' : 'Export needs the desktop app'}
            onClick={startExport}
          >
            Export…
          </button>
          {running && (
            <button type="button" onClick={() => getEngine().then((e) => e.cancelExport())}>
              Cancel
            </button>
          )}
        </div>
        {running && (
          <div className="progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(status.fraction * 100)}>
            <div className="progress-bar" style={{ width: `${(status.fraction * 100).toFixed(1)}%` }} />
            <span>
              {status.part ? `${status.part}: ` : ''}
              {status.stage}
            </span>
          </div>
        )}
        {status.phase === 'done' && status.result && (
          <div className="export-done">
            <p>
              Wrote {status.result.files.length} file{status.result.files.length === 1 ? '' : 's'} in {(status.result.elapsedMs / 1000).toFixed(1)} s.
            </p>
            <ul className="file-list">
              {status.result.parts.map((p) => (
                <li key={p.name}>
                  {p.name} · {p.sizeMm.map((v) => v.toFixed(0)).join(' × ')} mm{p.fitsBed === false ? ' · too big' : ''}
                </li>
              ))}
            </ul>
            <button type="button" onClick={() => reveal(status.result!.files[0] ?? status.result!.outDir)}>
              Reveal in Finder
            </button>
          </div>
        )}
        {status.phase === 'cancelled' && <p className="note">Export cancelled; nothing more was written.</p>}
        {status.error && <p className="error" role="alert">{status.error}</p>}
        <Warnings items={warnings} label="Export warnings" />
      </div>
    </section>
  );
}
