const fs = require('node:fs');
const file = process.argv[2];
const text = fs.readFileSync(file, 'utf8');
const marker = 'CODEX_CUA_WINDOWS_SURFACE_V1';

// Current Desktop bundles carry two independent Darwin-only checks: one for
// exposing the computer-use plugin and one for generating the CUA surface list.
// The Windows helper is supplied by the local CUA runtime, so both checks must
// admit win32 before the plugin can expose the window-based cua.computer API.
// Desktop 26.924 renamed every minified local in both gates without changing
// their shape, so the anchors match the gate STRUCTURE with capture groups and
// rewrite the patched forms with the bundle's own identifiers. Backreferences
// pin repeated locals (the platform object, the feature flags object and the
// browser-use state object) so lookalike code cannot satisfy a pattern.
const id = '[A-Za-z_$][\\w$]*';
const originalPluginGateRe = new RegExp(`if\\(!(${id})\\.installed\\|\\|(${id})==null\\|\\|(${id})&&(${id})\\.platform!==\`darwin\`\\)return null;`, 'g');
const patchedPluginGateRe = new RegExp(`if\\(!(${id})\\.installed\\|\\|(${id})==null\\|\\|(${id})&&\\((${id})\\.platform!==\`darwin\`&&\\4\\.platform!==\`win32\`\\)\\)return null;`, 'g');
const originalSurfaceGateRe = new RegExp(`(${id})=(${id})&&(${id})\\.platform===\`darwin\`&&(${id})\\.computerUse&&(${id})\\.enabled&&\\5\\.paths\\.serviceAppPath!=null`, 'g');
const modernPatchedSurfaceGateRe = new RegExp(`(${id})=(${id})&&(${id})\\.computerUse&&\\((${id})\\.platform===\`darwin\`&&(${id})\\.enabled&&\\5\\.paths\\.serviceAppPath!=null\\|\\|\\4\\.platform===\`win32\`\\)`, 'g');
const legacyPatchedSurfaceGateRe = new RegExp(`(${id})=(${id})&&\\((${id})\\.platform===\`darwin\`&&(${id})\\.computerUse&&(${id})\\.enabled&&\\5\\.paths\\.serviceAppPath!=null\\|\\|\\3\\.platform===\`win32\`&&\\4\\.computerUse&&\\4\\.computerUseNodeRepl\\)`, 'g');
const unversionedPatchedSurfaceGateRe = new RegExp(`(${id})=(${id})&&\\((${id})\\.platform===\`darwin\`&&(${id})\\.computerUse&&(${id})\\.enabled&&\\5\\.paths\\.serviceAppPath!=null\\|\\|\\3\\.platform===\`win32\`&&\\4\\.computerUse\\)`, 'g');
// Desktop 26.917 removes computerUseNodeRepl. Its shared readiness result f
// already requires the enabled CUA plugin, both Node paths and mcpToolExposure.
const modernReadinessRe = /return t\.browserUseTinysky&&!o&&a\.nodePath!=null&&a\.nodeReplPath!=null&&[A-Za-z_$][\w$]*\.[A-Za-z_$][\w$]*\(e,`mcpToolExposure`\)&&s\?\.plugin\.installed===!0&&s\.plugin\.enabled&&s\.plugin\.availability===`AVAILABLE`/g;
const patchedSurfaceGateRes = [modernPatchedSurfaceGateRe, legacyPatchedSurfaceGateRe, unversionedPatchedSurfaceGateRe];
// Do not infer the host layout from a dependency inserted by a previous patch.
const layoutText = patchedSurfaceGateRes.reduce((source, re) => source.replace(re, ''), text);
const legacyLayout = layoutText.includes('computerUseNodeRepl');
const modernLayout = !legacyLayout && [...layoutText.matchAll(modernReadinessRe)].length === 1;

function buildPluginGate(p, i, a, e) {
  return `if(!${p}.installed||${i}==null||${a}&&(${e}.platform!==\`darwin\`&&${e}.platform!==\`win32\`))return null;/*${marker}*/`;
}

function buildSurfaceGate(layout, result, flag, platform, features, state) {
  const darwinTail = `${state}.enabled&&${state}.paths.serviceAppPath!=null`;
  if (layout === 'modern') {
    return `${result}=${flag}&&${features}.computerUse&&(${platform}.platform===\`darwin\`&&${darwinTail}||${platform}.platform===\`win32\`)`;
  }
  return `${result}=${flag}&&(${platform}.platform===\`darwin\`&&${features}.computerUse&&${darwinTail}||${platform}.platform===\`win32\`&&${features}.computerUse&&${features}.computerUseNodeRepl)`;
}

function count(value, source = text) {
  let total = 0;
  let cursor = 0;
  while ((cursor = source.indexOf(value, cursor)) !== -1) {
    total += 1;
    cursor += value.length;
  }
  return total;
}

function countRe(re, source = text) {
  return (source.match(re) || []).length;
}

const pluginCount = countRe(originalPluginGateRe);
const surfaceCount = countRe(originalSurfaceGateRe);
const markerCount = count(marker);
const patchedPluginCount = countRe(patchedPluginGateRe);
const patchedSurfaceCounts = patchedSurfaceGateRes.map(re => countRe(re));
const patchedSurfaceCount = patchedSurfaceCounts.reduce((total, value) => total + value, 0);
if (markerCount || patchedPluginCount || patchedSurfaceCount) {
  if (markerCount === 1 && patchedPluginCount === 1 &&
      patchedSurfaceCount === 1 && pluginCount === 0 && surfaceCount === 0 &&
      (legacyLayout || modernLayout)) {
    const expectedLayout = modernLayout ? 'modern' : 'legacy';
    const currentGateIndex = patchedSurfaceCounts.findIndex(value => value === 1);
    const expectedGateIndex = modernLayout ? 0 : 1;
    if (currentGateIndex === expectedGateIndex) {
      process.stdout.write('already-patched');
    } else {
      // modern captures (result, flag, features, platform, state); the legacy
      // and unversioned forms capture (result, flag, platform, features, state).
      const next = text.replace(patchedSurfaceGateRes[currentGateIndex],
        (match, result, flag, first, second, state) => {
          const platform = currentGateIndex === 0 ? second : first;
          const features = currentGateIndex === 0 ? first : second;
          return buildSurfaceGate(expectedLayout, result, flag, platform, features, state);
        });
      fs.writeFileSync(file, next);
      process.stdout.write('patched');
    }
    process.exit(0);
  }
  process.stderr.write('incomplete or ambiguous CUA surface patch; refusing to modify the asset\n');
  process.exit(2);
}
if (pluginCount !== 1 || surfaceCount !== 1) {
  process.stderr.write(`current CUA surface anchors not found exactly once: plugin=${pluginCount} surface=${surfaceCount}\n`);
  process.exit(2);
}
if (!legacyLayout && !modernLayout) {
  process.stderr.write('unsupported or ambiguous CUA readiness predicate; refusing to modify the asset\n');
  process.exit(2);
}

const layout = modernLayout ? 'modern' : 'legacy';
const expectedSurfaceGateRe = modernLayout ? modernPatchedSurfaceGateRe : legacyPatchedSurfaceGateRe;
const next = text
  .replace(originalPluginGateRe, (match, p, i, a, e) => buildPluginGate(p, i, a, e))
  .replace(originalSurfaceGateRe, (match, result, flag, platform, features, state) =>
    buildSurfaceGate(layout, result, flag, platform, features, state));

if (count(marker, next) !== 1 || countRe(patchedPluginGateRe, next) !== 1 ||
    countRe(expectedSurfaceGateRe, next) !== 1 || countRe(originalPluginGateRe, next) !== 0 ||
    countRe(originalSurfaceGateRe, next) !== 0) {
  process.stderr.write('current CUA surface patch verification failed\n');
  process.exit(3);
}

fs.writeFileSync(file, next);
process.stdout.write('patched');
