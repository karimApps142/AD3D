// The 3D view: r3f canvas with shadows, on-demand rendering and the overlays drawn on top of it.

import { OrbitControls } from '@react-three/drei';
import { Canvas } from '@react-three/fiber';
import { useRef } from 'react';
import * as THREE from 'three';
import { StatsOverlay } from '../panels/StatsOverlay';
import { useAppStore } from '../state/store';
import { Backdrop, BACKGROUND } from './Backdrop';
import { CameraRig, FrontCamera } from './CameraRig';
import { CanvasErrorBoundary } from './CanvasErrorBoundary';
import { LightRig } from './LightRig';
import { PlateMesh } from './PlateMesh';
import { RenderDriver } from './RenderDriver';
import { TileOverlay } from './TileOverlay';
import { useLightDrag } from './useLightDrag';
import { exposeViewportHooks } from './viewportHooks';
import { useViewportStore } from './viewportStore';
import { ViewportToolbar } from './ViewportToolbar';

exposeViewportHooks();

const ORBIT_BUTTONS = { LEFT: THREE.MOUSE.ROTATE, MIDDLE: THREE.MOUSE.DOLLY, RIGHT: THREE.MOUSE.PAN };
/** The front viewer never rotates: left-drag pans instead. */
const FRONT_BUTTONS = { LEFT: THREE.MOUSE.PAN, MIDDLE: THREE.MOUSE.DOLLY, RIGHT: THREE.MOUSE.PAN };

function Scene() {
  const front = useViewportStore((s) => s.frontView);
  return (
    <>
      <color attach="background" args={[BACKGROUND]} />
      <PlateMesh />
      <LightRig />
      <Backdrop />
      <TileOverlay />
      <FrontCamera />
      <OrbitControls
        makeDefault
        enableDamping
        dampingFactor={0.12}
        enableRotate={!front}
        mouseButtons={front ? FRONT_BUTTONS : ORBIT_BUTTONS}
      />
      <CameraRig />
      <RenderDriver />
    </>
  );
}

function Hints() {
  const dragging = useAppStore((s) => s.view.lightDragging);
  const follow = useAppStore((s) => s.view.followDesignLight);
  const front = useViewportStore((s) => s.frontView);
  const requestFit = useAppStore((s) => s.requestFit);
  const setView = useAppStore((s) => s.setView);
  return (
    <div className="hints">
      <span>{dragging ? 'Swinging the light…' : front
            ? 'Front view · Drag: pan · Wheel: zoom · Alt+drag: move light · F: fit'
            : 'Drag: orbit · Right-drag: pan · Wheel: zoom · Alt+drag: move light · Alt+wheel: lamp distance · F: fit'}</span>
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
      <ViewportToolbar />
      <StatsOverlay />
      <Hints />
    </div>
  );
}
