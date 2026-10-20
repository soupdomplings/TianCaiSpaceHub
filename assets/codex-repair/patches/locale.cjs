const fs = require('node:fs');
const file = process.argv[2];
const text = fs.readFileSync(file, 'utf8');

if (!text.includes('enable_i18n') && text.includes('locale_source')) {
  process.stdout.write('already-patched');
  process.exit(0);
}

let next = text.replace(/(\w+=)\w+\?\.get\(`enable_i18n`,!1\)/, '$1!0');
if (next === text) {
  process.stderr.write('locale-i18n-patch-target-not-found\n');
  process.exit(2);
}

fs.writeFileSync(file, next);
process.stdout.write('patched');
