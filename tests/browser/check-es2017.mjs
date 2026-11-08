import { readFile } from 'node:fs/promises';
import { parse } from 'acorn';

const source = await readFile(new URL('../../assets/index.js', import.meta.url), 'utf8');
parse(source, { ecmaVersion: 2017, sourceType: 'script' });
console.log('assets/index.js parses as ES2017');
