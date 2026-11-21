/* Ground dimensions are metres. All surface layers share one world-space edge profile. */
(() => {
  const $ = id => document.getElementById(id);
  const sites = {
    street: {name:'Street curb', notes:'A raised curb catches road grit in its gutter. Expansion joints divide the sidewalk into slabs, with grass creeping over the outer lip.', layers:['Road asphalt','Cast concrete','Turf & topsoil']},
    verge: {name:'Roadside verge', notes:'The asphalt breaks into loose aggregate along an unbound shoulder. Exposed soil threads between stones and the first clumps of grass.', layers:['Road asphalt','Gravel shoulder','Turf & topsoil']},
    runway: {name:'Landing strip', notes:'Threshold paint wears through to runway aggregate. A paved shoulder meets a narrow erosion band before the closely cut airfield grass.', layers:['Runway asphalt','Paved shoulder','Mown grass']}
  };
  const swatches = ['#424447','#aaa28e','#697346'];
  let site = 'street', wear = .72, clean = false, topView = false;
  const groundColor = 0xe1e4d9;
  const canvas = document.querySelector('canvas');
  let renderer;
  try { renderer = new THREE.WebGLRenderer({canvas,antialias:true}); }
  catch { $('error').hidden=false; $('error').textContent='Enable WebGL to inspect the 3D surface.'; return; }
  renderer.setPixelRatio(Math.min(devicePixelRatio,2));
  renderer.shadowMap.enabled=true;
  renderer.shadowMap.type=THREE.PCFSoftShadowMap;
  const scene=new THREE.Scene();
  scene.background=new THREE.Color(groundColor);
  const camera=new THREE.OrthographicCamera(-6,6,5,-5,.05,100);
  const orbit=new THREE.OrbitControls(camera,canvas);
  orbit.enablePan=false; orbit.minZoom=.7; orbit.maxZoom=4; orbit.maxPolarAngle=Math.PI*.47;
  scene.add(new THREE.HemisphereLight(0xfff5dd,0xb4b19d,2.25));
  const sun=new THREE.DirectionalLight(0xffedcc,2.5);
  sun.position.set(-4,8,3); sun.castShadow=true;
  Object.assign(sun.shadow.camera,{left:-7,right:7,top:7,bottom:-7});
  sun.shadow.mapSize.set(2048,2048); sun.shadow.normalBias=.009; scene.add(sun);
  const floor=new THREE.Mesh(new THREE.PlaneGeometry(200,200),new THREE.ShadowMaterial({opacity:.16}));
  floor.rotation.x=-Math.PI/2; floor.position.y=-.66; floor.receiveShadow=true; scene.add(floor);
  let sample=new THREE.Group(); scene.add(sample);
  const draw=()=>renderer.render(scene,camera);
  orbit.addEventListener('change',draw);
  const hash=(x,z)=>{let v=Math.imul(x,374761393)+Math.imul(z,668265263); v=Math.imul(v^(v>>>13),1274126177); return ((v^(v>>>16))>>>0)/4294967296;};
  const mix=(a,b,t)=>a+(b-a)*t;
  function noise(x,z) {
    const i=Math.floor(x), j=Math.floor(z);
    let u=x-i,v=z-j; u*=u*(3-2*u); v*=v*(3-2*v);
    return mix(mix(hash(i,j),hash(i+1,j),u),mix(hash(i,j+1),hash(i+1,j+1),u),v);
  }
  function edges(z,d) {
    const coarse=noise(z*2,5)-.5, chips=noise(z*14,8)-.5;
    const first=site==='street'?-.65:site==='verge'?-.25:1.05;
    const last=site==='street'?1.45:site==='verge'?.73:1.8;
    return [first+d*(coarse*.25+chips*.12)*(site==='street'?.22:1),last+d*(coarse*.58+chips*.13)];
  }
  function texture(canvasElement) {
    const t=new THREE.CanvasTexture(canvasElement);
    t.colorSpace=THREE.SRGBColorSpace; t.anisotropy=Math.min(8,renderer.capabilities.getMaxAnisotropy()); return t;
  }
  function makeTexture(d) {
    const c=document.createElement('canvas'); c.width=1280;c.height=960;
    const ctx=c.getContext('2d'), pixels=ctx.createImageData(c.width,c.height);
    const asphalt=[53,54,53], cement=[170,164,146], gravel=[133,119,93], grass=[86,97,51], soil=[110,91,64], shoulder=[76,75,69];
    for(let j=0;j<c.height;j++){
      const z=j/c.height*6-3,[a,b]=edges(z,d);
      for(let i=0;i<c.width;i++){
        const x=i/c.width*8-4,k=(j*c.width+i)*4,grain=hash(i,j),macro=noise(x*2,z*2), fleck=noise(x*45,z*45);
        const gutter=Math.max(0,1-Math.abs(x-a)/.26), verge=Math.max(0,1-Math.abs(x-b)/.38);
        let col, shade=(grain-.5)*25+(macro-.5)*22+(fleck-.5)*16;
        if(x<a) {
          col=asphalt;
          shade+=d*(grain>.97?35:0);
          if(gutter*d*(.5+macro)>grain*2) col=soil;
          if(site==='runway' && x<-.25 && x>-3.65 && Math.cos(z*3.9)>-.2 && grain>d*(.1+.27*macro)) col=[195,189,166];
          if(site==='street') {
            const drain=Math.hypot((x+.99)*1.4,z-.62);
            shade-=d*Math.max(0,1-drain/.85)*(12+macro*18);
          }
          if(site==='verge' && Math.abs(x+1.25)<.055 && grain>d*.23) col=[195,185,151];
          const fissure=Math.abs(x+2+.34*Math.sin(z*1.5)+.35*(noise(z*5,9)-.5));
          if(fissure<.013*d && macro>.25) shade-=28;
        } else if(x<b) {
          col=site==='street'?cement:site==='verge'?gravel:shoulder;
          if(site==='street') {
            const curb=x<a+.22, spacing=curb?1.15:1.55;
            const joint=Math.abs((z+3.2+(curb?.42:0))/spacing-Math.round((z+3.2+(curb?.42:0))/spacing))*spacing;
            if(joint<.013+d*.008*fleck || Math.abs(x-a-.22)<.012) shade-=53;
            shade+=(curb?8:0)+Math.sin(j*1.8)*3;
          } else if(site==='verge') shade+=(fleck-.5)*42;
        } else {col=grass; shade+=(fleck-.5)*32;}
        if(verge*d*(.65+macro)>.20+fleck*.67) col=soil;
        for(let n=0;n<3;n++) pixels.data[k+n]=col[n]+shade;
        pixels.data[k+3]=255;
      }
    }
    ctx.putImageData(pixels,0,0);return texture(c);
  }
  function mesh(geometry,material) {
    const m=new THREE.Mesh(geometry,material); m.receiveShadow=true;m.castShadow=true;sample.add(m);return m;
  }
  function geometry(positions,uvs,indices) {
    const g=new THREE.BufferGeometry();
    g.setAttribute('position',new THREE.Float32BufferAttribute(positions,3));
    if(uvs) g.setAttribute('uv',new THREE.Float32BufferAttribute(uvs,2));
    if(indices) g.setIndex(indices);g.computeVertexNormals();return g;
  }
  function surfaces(d,map) {
    const mat=new THREE.MeshStandardMaterial({map,roughness:.94,side:THREE.DoubleSide});
    const positions=[],uvs=[],indices=[];
    for(let j=0;j<=180;j++){
      const z=j/30-3,[a,b]=edges(z,d),h=site==='street'?.145:.012;
      const chip=d*Math.pow(noise(z*11,2),4)*.06;
      const row=[[-4,0],[a-.09,-.016],[a,0],[a+.012,h-.027-chip],[a+.035,h-chip],[a+.22,h],[b-.025,h],[b,h-.022],[b+.06,h-.034],[4,h-.025]];
      row.forEach(([x,y],i)=>{
        positions.push(x,y,z);uvs.push((x+4)/8,1-(z+3)/6);
        const k=j*row.length+i;
        if(j&&i) indices.push(k-11,k-1,k,k-11,k,k-10);
      });
    }
    mesh(geometry(positions,uvs,indices),mat);
    // The exposed cut faces follow the same elevations as the upper surface.
    const sideCanvas=document.createElement('canvas');sideCanvas.width=1024;sideCanvas.height=100;
    const ctx=sideCanvas.getContext('2d'),img=ctx.createImageData(1024,100);
    for(let y=0;y<100;y++) for(let x=0;x<1024;x++){
      const seam=noise(x*.025,1)*8,grain=hash(x,y),rock=noise(x*.35,y*.35);
      const base=y<9+seam?[67,61,48]:y<39+seam?[117,99,72]:[88,68,49];
      const k=(y*1024+x)*4, shade=(rock-.5)*32+(grain-.5)*28+(grain>.965?28:0);
      for(let n=0;n<3;n++) img.data[k+n]=base[n]+shade;img.data[k+3]=255;
    }
    ctx.putImageData(img,0,0);
    const wall=new THREE.MeshStandardMaterial({map:texture(sideCanvas),side:THREE.DoubleSide});
    const sides=[Array.from({length:10},(_,i)=>i),Array.from({length:10},(_,i)=>1800+i),Array.from({length:181},(_,i)=>10*i),Array.from({length:181},(_,i)=>10*i+9)];
    for(const side of sides){
      const p=[],uv=[],ix=[];
      side.forEach((idx,i)=>{
        const [x,y,z]=positions.slice(idx*3,idx*3+3);
        p.push(x,-.65,z,x,y,z);const u=side.length===10?(x+4)/8:(z+3)/6;
        uv.push(u,0,u,1);if(i){const k=i*2;ix.push(k-2,k-1,k,k,k-1,k+1);}
      });
      mesh(geometry(p,uv,ix),wall);
    }
  }
  function drain() {
    const iron=new THREE.MeshStandardMaterial({color:0x48453f,roughness:.83,metalness:.43});
    const opening=new THREE.MeshStandardMaterial({color:0x242420,roughness:1});
    function part(w,h,l,x,y,z,material) {
      const item=mesh(new THREE.BoxGeometry(w,h,l),material);
      item.position.set(-.99+x,y,.62+z);
    }
    part(.45,.009,.79,0,.006,0,opening);
    for(const side of [-1,1]) {
      part(.028,.018,.79,side*.212,.014,0,iron);
      part(.40,.018,.032,0,.014,side*.38,iron);
    }
    for(let i=-4;i<=4;i++) part(.4,.018,.027,0,.013,i*.079,iron);
    part(.018,.012,.73,0,.011,0,iron);
  }
  function dressing(d) {
    const blades=[],colors=[],stoneLocations=[];
    let seed=4721;const rand=()=>((seed=(Math.imul(seed,1664525)+1013904223)>>>0)/4294967296);
    const green=new THREE.Color(),tip=new THREE.Color();
    for(let i=0;i<6500;i++){
      const x=-4+rand()*8,z=-3+rand()*6,[a,b]=edges(z,d), h=site==='street'?.145:.012;
      const dirt=x-b;
      if(d && ((Math.abs(x-a)<(site==='verge'?.55:.24) && rand()<d*.7) || (Math.abs(dirt)<.27 && rand()<d*.54))){
        const onCurb=site==='street'&&x>a;
        stoneLocations.push([x,onCurb?h:x>b?h-.025:.002,z,rand(),rand()]); 
      }
      if(dirt<-.12*d || x>3.97 || Math.abs(z)>2.96 || (dirt<0 && rand()>.28*d))continue;
      const patch=noise(x*3,z*3),length=(.04+patch*.085+rand()*.055)*(site==='runway'?.55:1);
      green.setHSL(.17+rand()*.045,.21+rand()*.19,.17+rand()*.09);
      tip.copy(green).offsetHSL(-.02,.04,.12+rand()*.1);
      for(let k=0;k<8;k++){
        const angle=rand()*Math.PI*2,w=.007+rand()*.009, dx=Math.cos(angle),dz=Math.sin(angle);
        const bx=x+dx*.022,bz=z+dz*.022,by=h+(dirt<0?0:-.031);
        blades.push(bx-dz*w,by,bz+dx*w,bx+dz*w,by,bz-dx*w,bx+dx*length*.68,by+length*(.45+rand()*.65),bz+dz*length*.68);
        colors.push(green.r,green.g,green.b,green.r,green.g,green.b,tip.r,tip.g,tip.b);
      }
    }
    const turf=geometry(blades);turf.setAttribute('color',new THREE.Float32BufferAttribute(colors,3));
    mesh(turf,new THREE.MeshStandardMaterial({vertexColors:true,side:THREE.DoubleSide}));
    if(stoneLocations.length){
      const stones=new THREE.InstancedMesh(new THREE.IcosahedronGeometry(1,0),new THREE.MeshStandardMaterial(),stoneLocations.length);
      const pose=new THREE.Object3D(),stone=new THREE.Color();
      stoneLocations.forEach(([x,y,z,r,t],i)=>{
        const s=.009+r*.029;pose.position.set(x,y+.004,z);pose.rotation.set(r,t*5,0);pose.scale.set(s*1.4,s*.55,s);pose.updateMatrix();
        stones.setMatrixAt(i,pose.matrix);stone.setHSL(.10,.12,.20+.24*t);stones.setColorAt(i,stone);
      });
      stones.castShadow=true;stones.receiveShadow=true;sample.add(stones);
    }
  }
  function build(){
    scene.remove(sample);
    const materials=new Set();sample.traverse(m=>{m.geometry?.dispose();if(m.material)materials.add(m.material);if(m.isInstancedMesh)m.dispose();});
    for(const m of materials){m.map?.dispose();m.dispose();}
    sample=new THREE.Group();scene.add(sample);
    const d=clean?0:wear;
    surfaces(d,makeTexture(d));dressing(d);if(site==='street')drain();draw();
  }
  function frame(){
    const w=canvas.clientWidth,h=canvas.clientHeight,aspect=w/h;
    const half=Math.max(3.85,5.45/aspect);
    Object.assign(camera,{left:-half*aspect,right:half*aspect,top:half,bottom:-half});
    camera.updateProjectionMatrix();renderer.setSize(w,h,false);draw();
  }
  function setCamera(top){
    topView=top;camera.position.set(...(top?[0,11,.001]:[-7.8,7.6,10.5]));
    orbit.target.set(0,-.18,0);camera.zoom=1;camera.updateProjectionMatrix();orbit.update();
    $('view').textContent=top?'Oblique view':'Top view';draw();
  }
  function selectSite(id){
    site=id;const data=sites[id];$('name').textContent=data.name;$('notes').textContent=data.notes;
    $('layers').replaceChildren(...data.layers.map((label,i)=>{
      const row=document.createElement('div'),chip=document.createElement('span');
      row.className='layer';chip.className='swatch';chip.style.background=site==='runway'&&i===1?'#555550':swatches[i];row.append(chip,label);return row;
    }));
    document.querySelectorAll('[data-site]').forEach(b=>b.setAttribute('aria-pressed',String(b.dataset.site===site)));
    build();
  }
  document.querySelectorAll('[data-site]').forEach(b=>b.onclick=()=>selectSite(b.dataset.site));
  $('wear').oninput=e=>{$('amount').textContent=e.target.value+'%';};
  $('wear').onchange=e=>{wear=Number(e.target.value)/100;build();};
  $('clean').onchange=e=>{clean=e.target.checked;$('wear').disabled=clean;build();};
  $('reset').onclick=()=>setCamera(false);$('view').onclick=()=>setCamera(!topView);
  new ResizeObserver(frame).observe(canvas);setCamera(false);selectSite(site);frame();
})();
