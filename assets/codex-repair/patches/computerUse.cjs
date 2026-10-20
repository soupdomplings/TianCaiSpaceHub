const fs = require('node:fs');
const [availabilityFile, installFlowFile, setupFile] = process.argv.slice(2);
let changed = false;

function read(file) {
  return fs.readFileSync(file, 'utf8');
}

function writeIfChanged(file, before, after) {
  if (after !== before) {
    fs.writeFileSync(file, after);
    changed = true;
  }
}

function patchComputerUseAvailability(file) {
  const before = read(file);
  if (!before.includes('featureName:`computer_use`')) {
    process.stderr.write('computer-use-availability-target-not-found\n');
    process.exit(2);
  }

  let after = before;
  after = after.replace(/=[A-Za-z_$][\w$]*\(`1506311413`\)/, '=!0');
  after = after.replace(
    /(featureName:`computer_use`[^;]+;let )([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*\),/,
    '$1$2={enabled:!0,isLoading:!1},'
  );

  // The browser-use patch runs first and its inAppConfigV3 replacement already rewrites this
  // same declaration, appending a /*CODEX_BROWSER_IN_APP_CONFIG_V3*/ marker between the object
  // literal and the following comma. So on a re-patch of an already-patched package the second
  // replace above has nothing left to do and the witness must tolerate that marker, otherwise
  // a correctly patched build is reported as target-not-found.
  const availabilityPatchedRe = /featureName:`computer_use`[^;]+;let [A-Za-z_$][\w$]*=\{enabled:!0,isLoading:!1\}(?:\/\*[^*]*\*\/)?,/;
  if (after === before && !availabilityPatchedRe.test(before)) {
    process.stderr.write('computer-use-availability-patch-target-not-found\n');
    process.exit(2);
  }
  writeIfChanged(file, before, after);
}

function patchComputerUseInstallFlow(file) {
  if (!file || file === '__none__') {
    return;
  }
  const before = read(file);
  if (!before.includes('openPluginInstall') ||
      (!before.includes('installPlugin:async') && !before.includes('install-plugin'))) {
    process.stderr.write('computer-use-install-flow-target-not-found\n');
    process.exit(2);
  }

  let after = before.replace(
    /([A-Za-z_$][\w$]*)=![A-Za-z_$][\w$]*\.isLoading&&[A-Za-z_$][\w$]*\.enabled,(?=[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\.available,[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\.available,)/,
    '$1=!0,'
  );
  after = after.replace(
    /([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*\.available,(?=[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\.available,[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\.available,)/,
    '$1=!0,'
  );

  if (after === before &&
      !/featureName:`computer_use`[\s\S]*?=!0,[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\.available,[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\.available,/.test(before) &&
      !/=!0,[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\.available,[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\.available,/.test(before)) {
    process.stderr.write('computer-use-install-flow-patch-target-not-found\n');
    process.exit(2);
  }
  writeIfChanged(file, before, after);
}

function patchComputerUseSetup(file) {
  const before = read(file);
  if (!before.includes('showComputerUseSetup')) {
    process.stderr.write('computer-use-setup-target-not-found\n');
    process.exit(2);
  }

  const after = before.replace(/=[A-Za-z_$][\w$]*\(`1506311413`\)/, '=!0');
  if (after === before && before.includes('1506311413')) {
    process.stderr.write('computer-use-setup-patch-target-not-found\n');
    process.exit(2);
  }
  writeIfChanged(file, before, after);
}

patchComputerUseAvailability(availabilityFile);
patchComputerUseInstallFlow(installFlowFile);
patchComputerUseSetup(setupFile);

process.stdout.write(changed ? 'patched' : 'already-patched');
