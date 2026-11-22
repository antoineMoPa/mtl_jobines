import {copyFile, mkdir, readFile} from 'node:fs/promises';
import {dirname, join} from 'node:path';
const [root, dist] = process.argv.slice(2);
const list = await readFile(join(root, 'web/assets.list'), 'utf8');
for (const path of list.split('\n')) {
    if (!path.startsWith('opencity/') && path !== 'assets/terrain/grass-mips.png') continue;
    await mkdir(dirname(join(dist, path)), {recursive:true});
    await copyFile(join(root, path), join(dist, path));
}
