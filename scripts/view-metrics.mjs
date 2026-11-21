import { execFile } from 'node:child_process';
import { createReadStream } from 'node:fs';
import { readdir, stat } from 'node:fs/promises';
import { createServer } from 'node:http';
import { pipeline } from 'node:stream';
import { promisify } from 'node:util';

const transfer = promisify(pipeline);

const directory = new URL('../target/traces/', import.meta.url);
const files = await readdir(directory).catch(error => {
  if (error.code === 'ENOENT') return [];
  throw error;
});
// Filenames start with a fixed-width epoch timestamp. Partial captures use a separate suffix.
const latest = files.filter(name => /^game-\d+-\d+\.json$/.test(name)).sort().pop();
if (!latest) {
  console.error('Record a session with MTL_TRACE=1 cargo run --locked, then quit the game.');
  process.exit(1);
}
const file = new URL(latest, directory);
const { size } = await stat(file);
const origin = 'https://ui.perfetto.dev';
const server = createServer(async (req, res) => {
  if (req.url !== '/' + latest) {
    res.writeHead(404).end();
    return;
  }
  res.setHeader('Access-Control-Allow-Origin', origin);
  if (req.method === 'OPTIONS') {
    res.writeHead(204, {
      'Access-Control-Allow-Methods': 'GET, HEAD, OPTIONS',
      'Access-Control-Allow-Private-Network': 'true',
    }).end();
    return;
  }
  if (req.method !== 'GET' && req.method !== 'HEAD') {
    res.writeHead(405).end();
    return;
  }
  res.writeHead(200, { 'Content-Type': 'application/json', 'Content-Length': size });
  if (req.method === 'HEAD') {
    res.end();
    return;
  }
  try {
    await transfer(createReadStream(file), res);
    server.close();
  } catch (error) {
    console.error('Trace transfer interrupted:', error.message);
  }
});
server.on('error', error => {
  console.error('Could not serve the trace:', error.message);
  process.exitCode = 1;
});
// Use the same loopback endpoint as Perfetto's open_trace_in_ui utility.
server.listen(9001, '127.0.0.1', () => {
  const url = origin + '/#!/?url=' + encodeURIComponent('http://127.0.0.1:9001/' + latest);
  console.log('Opening', latest, '\n' + url);
  const opener = { darwin: 'open', win32: 'rundll32' }[process.platform] ?? 'xdg-open';
  const args = process.platform === 'win32' ? ['url.dll,FileProtocolHandler', url] : [url];
  execFile(opener, args, error => {
    if (error) console.error('Open the URL above to view the trace.');
  });
});
