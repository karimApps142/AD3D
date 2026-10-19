import sys, math, numpy as np
sys.path.insert(0, sys.argv[1]); import squama as s
img = s.target_image()
def profile(t, cell, mode, hmax):
    nx = len(t); hl = np.zeros(nx); hr = np.zeros(nx); base = 3.4
    if mode == 'pivot':          # each facet pivots on its centre (independent steps)
        half = cell/2*np.tan(np.abs(t)); c = base + half
        return c - cell/2*np.tan(t), c + cell/2*np.tan(t)   # facing light (-x) => right edge higher
    h = base
    for i in range(nx):          # continuous: next facet starts where previous ended
        rise = cell*np.tan(t[i])
        if mode == 'greedy' and h + rise > base + hmax:
            h = base             # drop (casts a shadow on what follows)
        if mode == 'smart' and h + rise > base + hmax:
            # drop at the darkest boundary within the last few cells is better; simple version: drop now
            h = base
        hl[i] = h; hr[i] = h + rise; h = hr[i]
        h = max(h, base)
    m = min(hl.min(), hr.min());  return hl - m + base, hr - m + base
def run(alpha, S, mode, hmax=8.0, cell=6, X=300):
    th,nx,nz,X,Z = s.tilts(img, X, cell, alpha, S)
    g = np.asarray(img.convert('L').resize((nx,nz)), float)[::-1]/255
    a = math.radians(alpha); spc = 32; lit_b = np.zeros_like(th); maxh = 0
    for j in range(nz):
        t = th[j]; hl, hr = profile(t, cell, mode, hmax); maxh = max(maxh, hr.max(), hl.max())
        xs = (np.arange(nx*spc)+0.5)/spc*cell; ci = (xs//cell).astype(int); u = xs/cell - ci
        y = hl[ci] + (hr[ci]-hl[ci])*u
        horizon = np.maximum.accumulate(y + xs*math.tan(a)) - xs*math.tan(a)
        lit = (y >= horizon - 1e-6)
        lit_b[j] = (np.maximum(0, np.sin(a + t[ci]))*lit).reshape(nx, spc).mean(1)
    c1 = np.corrcoef(g.ravel(), lit_b.ravel())[0,1]
    print(f"alpha={alpha:>2} S={S:>2} {mode:7} hmax={hmax:>4}: corr with cast shadows={c1:.3f}  max relief={maxh:.1f}mm")
for al,S in ((5,10),(5,20),(10,10),(20,20)):
    run(al,S,'pivot'); run(al,S,'greedy',8.0); run(al,S,'greedy',1e9)
