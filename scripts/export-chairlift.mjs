// Export the supplied offline configurator without altering its authored geometry.
// Usage: modern-node scripts/export-chairlift.mjs .tmp/chairlift.html
import {readFile, writeFile, mkdir} from 'node:fs/promises';
import {resolve} from 'node:path';
import {chromium} from '../tests/web/node_modules/playwright/index.mjs';
const input=resolve(process.argv[2]||'.tmp/chairlift.html');
const source=await readFile(input,'utf8');
const marker='// ---------------------------------------------------------------- three.js setup';
if(!source.includes(marker))throw Error('Expected the supplied chairlift configurator.');
const hook=`globalThis.exportChairliftAssets = async () => {
  const result=[];
  for (const [name,model] of [['chair',buildChair(state.chair)],['pylon',buildPylon(state.pylon)],['base-station',buildStation(state.base,'base')],['top-station',buildStation(state.top,'top')]]) {
    const json=await new GLTFExporter().parseAsync(model,{binary:false});
    const bytes=new Uint8Array(packGLB(json));
    let binary=''; for(let i=0;i<bytes.length;i+=8192)binary+=String.fromCharCode(...bytes.subarray(i,i+8192));
    const bounds=new THREE.Box3().setFromObject(model);
    let triangles=0;model.traverse(o=>{if(o.isMesh)triangles+=(o.geometry.index?.count||o.geometry.attributes.position.count)/3;});
    result.push({name,base64:btoa(binary),triangles,bounds:{min:bounds.min.toArray(),max:bounds.max.toArray()}});
  }
  return result;
};\n`;
const browser=await chromium.launch({headless:true});
try {
  const page=await browser.newPage();
  await page.setContent(source.replace(marker,hook+marker));
  await page.waitForFunction(()=>typeof globalThis.exportChairliftAssets==='function');
  const result=await page.evaluate(()=>globalThis.exportChairliftAssets());
  await mkdir('data/equipment/chairlift',{recursive:true});
  for(const asset of result){const bytes=Buffer.from(asset.base64,'base64');await writeFile(`data/equipment/chairlift/${asset.name}.glb`,bytes);console.log(asset.name,bytes.length,asset.triangles);delete asset.base64;asset.bytes=bytes.length;}
  await writeFile('data/equipment/chairlift/manifest.json',JSON.stringify({source:'.tmp/chairlift.html',units:'metres',axes:'glTF +Y up; chair faces -Z; chair origin at grip, other origins at ground',assets:result},null,2)+'\n');
} finally {await browser.close();}
