// A labelled range slider with a number box. Both update live: while typing, the number box
// sends preview-quality changes; Enter or leaving the box sends the final value, Esc reverts to
// the value the box had when it was focused (APP-LIVE). Text that is not a number is refused and
// the box shows an error state.
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

/** The number in a typed text, or null unless the whole text is one decimal number. */
export function parseTyped(text: string): number | null {
  if (!/^\s*[-+]?(\d+\.?\d*|\.\d+)\s*$/.test(text)) return null;
  const v = Number(text);
  return Number.isFinite(v) ? v : null;
}

export function Slider({ param, label, unit, min, max, step, value, digits = 0, hint, disabled, extra, onChange, onCommit }: SliderProps) {
  const id = useId();
  const [draft, setDraft] = useState<string | null>(null);
  const shown = draft ?? value.toFixed(digits);
  const pointerDown = useRef(false);
  const dragged = useRef(false);
  /** Value when the box was focused (Esc restores it); whether typing sent a preview. */
  const atFocus = useRef(value);
  const typed = useRef(false);
  const invalid = draft !== null && parseTyped(draft) === null;
  const finishTyping = (revert: boolean) => {
    if (revert) {
      if (typed.current || value !== atFocus.current) onChange(atFocus.current, 'commit');
    } else if (typed.current) {
      const v = draft !== null ? parseTyped(draft) : null;
      onChange(v === null ? value : clamp(v, min, max), 'commit');
    }
    typed.current = false;
    setDraft(null);
  };
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
            className={`num${invalid ? ' invalid' : ''}`}
            type="text"
            inputMode="decimal"
            aria-label={`${label} value`}
            aria-invalid={invalid || undefined}
            value={shown}
            disabled={disabled}
            onFocus={(e) => {
              atFocus.current = value;
              typed.current = false;
              setDraft(value.toFixed(digits));
              e.currentTarget.select();
            }}
            onChange={(e) => {
              const text = e.currentTarget.value;
              setDraft(text);
              const v = parseTyped(text);
              if (v !== null) {
                typed.current = true;
                onChange(clamp(v, min, max), 'drag');
              }
            }}
            onBlur={() => {
              if (draft !== null) finishTyping(false);
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                finishTyping(false);
                e.currentTarget.blur();
              } else if (e.key === 'Escape') {
                finishTyping(true);
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
