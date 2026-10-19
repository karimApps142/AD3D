import sys, math, numpy as np
sys.path.insert(0, sys.argv[1]); import squama as s
img = s.target_image()
def run(alpha, S, cell=6, X=300, pivot='centre'):
    th,nx,nz,X,Z = s.tilts(img, X, cell, alpha, S)
    g = np.asarray(img.convert('L').resize((nx,nz)), float)[::-1]/255
    a = math.radians(alpha); spc = 24
    lit_b = np.zeros_like(th); lam_b = np.zeros_like(th)
    for j in range(nz):
        t = th[j]
        if pivot == 'centre': hl, hr = s.heights(th[j:j+1], cell); hl, hr = hl[0], hr[0]
        else:  # all facets share the same height at their right (down-light) edge
            hr = np.full(nx, 3.4); hl = hr + cell*np.tan(t)
            hl, hr = hl - min(0, hl.min()-3.4), hr - min(0, hl.min()-3.4)
        xs = (np.arange(nx*spc)+0.5)/spc*cell; ci = (xs//cell).astype(int); u = xs/cell - ci
        y = hl[ci] + (hr[ci]-hl[ci])*u
        horizon = np.maximum.accumulate(y + xs*math.tan(a)) - xs*math.tan(a)  # light from -x
        lit = (y >= horizon - 1e-9).astype(float)
        lam = np.maximum(0, np.sin(a + t[ci]))
        lam_b[j] = (lam).reshape(nx, spc).mean(1); lit_b[j] = (lam*lit).reshape(nx, spc).mean(1)
    c0 = np.corrcoef(g.ravel(), lam_b.ravel())[0,1]; c1 = np.corrcoef(g.ravel(), lit_b.ravel())[0,1]
    print(f"alpha={alpha:>2} S={S:>2} pivot={pivot:6}: corr(no cast shadow)={c0:.3f}  corr(with cast shadow)={c1:.3f}  mean lit frac={(lit_b.sum()/max(lam_b.sum(),1e-9)):.2f}")
for al,S in ((5,10),(5,20),(10,10),(20,20)):
    for pv in ('centre','edge'): run(al,S,pivot=pv)
