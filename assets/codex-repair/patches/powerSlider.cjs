const fs = require('node:fs');
const file = process.argv[2];
const marker = 'CODEX_POWER_SLIDER_V1';
const text = fs.readFileSync(file, 'utf8');

if (text.includes(marker)) {
  process.stdout.write('already-patched');
  process.exit(0);
}

if (!text.includes('harborEnabled:') && (text.includes('composer.modelPicker.power') || /model-picker-power-slider-impl/i.test(file) || text.includes('ModelPickerPowerSliderImpl'))) {
  process.stdout.write('already-patched');
  process.exit(0);
}

const gateRe = /function ([$A-Za-z_][$\w]*)\(\{harborEnabled:([$A-Za-z_][$\w]*),isElectron:([$A-Za-z_][$\w]*),isEverydayWorkMode:([$A-Za-z_][$\w]*)\}\)\{return \4\|\|\3&&\2\}/;
const match = text.match(gateRe);
if (!match) {
  process.stderr.write('power-slider-harbor-gate-target-not-found\n');
  process.exit(2);
}

const [, fn, harborEnabled, isElectron, isEverydayWorkMode] = match;
const replacement = `function ${fn}({harborEnabled:${harborEnabled},isElectron:${isElectron},isEverydayWorkMode:${isEverydayWorkMode}}){return ${isEverydayWorkMode}||${isElectron}/*${marker}*/}`;
const next = text.replace(gateRe, replacement);
if (!next.includes(marker)) {
  process.stderr.write('power-slider-patch-verification-failed\n');
  process.exit(2);
}
fs.writeFileSync(file, next);
process.stdout.write('patched');
