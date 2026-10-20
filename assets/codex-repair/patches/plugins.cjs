const fs = require('node:fs');
const [sidebarFile, skillsFile, detailFile, pageAuthFile] = process.argv.slice(2);
let changed = false;

function hasFile(file) {
  return typeof file === 'string' && file.length > 0 && file !== '__none__' && fs.existsSync(file);
}

function rewriteFile(label, file, patchedRe, originalRe, replacement) {
  const text = fs.readFileSync(file, 'utf8');
  if (patchedRe.test(text)) return;
  const next = text.replace(originalRe, replacement);
  if (next === text) {
    process.stderr.write(`${label}-target-not-found\n`);
    process.exit(2);
  }
  fs.writeFileSync(file, next);
  changed = true;
}

function patchOldPluginGates() {
  rewriteFile(
    'plugin-sidebar-gate',
    sidebarFile,
    /\{authMethod:(\w+)\}=([A-Za-z_$][\w$]*)\(\),(\w+)=([A-Za-z_$][\w$]*)\(`533078438`\),(\w+)=!1,(\w+)=e&&\3&&\5,(\w+)=([A-Za-z_$][\w$]*)\(\{hostId:([A-Za-z_$][\w$]*)\}\),(\w+)=e&&\7&&!\5,/,
    /\{authMethod:(\w+)\}=([A-Za-z_$][\w$]*)\(\),(\w+)=([A-Za-z_$][\w$]*)\(`533078438`\),(\w+)=([A-Za-z_$][\w$]*)\(\1\),(\w+)=e&&\3&&\5,(\w+)=([A-Za-z_$][\w$]*)\(\{hostId:([A-Za-z_$][\w$]*)\}\),(\w+)=e&&\8&&!\5,/,
    (_match, authMethodVar, authHook, flagVar, featureFlagHook, apiKeyGateVar, _apiKeyGateHook, disabledVar, availabilityVar, availabilityHook, hostIdVar, enabledVar) =>
      `{authMethod:${authMethodVar}}=${authHook}(),${flagVar}=${featureFlagHook}(\`533078438\`),${apiKeyGateVar}=!1,${disabledVar}=e&&${flagVar}&&${apiKeyGateVar},${availabilityVar}=${availabilityHook}({hostId:${hostIdVar}}),${enabledVar}=e&&${availabilityVar}&&!${apiKeyGateVar},`
  );

  rewriteFile(
    'plugin-skills-page-gate',
    skillsFile,
    /let (\w+)=!1,(\w+),(\w+);if\(e\[(\d+)\]!==(\w+)\|\|e\[(\d+)\]!==\1\|\|e\[(\d+)\]!==(\w+)\?/,
    /let (\w+)=(\w+),(\w+),(\w+);if\(e\[(\d+)\]!==(\w+)\|\|e\[(\d+)\]!==\1\|\|e\[(\d+)\]!==(\w+)\?/,
    (_match, pluginAuthBlockedVar, _sourceVar, effectFnVar, effectDepsVar, slotA, deepLinkBlockedVar, slotB, slotC, toastApiVar) =>
      `let ${pluginAuthBlockedVar}=!1,${effectFnVar},${effectDepsVar};if(e[${slotA}]!==${deepLinkBlockedVar}||e[${slotB}]!==${pluginAuthBlockedVar}||e[${slotC}]!==${toastApiVar}?`
  );

  rewriteFile(
    'plugin-detail-gate',
    detailFile,
    /\{authMethod:(\w+)\}=([A-Za-z_$][\w$]*)\(\);if\(!1\)\{let (\w+);return/,
    /\{authMethod:(\w+)\}=([A-Za-z_$][\w$]*)\(\);if\(([A-Za-z_$][\w$]*)\(\1\)\)\{let (\w+);return/,
    (_match, authMethodVar, authHook, _isAuthBlockedHook, redirectElementVar) =>
      `{authMethod:${authMethodVar}}=${authHook}();if(!1){let ${redirectElementVar};return`
  );
}

function patchPluginPageAuth(file) {
  const text = fs.readFileSync(file, 'utf8');
  if (!text.includes('openPluginInstall') || !text.includes('authMethod:')) {
    process.stderr.write('plugin-page-auth-target-not-found\n');
    process.exit(2);
  }
  const migratedPatchedRe = /\{authMethod:[A-Za-z_$][\w$]*\}=[A-Za-z_$][\w$]*\(\),[^;]{0,2500}?[A-Za-z_$][\w$]*=!1,[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\.kind===`manage`,/;
  if (/\{authMethod:[A-Za-z_$][\w$]*\}=[A-Za-z_$][\w$]*\(\),(?:\{data:[A-Za-z_$][\w$]*\}=[A-Za-z_$][\w$]*\(\),)?[A-Za-z_$][\w$]*=!1,/.test(text) || migratedPatchedRe.test(text)) return;

  const originalRe = /\{authMethod:([A-Za-z_$][\w$]*)\}=([A-Za-z_$][\w$]*)\(\),((?:\{data:[A-Za-z_$][\w$]*\}=[A-Za-z_$][\w$]*\(\),)?)([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*)\(\1\),/;
  let next = text.replace(originalRe, (_match, authMethodVar, authHook, middle, blockedVar) =>
    `{authMethod:${authMethodVar}}=${authHook}(),${middle}${blockedVar}=!1,`
  );
  if (next === text) {
    const migratedOriginalRe = /\{authMethod:([A-Za-z_$][\w$]*)\}=([A-Za-z_$][\w$]*)\(\),([^;]{0,2500}?)([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*)\(\1\),([A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\.kind===`manage`,)/;
    next = text.replace(migratedOriginalRe, (_match, authMethodVar, authHook, middle, blockedVar, _blockedHook, tail) =>
      `{authMethod:${authMethodVar}}=${authHook}(),${middle}${blockedVar}=!1,${tail}`
    );
  }
  if (next === text) {
    process.stderr.write('plugin-page-auth-patch-target-not-found\n');
    process.exit(2);
  }
  fs.writeFileSync(file, next);
  changed = true;
}

const oldFiles = [sidebarFile, skillsFile, detailFile];
const oldFileCount = oldFiles.filter(hasFile).length;
if (oldFileCount === 3) {
  patchOldPluginGates();
} else if (oldFileCount > 0 && !hasFile(pageAuthFile)) {
  process.stderr.write('plugin-old-gates-incomplete\n');
  process.exit(2);
}

if (hasFile(pageAuthFile)) {
  patchPluginPageAuth(pageAuthFile);
}

if (oldFileCount === 0 && !hasFile(pageAuthFile)) {
  // No legacy or current auth gate present in this build: the plugin auth gate
  // was removed upstream and install is already open. Treat as already-patched.
  process.stdout.write('already-patched');
  process.exit(0);
}

process.stdout.write(changed ? 'patched' : 'already-patched');
