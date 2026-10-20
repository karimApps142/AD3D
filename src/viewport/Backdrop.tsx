// The floor the plate stands on: a dark matte surface that takes the plate's shadow, with a
// 10 mm / 100 mm grid for scale. Follows the model bounds before each frame.

import { Grid } from '@react-three/drei';
import { useFrame } from '@react-three/fiber';
import { useEffect, useMemo, useRef } from 'react';
import * as THREE from 'three';
import { viewportBus } from './viewportBus';

export const BACKGROUND = '#0e1014';
const FLOOR_COLOR = '#181b21';
/** Just below the plate's bottom face (y = 0) so the two never z-fight. */
const FLOOR_Y = -0.3;

export function Backdrop() {
  const floor = useMemo(() => {
    const mesh = new THREE.Mesh(
      new THREE.CircleGeometry(1, 96).rotateX(-Math.PI / 2),
      new THREE.MeshStandardMaterial({ color: FLOOR_COLOR, roughness: 1, metalness: 0 }),
    );
    mesh.receiveShadow = true;
    mesh.name = 'floor';
    mesh.raycast = () => {};
    return mesh;
  }, []);
  const grid = useRef<THREE.Mesh>(null);
  const seen = useRef(-1);

  useEffect(
    () => () => {
      floor.geometry.dispose();
      (floor.material as THREE.Material).dispose();
    },
    [floor],
  );

  useFrame(() => {
    if (seen.current === viewportBus.modelBoxVersion) return;
    seen.current = viewportBus.modelBoxVersion;
    const box = viewportBus.modelBox;
    const cx = (box.min.x + box.max.x) / 2;
    const cz = (box.min.z + box.max.z) / 2;
    const span = Math.max(box.max.x - box.min.x, box.max.y - box.min.y, 50);
    floor.position.set(cx, FLOOR_Y, cz);
    floor.scale.setScalar(span * 6);
    grid.current?.position.set(cx, FLOOR_Y + 0.1, cz);
  }, 0);

  return (
    <>
      <primitive object={floor} />
      <Grid
        ref={grid}
        args={[1, 1]}
        cellSize={10}
        cellThickness={0.6}
        cellColor="#262b34"
        sectionSize={100}
        sectionThickness={1}
        sectionColor="#363e4b"
        fadeDistance={2400}
        fadeStrength={1.6}
        infiniteGrid
      />
    </>
  );
}
