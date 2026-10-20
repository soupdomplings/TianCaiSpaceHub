const fs = require('node:fs');
const file = process.argv[2];
const text = fs.readFileSync(file, 'utf8');

const copyPatchedMarker = 'codex_windows_bundled_marketplace_copy_fallback';
const sitesPatchedMarker = 'codex_windows_sites_bundled_plugin_available';
const deepResearchPatchedMarker = 'codex_windows_deep_research_bundled_plugin_available';
const imageGuidedAssetsPatchedMarker = 'codex_windows_image_guided_assets_bundled_plugin_available';
let after = text;
let changed = false;

function patchOptionalAvailability(property, originalPredicate, marker) {
  const identifier = '[A-Za-z_$][\\w$]*';
  const prefix = `\\.\\.\\.${identifier}\\.${identifier}\\.${property},isAvailable:`;
  const declarations = [...after.matchAll(new RegExp(prefix, 'g'))];
  if (declarations.length === 0 && !after.includes(marker)) return;
  const original = new RegExp(`${prefix}${originalPredicate}(?=})`, 'g');
  const patched = new RegExp(`${prefix}\\(\\)=>!0/\\*${marker}\\*/,installWhenMissing:!1(?=})`, 'g');
  const originalMatches = [...after.matchAll(original)];
  const patchedMatches = [...after.matchAll(patched)];
  const markerCount = after.split(marker).length - 1;
  if (declarations.length !== 1 ||
      (markerCount === 0 ? originalMatches.length !== 1 :
        markerCount !== 1 || patchedMatches.length !== 1 || originalMatches.length !== 0)) {
    process.stderr.write(`bundled-marketplace-${property}-availability-target-not-found-or-ambiguous\n`);
    process.exit(2);
  }
  if (markerCount === 0) {
    // Publish the optional entry without switching browser engines or installing it.
    after = after.replace(original, () => `${declarations[0][0]}()=>!0/*${marker}*/,installWhenMissing:!1`);
    changed = true;
  }
}

patchOptionalAvailability('pullRequests',
  '\\(\\{features:([A-Za-z_$][\\w$]*)\\}\\)=>\\1\\.codeReviewPlugin',
  'codex_windows_code_review_bundled_plugin_available');
patchOptionalAvailability('unifiedComputerUse',
  '\\(\\{features:([A-Za-z_$][\\w$]*)\\}\\)=>\\1\\.browserUseTinysky&&\\(\\1\\.inAppBrowserUse\\|\\|\\1\\.externalBrowserUse\\|\\|\\1\\.computerUse\\)',
  'codex_windows_unified_cua_bundled_plugin_available');

const hasNativeWindowsCopyFallback =
  (after.includes('copyDirectoryAllowDecryptedDestinationOnEncryptionFailure') &&
   after.includes('windows-file-copy-')) ||
  /platform!==`win32`\)\{await [A-Za-z_$][\w$]*\.default\.cp\([^)]*\{recursive:!0,verbatimSymlinks:!0\}\);return\}/.test(after) ||
  /require\(`\.\/windows-file-copy-[A-Za-z0-9_-]+\.js`\)/.test(after);

if (!after.includes(copyPatchedMarker) && !hasNativeWindowsCopyFallback) {
  const originalRe = /async function ([A-Za-z_$][\w$]*)\(([A-Za-z_$][\w$]*),([A-Za-z_$][\w$]*)\)\{if\(([A-Za-z_$][\w$]*)\.default\.platform===`darwin`\)\{await ([A-Za-z_$][\w$]*)\(`ditto`,\[`--noqtn`,\2,\3\]\);return\}await ([A-Za-z_$][\w$]*)\.default\.cp\(\2,\3,\{recursive:!0,verbatimSymlinks:!0\}\)\}/;
  const match = after.match(originalRe);
  if (!match) {
    process.stderr.write('bundled-marketplace-copy-target-not-found\n');
    process.exit(2);
  }
  const matchIndex = match.index ?? 0;
  const searchStart = Math.max(0, matchIndex - 2500);
  const searchEnd = Math.min(after.length, matchIndex + 2500);
  const nearby = after.slice(searchStart, searchEnd);
  const pathMatch = nearby.match(/\(0,([A-Za-z_$][\w$]*)\.join\)\(/) || after.match(/\(0,([A-Za-z_$][\w$]*)\.join\)\(/);
  if (!pathMatch) {
    process.stderr.write('bundled-marketplace-copy-path-var-not-found\n');
    process.exit(2);
  }

  const [, fn, sourceArg, targetArg, platformVar, dittoFn, fsVar] = match;
  const pathVar = pathMatch[1];
  const fallbackFn = `codexWindowsBundledMarketplaceCopyFallback`;
  const replacement = `async function ${fn}(${sourceArg},${targetArg}){if(${platformVar}.default.platform===\`darwin\`){await ${dittoFn}(\`ditto\`,[\`--noqtn\`,${sourceArg},${targetArg}]);return}try{await ${fsVar}.default.cp(${sourceArg},${targetArg},{recursive:!0,verbatimSymlinks:!0});return}catch(n){if(${platformVar}.default.platform!==\`win32\`)throw n;try{console.warn(\`${copyPatchedMarker}\`,n)}catch{}}async function ${fallbackFn}(e,t){let n=await ${fsVar}.default.lstat(e);if(n.isDirectory()){await ${fsVar}.default.mkdir(t,{recursive:!0});for(let r of await ${fsVar}.default.readdir(e))await ${fallbackFn}((0,${pathVar}.join)(e,r),(0,${pathVar}.join)(t,r));return}if(n.isSymbolicLink()){let r=await ${fsVar}.default.readlink(e);try{await ${fsVar}.default.symlink(r,t)}catch(e){if(e?.code!==\`EEXIST\`)throw e}return}await ${fsVar}.default.mkdir((0,${pathVar}.dirname)(t),{recursive:!0});let r=await ${fsVar}.default.readFile(e);await ${fsVar}.default.writeFile(t,r);try{await ${fsVar}.default.chmod(t,n.mode)}catch{}}await ${fallbackFn}(${sourceArg},${targetArg})}`;
  after = after.replace(originalRe, replacement);
  changed = true;
}

if (!after.includes(sitesPatchedMarker)) {
  const sitesAvailabilityRe = /isAvailable:\(\{features:([A-Za-z_$][\w$]*)\}\)=>\1\.sites/;
  const sitesRetirementRe = /async function [A-Za-z_$][\w$]*\(([A-Za-z_$][\w$]*)\)\{let\{plugins:([A-Za-z_$][\w$]*)\}=await \1\.getUserSavedConfiguration\(\);typeof \2==`object`&&\2&&!Array\.isArray\(\2\)&&Object\.hasOwn\(\2,`sites@openai-bundled`\)&&await \1\.uninstallPlugin\(\{pluginId:`sites@openai-bundled`\}\)\}/g;
  const sitesRetired = [...after.matchAll(sitesRetirementRe)].length === 1 &&
    !/\.\.\.[A-Za-z_$][\w$]*\.[A-Za-z_$][\w$]*\.sites\b/.test(after);
  if (sitesAvailabilityRe.test(after)) {
    after = after.replace(sitesAvailabilityRe, `isAvailable:()=>!0/*${sitesPatchedMarker}*/`);
    changed = true;
  } else if (!sitesRetired) {
    process.stderr.write('bundled-marketplace-sites-availability-target-not-found\n');
    process.exit(2);
  }
}

if (!after.includes(deepResearchPatchedMarker)) {
  const deepResearchAvailabilityRe = /isAvailable:\(\{features:([A-Za-z_$][\w$]*)\}\)=>\1\.deepResearch/;
  if (!deepResearchAvailabilityRe.test(after)) {
    process.stderr.write('bundled-marketplace-deep-research-availability-target-not-found\n');
    process.exit(2);
  }
  after = after.replace(deepResearchAvailabilityRe, `isAvailable:()=>!0/*${deepResearchPatchedMarker}*/`);
  changed = true;
}

const imageGuidedAssetsIdentifier = '[A-Za-z_$][\\w$]*';
const imageGuidedAssetsDeclarationRe = new RegExp(
  `\\.\\.\\.${imageGuidedAssetsIdentifier}\\.${imageGuidedAssetsIdentifier}\\.imageGuidedAssets,`, 'g');
const imageGuidedAssetsDeclarations = [...after.matchAll(imageGuidedAssetsDeclarationRe)];
const imageGuidedAssetsPrefix =
  `\\.\\.\\.${imageGuidedAssetsIdentifier}\\.${imageGuidedAssetsIdentifier}\\.imageGuidedAssets,autoInstallOptOutKey:[^,]+,isAvailable:`;
const imageGuidedAssetsOriginalRe = new RegExp(
  `(${imageGuidedAssetsPrefix})\\(\\{features:(${imageGuidedAssetsIdentifier})\\}\\)=>\\2\\.imageGuidedAssets(?=})`, 'g');
const imageGuidedAssetsPatchedRe = new RegExp(
  `${imageGuidedAssetsPrefix}\\(\\)=>!0/\\*${imageGuidedAssetsPatchedMarker}\\*/,installWhenMissing:!1(?=})`, 'g');
const imageGuidedAssetsOriginalMatches = [...after.matchAll(imageGuidedAssetsOriginalRe)];
const imageGuidedAssetsPatchedMatches = [...after.matchAll(imageGuidedAssetsPatchedRe)];
const imageGuidedAssetsMarkerCount = after.split(imageGuidedAssetsPatchedMarker).length - 1;
if (imageGuidedAssetsDeclarations.length === 0 && imageGuidedAssetsMarkerCount === 0) {
  // Older Desktop builds do not ship this optional plugin.
} else if (imageGuidedAssetsDeclarations.length !== 1 ||
    (imageGuidedAssetsMarkerCount === 0 ? imageGuidedAssetsOriginalMatches.length !== 1 :
      imageGuidedAssetsMarkerCount !== 1 || imageGuidedAssetsPatchedMatches.length !== 1 ||
      imageGuidedAssetsOriginalMatches.length !== 0)) {
  process.stderr.write('bundled-marketplace-image-guided-assets-availability-target-not-found-or-ambiguous\n');
  process.exit(2);
} else if (imageGuidedAssetsMarkerCount === 0) {
  after = after.replace(imageGuidedAssetsOriginalRe,
    (match, prefix) => `${prefix}()=>!0/*${imageGuidedAssetsPatchedMarker}*/,installWhenMissing:!1`);
  changed = true;
}

if (changed) {
  fs.writeFileSync(file, after);
  process.stdout.write('patched');
} else {
  process.stdout.write('already-patched');
}
