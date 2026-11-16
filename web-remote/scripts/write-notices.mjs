import { readFile, readdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const lock = JSON.parse(await readFile(path.join(root, 'package-lock.json'), 'utf8'));
const sections = [
  'TianCaiSpaceHub remote conversation web resources — third-party notices',
  'Generated from the locked production dependency set. Development tools are excluded.',
  'Dependencies retain their original copyright and license notices below.',
];

for (const [relative, entry] of Object.entries(lock.packages).sort(([a], [b]) => a.localeCompare(b))) {
  if (!relative || entry.dev || entry.link) continue;
  const directory = path.join(root, relative);
  const manifest = JSON.parse(await readFile(path.join(directory, 'package.json'), 'utf8'));
  const names = await readdir(directory);
  const licenses = names.filter(name => /^(licen[sc]e|copying)([-.].*)?$/i.test(name));
  sections.push(`\n${'='.repeat(72)}\n${manifest.name} ${manifest.version}\nLicense: ${typeof manifest.license === 'string' ? manifest.license : entry.license || 'See license text'}\n`);
  if (!licenses.length) throw new Error(`Missing license file in ${manifest.name}`);
  for (const name of licenses.sort()) {
    sections.push(`[${name}]\n${await readFile(path.join(directory, name), 'utf8')}`);
  }
}

await writeFile(path.resolve(root, '../assets/remote-web/THIRD_PARTY_NOTICES.txt'), sections.join('\n'), 'utf8');
