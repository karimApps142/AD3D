import Module from 'manifold-3d';
const wasm = await Module(); wasm.setup();
const {Manifold, CrossSection} = wasm;
const t=()=>performance.now();
function rep(name,f){const a=t();const r=f();console.log(name,(t()-a).toFixed(0)+'ms',r??'');return r;}
for (const seg of [128,256,512,1024]) {
  const A=Manifold.sphere(50,seg), B=Manifold.sphere(50,seg).translate([30,10,5]);
  const n=A.numTri();
  rep(`seg${seg} tris=${n} diff`,()=>{const c=A.subtract(B);return c.numTri()});
  rep(`seg${seg} slice x40`,()=>{let k=0;for(let z=-49;z<49;z+=2.5){k+=A.slice(z).numVert()}return k});
  rep(`seg${seg} splitByPlane`,()=>{const [p,q]=A.splitByPlane([1,0.3,0.2],3);return p.numTri()+q.numTri()});
}
for (const [nx,nz] of [[50,63],[100,126],[150,189]]) {
  rep(`squama-like union ${nx}x${nz}`,()=>{const cell=4,parts=[Manifold.cube([nx*cell,3,nz*cell])];
   for(let i=0;i<nx;i++)for(let j=0;j<nz;j++){const th=(Math.sin(i*0.3+j*0.2))*10*Math.PI/180;
     const p=Manifold.cube([cell,1.5,cell]).translate([i*cell,2.5,j*cell]);
     // approximate tilt by warping y about cell centre x
     const cx=i*cell+cell/2;
     parts.push(p.warp(v=>{ if(v[1]>3) v[1]+= (v[0]-cx)*Math.tan(th); }));}
   const m=Manifold.union(parts);return m.numTri()+' status '+m.status();});
}
