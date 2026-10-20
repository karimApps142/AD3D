// A labelled range slider with a number box. Both update live (no Enter key needed); the number
// box accepts any value in range while typing and snaps its display on blur.

import { useId, useState } from 'react';

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
  onChange: (value: number) => void;
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

export function Slider({ param, label, unit, min, max, step, value, digits = 0, hint, disabled, onChange }: SliderProps) {
  const id = useId();
  const [draft, setDraft] = useState<string | null>(null);
  const shown = draft ?? value.toFixed(digits);

  return (
    <div className="field" title={hint}>
      <div className="field-row">
        <label htmlFor={id}>{label}</label>
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
              if (Number.isFinite(v)) onChange(clamp(v, min, max));
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
                onChange(rounded);
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
        onChange={(e) => onChange(e.currentTarget.valueAsNumber)}
      />
    </div>
  );
}
