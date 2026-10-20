const fs = require('node:fs');
const file = process.argv[2];
const marker = 'CODEX_ULTRA_LOCAL_FALLBACK_V1';
const migrationMarker = 'CODEX_ULTRA_LOCAL_FALLBACK_V1_MIGRATION';

if (!file || file === '__none__') {
  process.stdout.write('not-applicable');
  process.exit(0);
}

const text = fs.readFileSync(file, 'utf8');
if (text.includes(marker) && text.includes(migrationMarker) && text.includes('ultraEffortLocalFallback')) {
  process.stdout.write('already-patched');
  process.exit(0);
}

const readConfigMatch = text.match(/([$A-Za-z_][$\w]*)\(([$A-Za-z_][$\w]*)\.showUltraInModelPickerSlider\)\.catch\(\(\)=>!1\)/);
const writeConfigMatch = text.match(/await ([$A-Za-z_][$\w]*)\(([$A-Za-z_][$\w]*),([$A-Za-z_][$\w]*)\.showUltraInModelPickerSlider,!1\)\.catch\(\(\)=>\{\}\)/);
if (!readConfigMatch || !writeConfigMatch || readConfigMatch[2] !== writeConfigMatch[3]) {
  process.stderr.write('ultra-local-config-helper-target-not-found\n');
  process.exit(2);
}
const readConfig = readConfigMatch[1];
const settings = readConfigMatch[2];
const writeConfig = writeConfigMatch[1];

const mutationRe = /async function ([$A-Za-z_][$\w]*)\(([$A-Za-z_][$\w]*),([$A-Za-z_][$\w]*)\)\{let ([$A-Za-z_][$\w]*)=\2\.query\.snapshot\(([$A-Za-z_][$\w]*)\),([$A-Za-z_][$\w]*)=\4\.getData\(\);\4\.setData\(([$A-Za-z_][$\w]*)=>\7==null\?\7:\{\.\.\.\7,ultraEffortEnabled:\3\}\);try\{await \2\.get\(([$A-Za-z_][$\w]*)\)\.setUltraEffortEnabled\(\3\),await Promise\.all\(\[\4\.invalidate\(\),\2\.query\.snapshot\(([$A-Za-z_][$\w]*)\)\.invalidate\(\)\]\)\}catch\(([$A-Za-z_][$\w]*)\)\{throw \4\.setData\(\6\),\10\}\}/;
const mutationMatch = text.match(mutationRe);
if (!mutationMatch) {
  process.stderr.write('ultra-mutation-target-not-found\n');
  process.exit(2);
}
const [, mutationFn, mutationScope, enabled, snapshot, userSettingsQuery, previous, cached, client, tppQuery] = mutationMatch;
const mutationReplacement = `async function ${mutationFn}(${mutationScope},${enabled}){let ${snapshot}=${mutationScope}.query.snapshot(${userSettingsQuery}),${previous}=${snapshot}.getData();${snapshot}.setData(${cached}=>${cached}==null?${cached}:{...${cached},ultraEffortEnabled:${enabled}});if(${previous}?.ultraEffortLocalFallback===!0){try{await ${writeConfig}(${mutationScope},${settings}.showUltraInModelPickerSlider,${enabled});return}catch(codexUltraLocalError){throw ${snapshot}.setData(${previous}),codexUltraLocalError}}try{await ${mutationScope}.get(${client}).setUltraEffortEnabled(${enabled}),await Promise.all([${snapshot}.invalidate(),${mutationScope}.query.snapshot(${tppQuery}).invalidate()])}catch(codexUltraRemoteError){throw ${snapshot}.setData(${previous}),codexUltraRemoteError}}`;
let next = text.replace(mutationRe, mutationReplacement);

// 0.6.24+ moves the conversation client behind a dynamic import inside the
// queryFn and adds keys (chatTheme) to the returned object, so tolerate an
// arbitrary prologue and any number of extra keys. The whole success body is
// captured verbatim and re-emitted inside the try, which keeps every returned
// field intact instead of rebuilding the object from named groups.
// 26.831+ adds settings keys both before lockdownModeEnabled and after
// ultraEffortEnabled, so locate the userSettings queryFn by anchor, capture its
// whole body with brace matching, and re-emit it verbatim inside the try. The
// enumerated-key regex stays below as a fallback for older build shapes.
let queryPatched = false;
{
  const anchor = '.userSettings());';
  let probe = 0;
  for (;;) {
    const anchorIdx = next.indexOf(anchor, probe);
    if (anchorIdx < 0) break;
    const windowStart = next.lastIndexOf('queryFn:async()=>{', anchorIdx);
    if (windowStart < 0) break;
    if (anchorIdx - windowStart >= 4000) {
      probe = anchorIdx + anchor.length;
      continue;
    }
    const bodyStart = windowStart + 'queryFn:async()=>{'.length;
    let depth = 1;
    let i = bodyStart;
    while (i < next.length && depth > 0) {
      const ch = next[i];
      if (ch === '{') depth++;
      else if (ch === '}') depth--;
      i++;
    }
    if (depth === 0) {
      const bodyEnd = i - 1;
      const successBody = next.slice(bodyStart, bodyEnd);
      if (successBody.includes('model_picker_persists_ultra_effort')) {
        const queryReplacement = `queryFn:async()=>{try{${successBody}}catch{return{lockdownModeEnabled:!1,ultraEffortEnabled:await ${readConfig}(${settings}.showUltraInModelPickerSlider).catch(()=>!1)===!0,ultraEffortLocalFallback:!0/*${marker}*/}}}`;
        next = next.slice(0, windowStart) + queryReplacement + next.slice(bodyEnd + 1);
        queryPatched = true;
      }
    }
    break;
  }
}
if (!queryPatched) {
const queryRe = /queryFn:async\(\)=>\{(let(?:\{[\s\S]{0,800}?import\.meta\.url\),|\s)([$A-Za-z_][$\w]*)=([$A-Za-z_][$\w]*)\.parse\(await ([$A-Za-z_][$\w]*)\.get\(([$A-Za-z_][$\w]*)\)\.userSettings\(\)\);return\{(?:[$A-Za-z_][$\w]*:[^,]+,)*?lockdownModeEnabled:\2\.settings\?\.lockdown_mode_enabled===!0,ultraEffortEnabled:\2\.settings\?\.model_picker_persists_ultra_effort===!0\})\}/;
const queryMatch = next.match(queryRe);
if (!queryMatch) {
  process.stderr.write('ultra-user-settings-query-target-not-found\n');
  process.exit(2);
}
const querySuccessBody = queryMatch[1];
const queryReplacement = `queryFn:async()=>{try{${querySuccessBody}}catch{return{lockdownModeEnabled:!1,ultraEffortEnabled:await ${readConfig}(${settings}.showUltraInModelPickerSlider).catch(()=>!1)===!0,ultraEffortLocalFallback:!0/*${marker}*/}}}`;
next = next.replace(queryRe, queryReplacement);
}

const migrationKey = 'queryKey:[`chatgpt-ultra-effort-migration`]';
const migrationKeyIndex = next.indexOf(migrationKey);
if (migrationKeyIndex < 0) {
  process.stderr.write('ultra-migration-query-target-not-found\n');
  process.exit(2);
}
const migrationStart = Math.max(0, migrationKeyIndex - 1200);
const migrationPrefix = next.slice(0, migrationStart);
const migrationSegment = next.slice(migrationStart, migrationKeyIndex + migrationKey.length);
const migrationOriginal = '.setUltraEffortEnabled(!0).catch(()=>{})';
if (!migrationSegment.includes(migrationOriginal)) {
  process.stderr.write('ultra-migration-swallowed-error-target-not-found\n');
  process.exit(2);
}
next = migrationPrefix + migrationSegment.replace(migrationOriginal, `.setUltraEffortEnabled(!0)/*${migrationMarker}*/`) + next.slice(migrationKeyIndex + migrationKey.length);

if (!next.includes(marker) || !next.includes(migrationMarker) || !next.includes('ultraEffortLocalFallback')) {
  process.stderr.write('ultra-local-fallback-patch-verification-failed\n');
  process.exit(2);
}
fs.writeFileSync(file, next);
process.stdout.write('patched');
