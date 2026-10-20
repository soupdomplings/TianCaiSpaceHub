const fs = require('node:fs');
const file = process.argv[2];
const text = fs.readFileSync(file, 'utf8');

const legacyPatchedRe = /function L\(e\)\{let (\w+)=v\(x\),(\w+)=e\?\.hostId\?\?\1,\{data:(\w+)\}=d\(E,\2\);return \3\?\.requirements\?\.featureRequirements\?\.fast_mode!==!1\}/;
const currentDirectPatchedRe = /featureRequirements\?\.fast_mode===!1;return!\w+\}/;
const currentAsyncPatchedRe = /async function \w+\(\w+,\w+\)\{let \w+=await \w+\(\w+,\w+\);return\(await \w+\.query\.fetch\(\w+,\{authMethod:\w+,hostId:\w+\}\)\)\.requirements\?\.featureRequirements\?\.fast_mode!==!1\}/;
const currentCachedAsyncPatchedRe = /async function ([A-Za-z_$][\w$]*)\(([A-Za-z_$][\w$]*),([A-Za-z_$][\w$]*)\)\{let ([A-Za-z_$][\w$]*)=await ([A-Za-z_$][\w$]*)\(\2,\3\);let ([A-Za-z_$][\w$]*)=await ([A-Za-z_$][\w$]*)\(\3,\{priority:`critical`\}\);return \2\.query\.setData\(([A-Za-z_$][\w$]*),\{authMethod:\4,hostId:\3\},\6\),\6\.requirements\?\.featureRequirements\?\.fast_mode!==!1\}/;
const currentManagerCachedAsyncPatchedRe = /async function ([A-Za-z_$][\w$]*)\(([A-Za-z_$][\w$]*),([A-Za-z_$][\w$]*)\)\{let ([A-Za-z_$][\w$]*)=await ([A-Za-z_$][\w$]*)\(\2,\3\);let ([A-Za-z_$][\w$]*)=await ([A-Za-z_$][\w$]*)\(\2,\3,\{priority:`critical`\}\);return \2\.query\.setData\(([A-Za-z_$][\w$]*),\{authMethod:\4,hostId:\3\},\6\),\6\.requirements\?\.featureRequirements\?\.fast_mode!==!1\}/;
const legacyOriginalRe = /function L\(e\)\{let (\w+)=v\(x\),(\w+)=e\?\.hostId\?\?\1,(\w+)=O\(\2\),\{data:(\w+)\}=d\(E,\2\);return!\(\3\?\.authMethod!==`chatgpt`\|\|\4\?\.requirements\?\.featureRequirements\?\.fast_mode===!1\)\}/;
const currentDirectOriginalRe = /function (\w+)\(e\)\{let (\w+)=([^,;]+),(\w+)=e\?\.hostId\?\?\2,(\w+)=(\w+\(\4\)),\{data:(\w+)\}=(\w+\(\w+,\4\)),(\w+)=\7\?\.requirements\?\.featureRequirements\?\.fast_mode===!1;return!\(\5\?\.authMethod!==`chatgpt`\|\|\9\)\}/;
const currentAsyncOriginalRe = /async function (\w+)\((\w+),(\w+)\)\{let (\w+)=await ([A-Za-z_$][\w$]*)\(\2,\3\);return \4===`chatgpt`\?\(await \2\.query\.fetch\(([A-Za-z_$][\w$]*),\{authMethod:\4,hostId:\3\}\)\)\.requirements\?\.featureRequirements\?\.fast_mode!==!1:!1\}/;
const currentCachedAsyncOriginalRe = /async function ([A-Za-z_$][\w$]*)\(([A-Za-z_$][\w$]*),([A-Za-z_$][\w$]*)\)\{let ([A-Za-z_$][\w$]*)=await ([A-Za-z_$][\w$]*)\(\2,\3\);if\(\4!==`chatgpt`\)return!1;let ([A-Za-z_$][\w$]*)=await ([A-Za-z_$][\w$]*)\(\3,\{priority:`critical`\}\);return \2\.query\.setData\(([A-Za-z_$][\w$]*),\{authMethod:\4,hostId:\3\},\6\),\6\.requirements\?\.featureRequirements\?\.fast_mode!==!1\}/;
const currentManagerCachedAsyncOriginalRe = /async function ([A-Za-z_$][\w$]*)\(([A-Za-z_$][\w$]*),([A-Za-z_$][\w$]*)\)\{let ([A-Za-z_$][\w$]*)=await ([A-Za-z_$][\w$]*)\(\2,\3\);if\(\4!==`chatgpt`(?:&&\4!==`personalAccessToken`)?\)return!1;let ([A-Za-z_$][\w$]*)=await ([A-Za-z_$][\w$]*)\(\2,\3,\{priority:`critical`\}\);return \2\.query\.setData\(([A-Za-z_$][\w$]*),\{authMethod:\4,hostId:\3\},\6\),\6\.requirements\?\.featureRequirements\?\.fast_mode!==!1\}/;
// Desktop 26.1007 delegates both speed modes to the shared requirements converter.
const speedAccessOriginalRe = /async function ([A-Za-z_$][\w$]*)\(([A-Za-z_$][\w$]*),([A-Za-z_$][\w$]*)\)\{let ([A-Za-z_$][\w$]*)=await ([A-Za-z_$][\w$]*)\(\2,\3\);if\(\4!==`chatgpt`&&\4!==`personalAccessToken`\)return null;let ([A-Za-z_$][\w$]*)=await ([A-Za-z_$][\w$]*)\(\2,\3,\{priority:`critical`\}\);return \2\.query\.setData\(([A-Za-z_$][\w$]*),\{authMethod:\4,hostId:\3\},\6\),([A-Za-z_$][\w$]*)\(\6\)\}/g;
const speedAccessPatchedRe = /async function ([A-Za-z_$][\w$]*)\(([A-Za-z_$][\w$]*),([A-Za-z_$][\w$]*)\)\{let ([A-Za-z_$][\w$]*)=await ([A-Za-z_$][\w$]*)\(\2,\3\);let ([A-Za-z_$][\w$]*)=await ([A-Za-z_$][\w$]*)\(\2,\3,\{priority:`critical`\}\);return \2\.query\.setData\(([A-Za-z_$][\w$]*),\{authMethod:\4,hostId:\3\},\6\),([A-Za-z_$][\w$]*)\(\6\)\}/g;
const currentSplitConditionRe = /if\((\w+)\?\.authMethod!==`chatgpt`\|\|(\w+)\)\{/;

const speedOriginal = [...text.matchAll(speedAccessOriginalRe)];
const speedPatched = [...text.matchAll(speedAccessPatchedRe)];
if (speedOriginal.length + speedPatched.length > 1) {
  process.stderr.write('fast-mode-speed-access-target-ambiguous\n');
  process.exit(2);
}
if (speedOriginal.length === 1) {
  const [source, fn, manager, host, auth, authFn, data, requirementsFn, query, converter] = speedOriginal[0];
  const replacement = `async function ${fn}(${manager},${host}){let ${auth}=await ${authFn}(${manager},${host});let ${data}=await ${requirementsFn}(${manager},${host},{priority:\`critical\`});return ${manager}.query.setData(${query},{authMethod:${auth},hostId:${host}},${data}),${converter}(${data})}`;
  fs.writeFileSync(file, text.replace(source, replacement));
  process.stdout.write('patched');
  process.exit(0);
}
if (speedPatched.length === 1) {
  process.stdout.write('already-patched');
  process.exit(0);
}

if (legacyPatchedRe.test(text) || currentAsyncPatchedRe.test(text) || currentCachedAsyncPatchedRe.test(text) || currentManagerCachedAsyncPatchedRe.test(text) || (currentDirectPatchedRe.test(text) && !legacyOriginalRe.test(text) && !currentDirectOriginalRe.test(text) && !currentSplitConditionRe.test(text))) {
  process.stdout.write('already-patched');
  process.exit(0);
}

let next = text;
let patched = false;
const legacyMatch = next.match(legacyOriginalRe);
if (legacyMatch) {
  const [, rootVar, hostVar, , dataVar] = legacyMatch;
  next = next.replace(legacyOriginalRe, `function L(e){let ${rootVar}=v(x),${hostVar}=e?.hostId??${rootVar},{data:${dataVar}}=d(E,${hostVar});return ${dataVar}?.requirements?.featureRequirements?.fast_mode!==!1}`);
  patched = true;
}

if (!patched) {
  const currentMatch = next.match(currentDirectOriginalRe);
  if (currentMatch) {
    const [, fn, rootVar, rootExpr, hostVar, , , dataVar, dataCall, disabledVar] = currentMatch;
    next = next.replace(currentDirectOriginalRe, `function ${fn}(e){let ${rootVar}=${rootExpr},${hostVar}=e?.hostId??${rootVar},{data:${dataVar}}=${dataCall},${disabledVar}=${dataVar}?.requirements?.featureRequirements?.fast_mode===!1;return!${disabledVar}}`);
    patched = true;
  }

  const currentAsyncMatch = next.match(currentAsyncOriginalRe);
  if (currentAsyncMatch) {
    const [, fn, hostManagerVar, hostIdVar, authMethodVar, authMethodFn, queryVar] = currentAsyncMatch;
    next = next.replace(currentAsyncOriginalRe, `async function ${fn}(${hostManagerVar},${hostIdVar}){let ${authMethodVar}=await ${authMethodFn}(${hostManagerVar},${hostIdVar});return(await ${hostManagerVar}.query.fetch(${queryVar},{authMethod:${authMethodVar},hostId:${hostIdVar}})).requirements?.featureRequirements?.fast_mode!==!1}`);
    patched = true;
  }

  const currentCachedAsyncMatch = next.match(currentCachedAsyncOriginalRe);
  if (currentCachedAsyncMatch) {
    const [, fn, hostManagerVar, hostIdVar, authMethodVar, authMethodFn, requirementsVar, requirementsFn, queryVar] = currentCachedAsyncMatch;
    next = next.replace(currentCachedAsyncOriginalRe, `async function ${fn}(${hostManagerVar},${hostIdVar}){let ${authMethodVar}=await ${authMethodFn}(${hostManagerVar},${hostIdVar});let ${requirementsVar}=await ${requirementsFn}(${hostIdVar},{priority:\`critical\`});return ${hostManagerVar}.query.setData(${queryVar},{authMethod:${authMethodVar},hostId:${hostIdVar}},${requirementsVar}),${requirementsVar}.requirements?.featureRequirements?.fast_mode!==!1}`);
    patched = true;
  }

  const currentManagerCachedAsyncMatch = next.match(currentManagerCachedAsyncOriginalRe);
  if (currentManagerCachedAsyncMatch) {
    const [, fn, hostManagerVar, hostIdVar, authMethodVar, authMethodFn, requirementsVar, requirementsFn, queryVar] = currentManagerCachedAsyncMatch;
    next = next.replace(currentManagerCachedAsyncOriginalRe, `async function ${fn}(${hostManagerVar},${hostIdVar}){let ${authMethodVar}=await ${authMethodFn}(${hostManagerVar},${hostIdVar});let ${requirementsVar}=await ${requirementsFn}(${hostManagerVar},${hostIdVar},{priority:\`critical\`});return ${hostManagerVar}.query.setData(${queryVar},{authMethod:${authMethodVar},hostId:${hostIdVar}},${requirementsVar}),${requirementsVar}.requirements?.featureRequirements?.fast_mode!==!1}`);
    patched = true;
  }

  if (/canUseFastMode:!1/.test(next)) {
    const splitNext = next.replace(currentSplitConditionRe, 'if($2){');
    if (splitNext === next) {
      process.stderr.write('split-gate-target-not-found\n');
      process.exit(2);
    }
    next = splitNext;
    patched = true;
  }
}

if (!patched) {
  process.stderr.write('fast-mode-patch-target-not-found\n');
  process.exit(2);
}
fs.writeFileSync(file, next);
process.stdout.write('patched');
