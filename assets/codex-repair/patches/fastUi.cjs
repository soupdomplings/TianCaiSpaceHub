const fs = require('node:fs');
const file = process.argv[2];
const text = fs.readFileSync(file, 'utf8');

const speedUiPrefix = /function ([A-Za-z_$][\w$]*)\(([A-Za-z_$][\w$]*)\)\{let ([A-Za-z_$][\w$]*)=\(0,[A-Za-z_$][\w$]*\.c\)\(10\),([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*\),([A-Za-z_$][\w$]*)=\2\?\.hostId\?\?\4,([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*\(\5\),([A-Za-z_$][\w$]*)=/g;
const speedUiCandidates = [];
for (const match of text.matchAll(speedUiPrefix)) {
  const [, fn, param, memo, root, host, auth, allowed] = match;
  const offset = match.index + match[0].length;
  const original = `${auth}?.authMethod===\`chatgpt\`||${auth}?.authMethod===\`personalAccessToken\``;
  const patched = '/*CODEX_SERVICE_TIER_ACCESS_UI_V1*/!0';
  const value = text.startsWith(original, offset) ? original : text.startsWith(patched, offset) ? patched : null;
  if (value === null) continue;
  const end = text.indexOf('}var ', offset);
  if (end < 0 || end - offset > 1800) continue;
  const body = text.slice(offset + value.length, end);
  const access = body.match(/let\{data:([A-Za-z_$][\w$]*),isPending:([A-Za-z_$][\w$]*)\}=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*,[A-Za-z_$][\w$]*\),([A-Za-z_$][\w$]*)=!!([A-Za-z_$][\w$]*)\?\.isLoading\|\|([A-Za-z_$][\w$]*)&&\2,([A-Za-z_$][\w$]*);/);
  if (!access || access[4] !== auth || access[5] !== allowed) continue;
  const [, data, pending, loading, , , result] = access;
  const conversion = body.match(/([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*)&&!([A-Za-z_$][\w$]*)&&([A-Za-z_$][\w$]*)!=null\?[A-Za-z_$][\w$]*\(\4\):null/);
  const returned = body.match(/\{serviceTierAccess:[A-Za-z_$][\w$]*,isLoading:([A-Za-z_$][\w$]*)\}/);
  if (!conversion || conversion[1] !== result || conversion[2] !== allowed ||
      conversion[3] !== loading || conversion[4] !== data || !returned || returned[1] !== loading) continue;
  speedUiCandidates.push({offset, value, patched});
}
if (speedUiCandidates.length > 1) {
  process.stderr.write('fast-ui-speed-access-target-ambiguous\n');
  process.exit(2);
}
const speedUiMarkerCount = text.split('CODEX_SERVICE_TIER_ACCESS_UI_V1').length - 1;
if (speedUiMarkerCount !== speedUiCandidates.filter(candidate => candidate.value === candidate.patched).length) {
  process.stderr.write('fast-ui-speed-access-patch-incomplete\n');
  process.exit(2);
}
if (speedUiCandidates.length === 1) {
  const {offset, value, patched} = speedUiCandidates[0];
  if (value === patched) {
    process.stdout.write('already-patched');
  } else {
    fs.writeFileSync(file, text.slice(0, offset) + patched + text.slice(offset + value.length));
    process.stdout.write('patched');
  }
  process.exit(0);
}
const constantPatchedRe = /let\{data:\w+,isPending:\w+\}=[^;]+,(\w+)=!1,(\w+)=!0,\w+;[\s\S]*?\{isServiceTierAllowed:\2,isLoading:\1\}/;
if (constantPatchedRe.test(text)) {
  process.stdout.write('already-patched');
  process.exit(0);
}

const patchedRe = /let\{data:\w+,isPending:(\w+)\}=[A-Za-z_$][\w$]*\([^)]+\),(\w+)=!!\w+\?\.isLoading\|\|\1,(\w+)=!\2&&\w+!=null&&\w+\?\.requirements\?\.featureRequirements\?\.fast_mode!==!1/;
if (patchedRe.test(text)) {
  process.stdout.write('already-patched');
  process.exit(0);
}

const uiGateRe = /let\{data:(\w+),isPending:(\w+)\}=([A-Za-z_$][\w$]*\([^)]+\)),(\w+)=!!(\w+)\?\.isLoading\|\|(\w+)&&\2,(\w+)=\6&&!\4&&\1!=null&&\1\?\.requirements\?\.featureRequirements\?\.fast_mode!==!1/;
const match = text.match(uiGateRe);
if (!match) {
  process.stderr.write('fast-ui-patch-target-not-found\n');
  process.exit(2);
}

const [, dataVar, pendingVar, queryCall, loadingVar, authStateVar, chatgptOnlyVar, allowedVar] = match;
const replacement = `let{data:${dataVar},isPending:${pendingVar}}=${queryCall},${loadingVar}=!!${authStateVar}?.isLoading||${pendingVar},${allowedVar}=!${loadingVar}&&${dataVar}!=null&&${dataVar}?.requirements?.featureRequirements?.fast_mode!==!1`;
fs.writeFileSync(file, text.replace(uiGateRe, replacement));
process.stdout.write('patched');
