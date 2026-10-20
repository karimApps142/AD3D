// A labelled range slider with a number box. Both update live (no Enter key needed); the number
// box accepts any value in range while typing and snaps its display on blur.
//
// onChange gets a phase (§13): 'drag' while the pointer holds the thumb, 'commit' for keyboard,
// typed and programmatic changes. onCommit fires on pointer release after a drag, so callers can
// regenerate at Quality::Final.

import { useId, useRef, useState } from 'react';
import type { ChangePhase } from './squamaDto';

export interface SliderProps {
  /** Exposed as data-param on the range input (used by tests and the spike harness). */
  param: string;
  label: string;
  unit?: string;
  min: number;
  max: number;
  step: number;
  value: number;
  /** Decimals shown in the number box. */
  digits?: number;
  hint?: string;
  disabled?: boolean;
  /** Text after the label, e.g. a snapped value. */
  extra?: string;
  onChange: (value: number, phase: ChangePhase) => void;
  /** Pointer released after at least one drag change. */
  onCommit?: () => void;
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

export function Slider({ param, label, unit, min, max, step, value, digits = 0, hint, disabled, extra, onChange, onCommit }: SliderProps) {
  const id = useId();
  const [draft, setDraft] = useState<string | null>(null);
  const shown = draft ?? value.toFixed(digits);
  const pointerDown = useRef(false);
  const dragged = useRef(false);
  const release = () => {
    if (!pointerDown.current) return;
    pointerDown.current = false;
    if (dragged.current) {
      dragged.current = false;
      onCommit?.();
    }
  };

  return (
    <div className="field" title={hint}>
      <div className="field-row">
        <label htmlFor={id}>
          {label}
          {extra && <span className="field-extra"> {extra}</span>}
        </label>
        <span className="field-value">
          <input
            className="num"
            type="text"
            inputMode="decimal"
            aria-label={`${label} value`}
            value={shown}
            disabled={disabled}
            onFocus={(e) => {
              setDraft(value.toFixed(digits));
              e.currentTarget.select();
            }}
            onChange={(e) => {
              const text = e.currentTarget.value;
              setDraft(text);
              const v = Number.parseFloat(text);
              if (Number.isFinite(v)) onChange(clamp(v, min, max), 'commit');
            }}
            onBlur={() => setDraft(null)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === 'Escape') {
                e.currentTarget.blur();
              } else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
                e.preventDefault();
                const dir = e.key === 'ArrowUp' ? 1 : -1;
                const next = clamp(value + dir * step * (e.shiftKey ? 10 : 1), min, max);
                const rounded = Number(next.toFixed(6));
                setDraft(rounded.toFixed(digits));
                onChange(rounded, 'commit');
              }
            }}
          />
          {unit && <span className="unit">{unit}</span>}
        </span>
      </div>
      <input
        id={id}
        type="range"
        data-param={param}
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onPointerDown={() => {
          pointerDown.current = true;
          dragged.current = false;
          // WebKit may deliver the release outside the input; catch it on the window too.
          window.addEventListener('pointerup', release, { once: true });
        }}
        onPointerUp={release}
        onPointerCancel={release}
        onLostPointerCapture={release}
        onBlur={release}
        onChange={(e) => {
          const v = e.currentTarget.valueAsNumber;
          if (pointerDown.current) dragged.current = true;
          onChange(v, pointerDown.current ? 'drag' : 'commit');
        }}
      />
    </div>
  );
}
