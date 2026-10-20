// The 3D view: r3f canvas with shadows, on-demand rendering and the overlays drawn on top of it.

import { OrbitControls } from '@react-three/drei';
import { Canvas } from '@react-three/fiber';
import { useRef } from 'react';
import { StatsOverlay } from '../panels/StatsOverlay';
import { useAppStore } from '../state/store';
import { Backdrop, BACKGROUND } from './Backdrop';
import { CameraRig } from './CameraRig';
import { CanvasErrorBoundary } from './CanvasErrorBoundary';
import { LightRig } from './LightRig';
import { PlateMesh } from './PlateMesh';
import { RenderDriver } from './RenderDriver';
import { useLightDrag } from './useLightDrag';

function Scene() {
  return (
    <>
      <color attach="background" args={[BACKGROUND]} />
      <PlateMesh />
      <LightRig />
      <Backdrop />
      <OrbitControls makeDefault enableDamping dampingFactor={0.12} />
      <CameraRig />
      <RenderDriver />
    </>
  );
}

function Hints() {
  const dragging = useAppStore((s) => s.view.lightDragging);
  const follow = useAppStore((s) => s.view.followDesignLight);
  const requestFit = useAppStore((s) => s.requestFit);
  const setView = useAppStore((s) => s.setView);
  return (
    <div className="hints">
      <span>{dragging ? 'Swinging the light…' : 'Drag: orbit · Right-drag: pan · Wheel: zoom · Alt+drag: move light · F: fit'}</span>
      {!follow && (
        <button type="button" className="chip" onClick={() => setView({ followDesignLight: true })}>
          Light back to design
        </button>
      )}
      <button type="button" className="chip" onClick={requestFit}>
        Fit
      </button>
    </div>
  );
}

export function Viewport() {
  const wrapRef = useRef<HTMLDivElement>(null);
  useLightDrag(wrapRef);
  return (
    <div ref={wrapRef} className="viewport">
      <CanvasErrorBoundary>
        <Canvas
          shadows="percentage"
          flat
          frameloop="demand"
          dpr={[1, 2]}
          gl={{ antialias: true, powerPreference: 'high-performance' }}
          camera={{ fov: 30, near: 1, far: 20000, position: [150, 189, 1500] }}
        >
          <Scene />
        </Canvas>
      </CanvasErrorBoundary>
      <StatsOverlay />
      <Hints />
    </div>
  );
}
