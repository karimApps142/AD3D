"""Clean-room Squama-style prototype: image -> tilted-facet plate (STL).
Plate lies in the XZ plane (X width, Z height), front faces +Y. Light from the Left
at elevation alpha out of the plate plane. Each cell is a planar facet tilted about Z.
Two constructions: (A) direct watertight mesh with crossing-split walls, (B) manifold union of prisms.
"""
import sys, time, math
import numpy as np
from PIL import Image, ImageDraw
import manifold3d as m3d
import trimesh

def target_image(w=500, h=630):
    im = Image.new("L", (w, h), 0)
    d = ImageDraw.Draw(im)
    for x in range(w):
        d.line([(x, 0), (x, h // 3)], fill=int(255 * x / w))
    d.ellipse([100, 260, 400, 560], fill=230)
    d.rectangle([180, 360, 320, 460], fill=40)
    return im

def tilts(img, X=300, cell=6, alpha=5, S=10, gamma=1.0):
    nx = max(1, round(X / cell)); X = nx * cell
    nz = max(1, round(X * img.height / img.width / cell)); Z = nz * cell
    g = np.asarray(img.convert("L").resize((nx, nz), Image.BOX), dtype=float) / 255.0
    g = g[::-1]  # row 0 = bottom (z=0)
    g = g ** gamma
    a, s = math.radians(alpha), math.radians(S)
    emin, emax = max(0.0, math.sin(a - s)), math.sin(a + s)
    E = emin + g * (emax - emin)               # Lambert: E = sin(alpha + theta)
    th = np.arcsin(np.clip(E, 0, 1)) - a        # theta in [-S, S]
    th[g <= 0.0] = -s                           # pure black -> max tilt away
    return th, nx, nz, X, Z

def heights(th, cell, T=3.0, pad=0.4):
    # facet pivots about its cell centre; offset so the lowest corner sits pad above base
    half = cell / 2 * np.tan(np.abs(th))
    c = T + pad + half                          # centre height
    hl = c - (cell / 2) * np.tan(th)            # facet faces the light (-X) => right edge higher
    hr = c + (cell / 2) * np.tan(th)
    return hl, hr

def build_direct(th, nx, nz, cell, T=3.0):
    """Watertight mesh, no booleans. Facet height varies only with x (Left/Right light).
    Row walls can cross -> split at the crossing and insert it into both facet edges.
    At every 4-cell grid corner up to 4 heights meet on one vertical line -> insert them into
    every wall touching that line (T-junction removal). Walls are planar polygons -> earcut."""
    hl, hr = heights(th, cell, T)
    V, F = [], []
    def v(p): V.append(p); return len(V) - 1
    def h_at(i, j, t): return hl[j, i] + (hr[j, i] - hl[j, i]) * t
    extra = {}
    for j in range(nz - 1):
        for i in range(nx):
            d0 = hl[j, i] - hl[j + 1, i]; d1 = hr[j, i] - hr[j + 1, i]
            if d0 * d1 < 0:
                t = d0 / (d0 - d1)
                extra.setdefault((i, j, 'top'), []).append(t)
                extra.setdefault((i, j + 1, 'bot'), []).append(t)
    edge_pts = {}  # (i,j,side) -> list of (t, vid)
    for j in range(nz):
        for i in range(nx):
            x0, z0 = i * cell, j * cell
            ring = []
            for side, z, ts in (('bot', z0, [0.0] + sorted(extra.get((i, j, 'bot'), [])) + [1.0]),
                                ('top', z0 + cell, [1.0] + sorted(extra.get((i, j, 'top'), []), reverse=True) + [0.0])):
                pts = [(t, v((x0 + t * cell, h_at(i, j, t), z))) for t in ts]
                edge_pts[(i, j, side)] = pts
                ring += [p[1] for p in pts]
            cidx = v((x0 + cell / 2, (hl[j, i] + hr[j, i]) / 2, z0 + cell / 2))
            for k in range(len(ring)):
                a, b = ring[k], ring[(k + 1) % len(ring)]
                if a != b and V[a] != V[b]:
                    F.append((cidx, b, a))
    import mapbox_earcut as earcut
    def corner(I, J):  # vertex ids of all cell corners meeting at grid point (I,J)
        out = []
        for (ci, cj, side, k) in ((I - 1, J - 1, 'top', 0), (I, J - 1, 'top', -1), (I - 1, J, 'bot', -1), (I, J, 'bot', 0)):
            if 0 <= ci < nx and 0 <= cj < nz: out.append(edge_pts[(ci, cj, side)][k][1])
        return out
    def vjump(a, b, I, J):  # vertical segment a->b at corner (I,J), with T-junction heights inserted
        ya, yb = V[a][1], V[b][1]; lo, hi = min(ya, yb), max(ya, yb)
        mid = sorted({V[q][1]: q for q in corner(I, J) if lo < V[q][1] < hi}.items(), reverse=bool(ya > yb))
        return [q for _, q in mid]
    def emit(ring, uax):  # planar wall polygon -> earcut, keep ring winding
        r = []
        for q in ring:
            if not r or V[q] != V[r[-1]]: r.append(q)
        if len(r) > 1 and V[r[0]] == V[r[-1]]: r.pop()
        if len(r) < 3: return
        uv = np.array([[V[q][uax], V[q][1]] for q in r], float)
        area = np.sum(uv[:, 0] * np.roll(uv[:, 1], -1) - np.roll(uv[:, 0], -1) * uv[:, 1])
        if abs(area) < 1e-12: return
        for t in earcut.triangulate_float64(uv, np.array([len(r)], dtype=np.uint32)).reshape(-1, 3):
            a2 = np.cross(np.append(uv[t[1]] - uv[t[0]], 0), np.append(uv[t[2]] - uv[t[0]], 0))[2]
            F.append(tuple(r[k] for k in t) if a2 * area > 0 else (r[t[0]], r[t[2]], r[t[1]]))
    # walls between columns: plane x = const, ring r0 -> r1 -> (jump) -> l1 -> l0 -> (jump) -> r0
    for j in range(nz):
        for i in range(nx - 1):
            r0 = edge_pts[(i, j, 'bot')][-1][1]; r1 = edge_pts[(i, j, 'top')][0][1]
            l0 = edge_pts[(i + 1, j, 'bot')][0][1]; l1 = edge_pts[(i + 1, j, 'top')][-1][1]
            emit([r0, r1] + vjump(r1, l1, i + 1, j + 1) + [l1, l0] + vjump(l0, r0, i + 1, j), 2)
    # walls between rows: plane z = const, split at crossings; ring a0 -> b0 -> b1 -> a1
    for j in range(nz - 1):
        for i in range(nx):
            A = sorted(edge_pts[(i, j, 'top')]); B = sorted(edge_pts[(i, j + 1, 'bot')])
            for k in range(len(A) - 1):
                a0, a1, b0, b1 = A[k][1], A[k + 1][1], B[k][1], B[k + 1][1]
                j0 = vjump(a0, b0, i, j + 1) if k == 0 else []
                j1 = vjump(b1, a1, i + 1, j + 1) if k == len(A) - 2 else []
                emit([a0] + j0 + [b0, b1] + j1 + [a1], 0)
    # four side walls: each is a planar polygon (top polyline with vertical jumps + straight base),
    # triangulated with earcut, then oriented outward. Back = 2 triangles on the 4 base corners.
    import mapbox_earcut as earcut
    X, Z = nx * cell, nz * cell
    c00, cX0, cXZ, c0Z = v((0, 0, 0)), v((X, 0, 0)), v((X, 0, Z)), v((0, 0, Z))
    bot = [q for i in range(nx) for _, q in edge_pts[(i, 0, 'bot')]]
    rgt = [q for j in range(nz) for q in (edge_pts[(nx - 1, j, 'bot')][-1][1], edge_pts[(nx - 1, j, 'top')][0][1])]
    top = [q for i in reversed(range(nx)) for _, q in edge_pts[(i, nz - 1, 'top')]]
    lft = [q for j in reversed(range(nz)) for q in (edge_pts[(0, j, 'top')][-1][1], edge_pts[(0, j, 'bot')][0][1])]
    for poly, a_end, a_start, uax, outward in ((bot, cX0, c00, 0, (0, 0, -1)), (rgt, cXZ, cX0, 2, (1, 0, 0)),
                                               (top, c0Z, cXZ, 0, (0, 0, 1)), (lft, c00, c0Z, 2, (-1, 0, 0))):
        ring = []
        for q in poly + [a_end, a_start]:
            if not ring or V[q] != V[ring[-1]]: ring.append(q)
        if V[ring[0]] == V[ring[-1]]: ring.pop()
        uv = np.array([[V[q][uax], V[q][1]] for q in ring], float)
        tri = earcut.triangulate_float64(uv, np.array([len(ring)], dtype=np.uint32)).reshape(-1, 3)
        for t in tri:
            a, b, c = (ring[k] for k in t)
            n = np.cross(np.subtract(V[b], V[a]), np.subtract(V[c], V[a]))
            F.append((a, b, c) if np.dot(n, outward) > 0 else (a, c, b))
    F += [(c00, cX0, cXZ), (c00, cXZ, c0Z)]  # back (-Y); orientation checked by validator
    return np.array(V), np.array(F)

def build_union(th, nx, nz, cell, T=3.0):
    hl, hr = heights(th, cell, T)
    parts = [m3d.Manifold.cube([nx * cell, T + 0.01, nz * cell])]
    for j in range(nz):
        for i in range(nx):
            x0, z0 = i * cell, j * cell
            verts = np.array([[x0, 0, z0], [x0 + cell, 0, z0], [x0 + cell, 0, z0 + cell], [x0, 0, z0 + cell],
                              [x0, hl[j, i], z0], [x0 + cell, hr[j, i], z0], [x0 + cell, hr[j, i], z0 + cell], [x0, hl[j, i], z0 + cell]], dtype=np.float32)
            tris = np.array([[0, 1, 2], [0, 2, 3], [4, 6, 5], [4, 7, 6], [0, 4, 5], [0, 5, 1], [1, 5, 6], [1, 6, 2],
                             [2, 6, 7], [2, 7, 3], [3, 7, 4], [3, 4, 0]], dtype=np.uint32)
            parts.append(m3d.Manifold(m3d.Mesh(vert_properties=verts, tri_verts=tris)))
    return m3d.Manifold.batch_boolean(parts, m3d.OpType.Add)

if __name__ == "__main__":
    out = sys.argv[1]
    img = target_image()
    for cell in (6, 4, 2):
        th, nx, nz, X, Z = tilts(img, X=300 if cell == 6 else 200, cell=cell)
        t0 = time.time(); V, F = build_direct(th, nx, nz, cell); t1 = time.time()
        tm = trimesh.Trimesh(V, F, process=True)
        print(f"cell={cell} grid={nx}x{nz} size={X}x{Z} direct: {len(tm.faces)} tris {t1-t0:.2f}s "
              f"watertight={tm.is_watertight} winding={tm.is_winding_consistent} vol={tm.volume:.0f}")
        tm.export(f"{out}/squama_direct_c{cell}.stl")
        t0 = time.time(); M = build_union(th, nx, nz, cell); t1 = time.time()
        mm = M.to_mesh()
        print(f"   union: {len(mm.tri_verts)} tris {t1-t0:.2f}s status={M.status()} vol={M.volume():.0f}")
        # cast-shadow check: step height between columns vs shadow length at alpha
    hl, hr = heights(*tilts(img, 300, 6)[:1], 6)
    step = np.abs(hr[:, :-1] - hl[:, 1:])
    print(f"cell6 col-step mean={step.mean():.2f}mm max={step.max():.2f}mm; shadow len at 5deg = step/tan5 -> mean {step.mean()/math.tan(math.radians(5)):.1f}mm")
