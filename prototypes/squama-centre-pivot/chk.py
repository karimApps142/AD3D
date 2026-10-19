import sys, numpy as np, trimesh
sys.path.insert(0, sys.argv[1]); import squama as s
th,nx,nz,X,Z = s.tilts(s.target_image(),300,6)
V,F = s.build_direct(th,nx,nz,6)
tm = trimesh.Trimesh(V,F,process=True); tm.update_faces(tm.nondegenerate_faces()); tm.remove_unreferenced_vertices(); print("vol",tm.volume,"winding",tm.is_winding_consistent)
print('watertight',tm.is_watertight)
from collections import Counter
e = Counter(tuple(sorted(x)) for x in tm.edges)
bad=[k for k,c in e.items() if c!=2]; print('bad edges',len(bad))
for k in bad[:6]: print(tm.vertices[list(k)].round(3).tolist(), e[k])
