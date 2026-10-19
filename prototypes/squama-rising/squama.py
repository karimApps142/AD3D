"""Clean-room Squama-style prototype (reconstructed from public video behaviour only).

Plate lies in the X-Z plane, facets face +Y. Light grazes from -X (Left) at elevation
`light_deg` above the plate. Each cell is a planar facet tilted about Z by theta in
[-surf_deg, +surf_deg]. Lambert: brightness = max(0, sin(light + theta)).
Two meshers: (a) union of per-cell convex prisms via manifold3d, (b) direct mesher
with vertical-line splitting (watertight, no booleans).
"""
import sys, time, math
import numpy as np
from PIL import Image, ImageDraw


def target_image(path=None, w=200, h=252):
    if path:
        return np.asarray(Image.open(path).convert("L"), dtype=np.float64) / 255.0
    img = Image.new("L", (w, h), 0)
    d = ImageDraw.Draw(img)
    yy, xx = np.mgrid[0:h, 0:w]
    grad = (xx / w) * 255
    img = Image.fromarray(grad.astype(np.uint8))
    d = ImageDraw.Draw(img)
    d.ellipse([40, 60, 160, 180], fill=255)
    d.rectangle([80, 100, 120, 140], fill=0)
    return np.asarray(img, dtype=np.float64) / 255.0


def cell_angles(img, size_x, cell, light_deg, surf_deg):
    nx = max(1, round(size_x / cell))
    nz = max(1, round(nx * img.shape[0] / img.shape[1]))
    # area-average image into nz x nx cells
    pil = Image.fromarray((img * 255).astype(np.uint8)).resize((nx, nz), Image.BOX)
    g = np.asarray(pil, dtype=np.float64) / 255.0
    g = np.flipud(g)  # row 0 = bottom (z=0)
    a, s = math.radians(light_deg), math.radians(surf_deg)
    bmin, bmax = max(0.0, math.sin(a - s)), math.sin(a + s)
    b = bmin + g * (bmax - bmin)
    theta = np.arcsin(np.clip(b, 0, 1)) - a
    theta = np.clip(theta, -s, s)
    return theta, nx, nz, g


def heights(theta, cell, thick):
    """Facet surface y(x_local) = yc + tan(theta)*(x_local), x_local in [-c/2, c/2].
    Choose yc so the lowest corner is exactly `thick`."""
    t = np.tan(theta)
    yc = thick + 0.5 * cell * np.abs(t)
    # break exact height ties: identical facets meeting diagonally at a grid vertex create a
    # pinch (4 faces on one edge = non-manifold). 1e-4 mm is far below printer resolution.
    yc = yc + 1e-4 * np.random.default_rng(0).random(yc.shape)
    yl = yc - 0.5 * cell * t  # height at left edge of cell
    yr = yc + 0.5 * cell * t  # height at right edge
    return yl, yr


def mesh_manifold(theta, nx, nz, cell, thick):
    import manifold3d as m3
    yl, yr = heights(theta, cell, thick)
    parts = []
    for j in range(nz):
        for i in range(nx):
            x0, x1, z0, z1 = i * cell, (i + 1) * cell, j * cell, (j + 1) * cell
            pts = [(x0, 0, z0), (x1, 0, z0), (x0, 0, z1), (x1, 0, z1),
                   (x0, yl[j, i], z0), (x1, yr[j, i], z0), (x0, yl[j, i], z1), (x1, yr[j, i], z1)]
            parts.append(m3.Manifold.hull_points(pts))
    man = m3.Manifold.batch_boolean(parts, m3.OpType.Add)
    mesh = man.to_mesh()
    return np.asarray(mesh.vert_properties)[:, :3], np.asarray(mesh.tri_verts)


def mesh_direct(theta, nx, nz, cell, thick):
    """Direct watertight mesher. Light from Left/Right => heights constant along Z within a
    cell, so: tops are quads; walls on X-boundaries (x=const) are rectangles; walls on
    Z-boundaries (z=const) are between two sloped segments and may cross (bow-tie split).
    T-junctions on vertical lines at grid vertices are avoided by emitting every wall/side
    as a polygon whose vertical edges carry all heights present on that vertical line."""
    yl, yr = heights(theta, cell, thick)
    V, F, vid = [], [], {}

    def v(x, y, z):
        k = (round(x, 9), round(y, 9), round(z, 9))
        if k not in vid:
            vid[k] = len(V); V.append(k)
        return vid[k]

    def H(i, j, side):  # height of cell (i,j) at its left(0)/right(1) edge; outside = 0
        if i < 0 or i >= nx or j < 0 or j >= nz:
            return 0.0
        return yl[j, i] if side == 0 else yr[j, i]

    # vertical line at grid vertex (i,j): all heights of the <=4 surrounding cells + 0
    def line_heights(i, j):
        hs = {0.0, H(i - 1, j - 1, 1), H(i, j - 1, 0), H(i - 1, j, 1), H(i, j, 0)}
        return sorted(hs)

    def vert_chain(i, j, ya, yb):  # vertex ids along line (i,j) from ya to yb inclusive
        hs = [h for h in line_heights(i, j) if min(ya, yb) - 1e-12 <= h <= max(ya, yb) + 1e-12]
        if ya > yb:
            hs = hs[::-1]
        return [v(i * cell, h, j * cell) for h in hs]

    def fan(poly):  # poly is planar & convex in all our cases
        for k in range(1, len(poly) - 1):
            if len({poly[0], poly[k], poly[k + 1]}) == 3:
                F.append((poly[0], poly[k], poly[k + 1]))

    # crossing points on z-boundaries (where below/above facet edges intersect) must also
    # be vertices of the top facets, otherwise they are T-junctions -> not watertight.
    cross = {}
    for j in range(1, nz):
        for i in range(nx):
            d0 = yl[j, i] - yl[j - 1, i]; d1 = yr[j, i] - yr[j - 1, i]
            if d0 * d1 < 0:
                t = d0 / (d0 - d1)
                cross[(i, j)] = (i * cell + t * cell, yl[j - 1, i] + t * (yr[j - 1, i] - yl[j - 1, i]))
    # tops (+Y facing): polygon a,[xb],b,c,[xt],d fanned from an interior centre vertex
    for j in range(nz):
        for i in range(nx):
            a = v(i * cell, yl[j, i], j * cell); b = v((i + 1) * cell, yr[j, i], j * cell)
            c = v((i + 1) * cell, yr[j, i], (j + 1) * cell); d = v(i * cell, yl[j, i], (j + 1) * cell)
            if (i, j) not in cross and (i, j + 1) not in cross:
                F.append((a, d, c)); F.append((a, c, b)); continue
            poly = [a]
            if (i, j) in cross:
                xc, yc = cross[(i, j)]; poly.append(v(xc, yc, j * cell))
            poly += [b, c]
            if (i, j + 1) in cross:
                xc, yc = cross[(i, j + 1)]; poly.append(v(xc, yc, (j + 1) * cell))
            poly.append(d)
            ctr = v((i + 0.5) * cell, 0.5 * (yl[j, i] + yr[j, i]), (j + 0.5) * cell)
            for k in range(len(poly)):  # poly is CCW seen from -Y; reverse for +Y normal
                F.append((ctr, poly[(k + 1) % len(poly)], poly[k]))
    # bottom plate (y=0), split at every grid vertex to match side walls
    for j in range(nz):
        for i in range(nx):
            a = v(i * cell, 0, j * cell); b = v((i + 1) * cell, 0, j * cell)
            c = v((i + 1) * cell, 0, (j + 1) * cell); d = v(i * cell, 0, (j + 1) * cell)
            F.append((a, b, c)); F.append((a, c, d))
    # walls on x = i*cell between cell (i-1,j) [left] and (i,j) [right]; constant along z
    for j in range(nz):
        for i in range(nx + 1):
            hL, hR = H(i - 1, j, 1), H(i, j, 0)
            if abs(hL - hR) < 1e-12:
                continue
            # polygon in plane x=const: along z=j edge from hR->hL, z=j+1 edge hL->hR
            lo_chain = vert_chain(i, j, hR, hL)
            hi_chain = vert_chain(i, j + 1, hL, hR)
            poly = lo_chain + hi_chain
            # orient: normal should point toward lower side
            fan(poly)  # chain order hR->hL->hR already yields the outward normal
    # walls on z = j*cell between cell (i,j-1) [below] and (i,j) [above]; sloped, may cross
    for j in range(nz + 1):
        for i in range(nx):
            b0, b1 = H(i, j - 1, 0), H(i, j - 1, 1)  # below cell at x0, x1
            a0, a1 = H(i, j, 0), H(i, j, 1)          # above cell
            d0, d1 = a0 - b0, a1 - b1
            x0, x1, z = i * cell, (i + 1) * cell, j * cell
            def piece(xs, xe, bs, be, as_, ae, is_, ie):
                # trapezoid between below-segment and above-segment, from xs to xe
                ds, de = as_ - bs, ae - be
                if abs(ds) < 1e-12 and abs(de) < 1e-12:
                    return
                sgn = ds + de
                left = vert_chain(i, j, bs, as_) if is_ else ([v(xs, bs, z)] if abs(ds) < 1e-12 else [v(xs, bs, z), v(xs, as_, z)])
                right = vert_chain(i + 1, j, ae, be) if ie else ([v(xe, ae, z)] if abs(de) < 1e-12 else [v(xe, ae, z), v(xe, be, z)])
                poly = left + right  # bs..as at xs, ae..be at xe : loop
                fan(poly)  # bs->as->ae->be order already yields the outward normal
            if d0 * d1 < 0:  # crossing: split at xc
                t = d0 / (d0 - d1); xc = x0 + t * (x1 - x0); yc = b0 + t * (b1 - b0)
                piece(x0, xc, b0, yc, a0, yc, True, False)
                piece(xc, x1, yc, b1, yc, a1, False, True)
            else:
                piece(x0, x1, b0, b1, a0, a1, True, True)
    return np.array(V, dtype=np.float64), np.array(F, dtype=np.int64)


def main():
    img_path = sys.argv[1] if len(sys.argv) > 1 else None
    size_x, cell, light, surf, thick = 200.0, 4.0, 5.0, 10.0, 3.0
    img = target_image(img_path)
    theta, nx, nz, g = cell_angles(img, size_x, cell, light, surf)
    print(f"grid {nx}x{nz}, size {nx*cell}x{nz*cell} mm, theta range "
          f"[{math.degrees(theta.min()):.2f},{math.degrees(theta.max()):.2f}] deg")
    import trimesh
    out = sys.argv[2] if len(sys.argv) > 2 else "."
    for name, fn in (("manifold", mesh_manifold), ("direct", mesh_direct)):
        t0 = time.time()
        Vt, Ft = fn(theta, nx, nz, cell, thick)
        dt = time.time() - t0
        m = trimesh.Trimesh(Vt, Ft, process=True)
        print(f"[{name}] {dt:.2f}s verts={len(m.vertices)} faces={len(m.faces)} "
              f"watertight={m.is_watertight} winding_ok={m.is_winding_consistent} "
              f"volume={m.volume:.1f} bounds={np.round(m.bounds,2).tolist()}")
        m.export(f"{out}/squama_{name}.stl")
    # shading check: re-derive brightness from top-face normals of the direct mesh
    a = math.radians(light)
    L = np.array([-math.cos(a), math.sin(a), 0.0])
    c = m.triangles_center; n = m.face_normals
    top = n[:, 1] > 0.5
    ii = np.clip((c[top, 0] // cell).astype(int), 0, nx - 1)
    jj = np.clip((c[top, 2] // cell).astype(int), 0, nz - 1)
    shade = np.zeros((nz, nx)); shade[jj, ii] = np.clip(n[top] @ L, 0, None)
    r = np.corrcoef(shade.ravel(), g.ravel())[0, 1]
    print(f"Lambert shade vs target correlation r={r:.4f}")
    rend = (shade - shade.min()) / (np.ptp(shade) + 1e-12)
    Image.fromarray((np.flipud(rend) * 255).astype(np.uint8)).resize((nx * 4, nz * 4), Image.NEAREST).save(f"{out}/squama_render.png")
    Image.fromarray((np.flipud(g) * 255).astype(np.uint8)).resize((nx * 4, nz * 4), Image.NEAREST).save(f"{out}/squama_target.png")


if __name__ == "__main__":
    main()
