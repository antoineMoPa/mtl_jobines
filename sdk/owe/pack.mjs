// Resolve source paths at build time. Native and browser clients load identical bytes.
import {readFile, writeFile, rename} from 'node:fs/promises';
import {dirname, resolve} from 'node:path';
const [manifest, output] = process.argv.slice(2);
const pack = JSON.parse(await readFile(manifest, 'utf8'));
for (const map of Object.values(pack.maps)) {
    map.road = await readFile(resolve(dirname(manifest), map.road), 'utf8');
    if (map.elevation) {
        map.elevation = Array.from(await readFile(resolve(dirname(manifest), map.elevation)));
    }
}
const tmp = output + '.' + process.pid + '.tmp';
await writeFile(tmp, JSON.stringify(pack));
await rename(tmp, output);
