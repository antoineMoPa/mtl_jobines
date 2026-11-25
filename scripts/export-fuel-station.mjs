// Re-export the editable low-poly station in a private, window-free Claydash session.
// CLAYDASH_BIN=/path/to/claydash node scripts/export-fuel-station.mjs [output.glb]
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, renameSync, copyFileSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const source = join(repo, 'data/maps/mtl/models/fuel-station.claydash');
const output = resolve(process.argv[2] ?? join(repo, 'data/maps/mtl/models/fuel-station.glb'));
const binary = process.env.CLAYDASH_BIN ?? (process.platform === 'darwin' && existsSync('/Applications/Claydash.app/Contents/MacOS/claydash')
  ? '/Applications/Claydash.app/Contents/MacOS/claydash' : 'claydash');
const scratch = mkdtempSync(join(tmpdir(), 'fuel-station-export-'));
const candidate = join(scratch, 'fuel-station.glb');
const child = spawn(binary, ['mcp', '--headless', '--scene', source], { stdio: ['pipe', 'pipe', 'pipe'] });
const pending = new Map();
let serial = 1, buffer = '';
const fail = error => {
  for (const request of pending.values()) { clearTimeout(request.timeout); request.reject(error); }
  pending.clear();
};
child.stderr.on('data', bytes => process.stderr.write(bytes));
child.on('error', fail);
child.on('exit', code => fail(new Error(`Private Claydash session exited (${code})`)));
child.stdout.on('data', bytes => {
  buffer += bytes;
  let newline;
  while ((newline = buffer.indexOf('\n')) >= 0) {
    const line = buffer.slice(0, newline); buffer = buffer.slice(newline + 1);
    if (!line.trim()) continue;
    let message;
    try { message = JSON.parse(line); } catch { fail(new Error(`Invalid MCP response: ${line.slice(0, 200)}`)); continue; }
    const request = pending.get(message.id);
    if (!request) continue;
    clearTimeout(request.timeout); pending.delete(message.id);
    if (message.error) request.reject(new Error(JSON.stringify(message.error)));
    else request.resolve(message.result);
  }
});
const call = (method, params = {}) => new Promise((resolve, reject) => {
  const id = serial++;
  const timeout = setTimeout(() => { pending.delete(id); reject(new Error(`MCP timeout: ${method}`)); }, 60_000);
  pending.set(id, { resolve, reject, timeout });
  child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
});
const tool = async (name, args) => {
  const result = await call('tools/call', { name, arguments: args });
  const text = result.content?.find(item => item.type === 'text')?.text;
  if (result.isError) throw new Error(text ?? `Claydash ${name} failed`);
  return result.structuredContent ?? JSON.parse(text);
};
let job;
try {
  await call('initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'fuel-station-export', version: '1' } });
  child.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
  job = await tool('export_glb', { path: candidate, geometry: 'direct', direct_resolution: 16, direct_error: 0.02 });
  const deadline = Date.now() + 180_000;
  while (job.status !== 'completed') {
    if (['failed', 'cancelled'].includes(job.status)) throw new Error(job.error ?? `Export ${job.status}`);
    if (Date.now() > deadline) { await tool('cancel_glb_export', { id: job.id }); throw new Error('Export deadline exceeded'); }
    await delay(250);
    job = await tool('get_glb_export_status', { id: job.id });
  }
  const bytes = readFileSync(candidate);
  if (bytes.readUInt32LE(0) !== 0x46546c67 || bytes.readUInt32LE(4) !== 2) throw new Error('Expected glTF binary version 2');
  const doc = JSON.parse(bytes.subarray(20, 20 + bytes.readUInt32LE(12)).toString());
  const triangles = doc.nodes.reduce((sum, node) => sum + (node.mesh === undefined ? 0 : doc.meshes[node.mesh].primitives.reduce(
    (total, primitive) => total + doc.accessors[primitive.indices ?? primitive.attributes.POSITION].count / 3, 0)), 0);
  if (!triangles || triangles > 10_000) throw new Error(`Station triangle budget exceeded: ${triangles}`);
  const expected = [[-15.1000004, -0.050000012, -11.1000004], [15.1000004, 7.1500001, 10.0550003]];
  const lo = [Infinity, Infinity, Infinity], hi = [-Infinity, -Infinity, -Infinity];
  for (const mesh of doc.meshes) for (const primitive of mesh.primitives) {
    const positions = doc.accessors[primitive.attributes.POSITION];
    for (let axis = 0; axis < 3; axis++) { lo[axis] = Math.min(lo[axis], positions.min[axis]); hi[axis] = Math.max(hi[axis], positions.max[axis]); }
  }
  if (doc.nodes.some(node => node.matrix || node.translation || node.rotation || node.scale)) throw new Error('Bounds check expects baked world transforms');
  for (let axis = 0; axis < 3; axis++) {
    if (!Number.isFinite(lo[axis]) || !Number.isFinite(hi[axis]) || Math.abs(lo[axis] - expected[0][axis]) > 0.005 || Math.abs(hi[axis] - expected[1][axis]) > 0.005) throw new Error('Station placement bounds changed');
  }
  // Publish only after validation; never replace the current asset with a failed export.
  const staging = `${output}.export-${process.pid}`;
  try { copyFileSync(candidate, staging); renameSync(staging, output); }
  finally { rmSync(staging, { force: true }); }
  console.log(JSON.stringify({ output, nodes: doc.nodes.length, triangles, bytes: bytes.length, bounds: [lo, hi] }, null, 2));
} finally {
  child.stdin.end();
  const stop = setTimeout(() => child.kill(), 1500);
  stop.unref();
  child.once('exit', () => clearTimeout(stop));
  rmSync(scratch, { recursive: true, force: true });
}
