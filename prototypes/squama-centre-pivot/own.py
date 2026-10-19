import sys, numpy as np
from collections import Counter
sys.path.insert(0, sys.argv[1]); import squama as s
th,nx,nz,X,Z = s.tilts(s.target_image(),300,6)
V,F = s.build_direct(th,nx,nz,6)
key={}; rid=[]
for p in V:
    k=tuple(np.round(p,6)); rid.append(key.setdefault(k,len(key)))
rid=np.array(rid); F2=rid[F]
F2=[f for f in F2 if len(set(f))==3]
P=np.array(list(key.keys()))
F2=[f for f in F2 if np.linalg.norm(np.cross(P[f[1]]-P[f[0]],P[f[2]]-P[f[0]]))>1e-9]
de=Counter()
for f in F2:
    for a,b in ((f[0],f[1]),(f[1],f[2]),(f[2],f[0])): de[(a,b)]+=1
bad=[(e,c) for e,c in de.items() if c!=1 or de.get((e[1],e[0]),0)!=1]
print('faces',len(F2),'bad directed',len(bad))
from collections import defaultdict
for e,c in bad[:8]: print(P[list(e)].round(3).tolist(),c,de.get((e[1],e[0]),0))
vol=sum(np.dot(P[f[0]],np.cross(P[f[1]],P[f[2]])) for f in F2)/6; print('vol',vol)
