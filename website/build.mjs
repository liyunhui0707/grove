import fs from 'node:fs';
import path from 'node:path';
const source = path.join(import.meta.dirname, 'src');
const output = path.join(import.meta.dirname, 'dist');
fs.mkdirSync(path.join(output, 'assets'), { recursive: true });
for (const name of ['index.html', 'styles.css', 'site.js', 'assets/grove.svg', 'assets/comparison.png'])
  fs.copyFileSync(path.join(source, name), path.join(output, name));
console.log('Built static Grove website with five explicit public files.');
