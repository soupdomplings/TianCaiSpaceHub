const fs = require('node:fs');
const file = process.argv[2];
const text = fs.readFileSync(file, 'utf8');
const marker = 'CODEX_NODE_REPL_TRUSTED_PATHS_V1';
const identifier = '[A-Za-z_$][\\w$]*';

if (text.includes(marker)) {
  process.stdout.write('already-patched');
  process.exit(0);
}

const keyMatch = text.match(/([A-Za-z_$][\w$]*)=`NODE_REPL_TRUSTED_CODE_PATHS`/);
if (!keyMatch) {
  process.stderr.write('node-repl-trusted-paths-key-not-found\n');
  process.exit(2);
}

const key = keyMatch[1];
const escapedKey = key.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const originalRe = new RegExp(`\\[${escapedKey}\\]:(${identifier})\\(\\[(${identifier}),(${identifier})\\],(${identifier})\\)`);
const patchedRe = new RegExp(`\\[${escapedKey}\\]:${identifier}\\(\\[${identifier},${identifier},process\\.env\\[${escapedKey}\\](?:\\?\\?\`\`)?\\],${identifier}\\)`);
if (patchedRe.test(text)) {
  process.stdout.write('already-patched');
  process.exit(0);
}

const match = text.match(originalRe);
if (!match) {
  process.stderr.write('node-repl-trusted-paths-target-not-found\n');
  process.exit(2);
}

const [, joinFunction, codexHome, nodeModuleDirs, platform] = match;
const replacement = `[${key}]:${joinFunction}([${codexHome},${nodeModuleDirs},process.env[${key}]??\`\`],${platform})/*${marker}*/`;
const next = text.replace(originalRe, replacement);
fs.writeFileSync(file, next);
process.stdout.write('patched');
