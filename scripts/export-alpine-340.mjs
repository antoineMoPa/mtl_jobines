// THREE_ROOT=/path/to/node_modules/three PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs node scripts/export-alpine-340.mjs
// Exports the existing configurator with explicit game articulation roles.
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const three = process.env.THREE_ROOT ?? resolve('node_modules/three');
const folder = resolve('data/vehicle/winter/alpine-340');
const browser = await chromium.launch({headless:true, args:['--enable-unsafe-swiftshader']});
try {
  const page = await browser.newPage();
  await page.route('https://cdn.jsdelivr.net/npm/three@0.160.0/**', route => {
    const path = new URL(route.request().url()).pathname.split('three@0.160.0/')[1];
    return route.fulfill({body:readFileSync(resolve(three, path)), contentType:'application/javascript', headers:{'access-control-allow-origin':'*'}});
  });
  await page.route('http://alpine.local/', route => route.fulfill({body:readFileSync(`${folder}/source.html`),contentType:'text/html'}));
  await page.goto('http://alpine.local/');
  await page.waitForFunction(() => window.__cfg);
  const result = await page.evaluate(async () => {
    const THREE = await import('three');
    const { GLTFExporter } = await import('three/addons/exporters/GLTFExporter.js');
    const {vehicle,doors,controls} = window.__cfg;
    controls.autoRotate = false;
    const [chassis,cab,track,front] = vehicle.children;
    for (const door of doors) {
      door.rotation.y = 0;
      const name = door.position.z < 0 ? 'door_front_left' : 'door_front_right';
      // Move the upper glazing with the lower leaf for a full-height opening.
      const transom = cab.children.find(o => o.isMesh && Math.abs(o.position.x+0.28)<0.001 && Math.sign(o.position.z)===Math.sign(door.position.z));
      if (transom) door.attach(transom);
      door.traverse(o => {if(o.isMesh) o.name=name;});
    }
    const rear=-2.02, forward=0.32, radius=0.241, straight=forward-rear;
    const loop=2*straight+2*Math.PI*radius;
    function frame(s) {
      if(s<straight) return [forward-s,0.259-radius,Math.PI];
      if(s<straight+Math.PI*radius) {const a=-Math.PI/2-(s-straight)/radius;return [rear+radius*Math.cos(a),0.259+radius*Math.sin(a),a-Math.PI/2];}
      if(s<2*straight+Math.PI*radius) return [rear+s-straight-Math.PI*radius,0.259+radius,0];
      const a=Math.PI/2-(s-2*straight-Math.PI*radius)/radius;
      return [forward+radius*Math.cos(a),0.259+radius*Math.sin(a),a-Math.PI/2];
    }
    const material=new THREE.MeshStandardMaterial({color:0x292c31,roughness:0.92});
    for(const side of track.children.filter(o=>o.isGroup)) {
      // Replace the irregular static wrap with evenly pitched, animated shoes.
      for(const o of [...side.children]) {
        if(o.isMesh && o.material.map) {side.remove(o);continue;}
        if(o.geometry?.type==='CylinderGeometry' && o.geometry.parameters.radiusTop>=0.23) o.name='snow_track_wheel';
      }
      for(let i=0;i<56;i++) {
        const [x,y,a]=frame(i*loop/56);
        const shoe=new THREE.Mesh(new THREE.BoxGeometry(loop/56+0.003,0.035,0.30),material);
        shoe.position.set(x,y,0);shoe.rotation.z=a;
        shoe.name=`snow_track_shoe_${i}`;side.add(shoe);
      }
    }
    for(const g of front.children.filter(o=>o.isGroup)) g.traverse(o=>{if(o.isMesh)o.name='snow_ski';});
    vehicle.traverse(o=>{
      if(!o.isMesh)return;
      if(o.userData.skipExport || o.geometry.type==='PlaneGeometry') {o.visible=false;return;}
      if(o.material.map){o.material=o.material.clone();o.material.map=null;}
      if(!o.geometry.index) {
        o.geometry=o.geometry.clone();
        o.geometry.setIndex(Array.from({length:o.geometry.attributes.position.count},(_,i)=>i));
      }
      if(o.material.emissive?.r>0.5 && o.position.x>0.7)o.name='headlamp_lens';
      if(o.material.emissive?.r>0.5 && o.material.emissive.g<0.1)o.name='rear_lamp';
      if(!o.name)o.name='body';
    });
    // Room for an adult driver: scale geometry, with matching game articulation dimensions.
    vehicle.scale.setScalar(1.35);
    vehicle.updateMatrixWorld(true);
    const bounds=new THREE.Box3().setFromObject(vehicle);
    const glb=await new GLTFExporter().parseAsync(vehicle,{binary:true,onlyVisible:true});
    return {bytes:Array.from(new Uint8Array(glb)),bounds:[bounds.min.toArray(),bounds.max.toArray()]};
  });
  writeFileSync(`${folder}/model.glb`,Buffer.from(result.bytes));
  console.log(JSON.stringify({bytes:result.bytes.length,bounds:result.bounds}));
} finally {await browser.close();}
