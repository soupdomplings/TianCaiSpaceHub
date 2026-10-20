const fs = require('node:fs');
const file = process.argv[2];
const models = [...new Set(process.argv.slice(3).flatMap(value => value.split(',')).map(value => value.trim()).filter(Boolean))];
if (models.length === 0) {
  process.stderr.write('custom-model-list-empty\n');
  process.exit(2);
}

const marker = 'CODEX_CUSTOM_MODELS_V1';
const text = fs.readFileSync(file, 'utf8');
if (text.includes(marker)) {
  const lists = [...text.matchAll(/\/\*CODEX_CUSTOM_MODELS_V1\*\/(\[[^\]\r\n]*\])\.includes\(([$A-Za-z_][$\w]*)\.model\)\|\|/g)];
  if (text.split(marker).length !== 2 || lists.length !== 1) {
    process.stderr.write('custom-model-existing-patch-ambiguous\n');
    process.exit(2);
  }
  let previous;
  try { previous = JSON.parse(lists[0][1]); } catch { previous = null; }
  if (!Array.isArray(previous) || !previous.every(value => typeof value === 'string')) {
    process.stderr.write('custom-model-existing-list-invalid\n');
    process.exit(2);
  }
  if (JSON.stringify(previous) === JSON.stringify(models)) {
    process.stdout.write('already-patched');
  } else {
    const start = lists[0].index + `/*${marker}*/`.length;
    fs.writeFileSync(file, text.slice(0, start) + JSON.stringify(models) + text.slice(start + lists[0][1].length));
    process.stdout.write('patched');
  }
  process.exit(0);
}

const visibilityPatterns = [
  {
    // Desktop 26.1002 adds API-key access-program checks. Keep the denial
    // branch outside the local visibility override.
    re: /return (([$A-Za-z_][$\w]*)===`apikey`&&!([$A-Za-z_][$\w]*)&&([$A-Za-z_][$\w]*)!=null&&\4\.length>0&&!\4\.includes\(`standard`\)\?!1:)([$A-Za-z_][$\w]*)\?\.has\(([$A-Za-z_][$\w]*)\.model\)===!0\|\|\6\.model!==`codex-auto-review`&&\(([$A-Za-z_][$\w]*)&&!\6\.hidden\|\|\(([$A-Za-z_][$\w]*)&&!([$A-Za-z_][$\w]*)&&\2!==`amazonBedrock`\?([$A-Za-z_][$\w]*)\.has\(\6\.model\)\|\|\2===`apikey`&&\3&&!\6\.hidden&&\4\?\.some\(([$A-Za-z_][$\w]*)=>\11!==`standard`\)===!0:!\6\.hidden\)\)\}/,
    modelGroup: 6,
    accessGuardGroup: 1,
    isReturn: true,
  },
  {
    re: /return ([$A-Za-z_][$\w]*)\?\.has\(([$A-Za-z_][$\w]*)\.model\)===!0\|\|\2\.model!==`codex-auto-review`&&\(([$A-Za-z_][$\w]*)&&!([$A-Za-z_][$\w]*)&&([$A-Za-z_][$\w]*)!==`amazonBedrock`\?([$A-Za-z_][$\w]*)\.has\(\2\.model\):!\2\.hidden\)\}/,
    modelGroup: 2,
    isReturn: true,
  },
  {
    re: /if\(([$A-Za-z_][$\w]*)\?([$A-Za-z_][$\w]*)\.has\(([$A-Za-z_][$\w]*)\.model\):!\3\.hidden\)\{/,
    modelGroup: 3,
  },
  {
    re: /if\(([$A-Za-z_][$\w]*)\?\.has\(([$A-Za-z_][$\w]*)\.model\)===!0\|\|\(([$A-Za-z_][$\w]*)\?([$A-Za-z_][$\w]*)\.has\(\2\.model\):!\2\.hidden\)\)\{/,
    modelGroup: 2,
  },
  {
    re: /return ([$A-Za-z_][$\w]*)\?\.has\(([$A-Za-z_][$\w]*)\.model\)===!0\|\|\(([$A-Za-z_][$\w]*)(?:&&[$A-Za-z_][$\w]*!==`amazonBedrock`)?\?([$A-Za-z_][$\w]*)\.has\(\2\.model\):!\2\.hidden\)\}/,
    modelGroup: 2,
    isReturn: true,
  },
  {
    // 0.6.24+ inserts a `hasConfiguredModelCatalog&&!model.hidden||` disjunct
    // ahead of the useHiddenModels ternary and wraps that ternary in one more
    // paren layer.
    re: /return ([$A-Za-z_][$\w]*)\?\.has\(([$A-Za-z_][$\w]*)\.model\)===!0\|\|\2\.model!==`codex-auto-review`&&\(([$A-Za-z_][$\w]*)&&!\2\.hidden\|\|\(([$A-Za-z_][$\w]*)&&!([$A-Za-z_][$\w]*)&&([$A-Za-z_][$\w]*)!==`amazonBedrock`\?([$A-Za-z_][$\w]*)\.has\(\2\.model\):!\2\.hidden\)\)\}/,
    modelGroup: 2,
    isReturn: true,
  },
];
const target = visibilityPatterns
  .map(({ re, modelGroup, isReturn, accessGuardGroup }) => ({ match: text.match(re), modelGroup, isReturn, accessGuardGroup }))
  .find(({ match }) => match != null);
const match = target?.match;
if (!match) {
  process.stderr.write('custom-model-visibility-target-not-found\n');
  process.exit(2);
}

const forced = JSON.stringify(models);
const modelVar = match[target.modelGroup];
let replacement;
if (target.isReturn) {
  const accessGuard = target.accessGuardGroup ? match[target.accessGuardGroup] : '';
  const originalCondition = match[0].slice('return '.length + accessGuard.length, -1);
  replacement = `return ${accessGuard}/*${marker}*/${forced}.includes(${modelVar}.model)||(${originalCondition})}`;
} else {
  const originalCondition = match[0].slice(3, -2);
  replacement = `if(/*${marker}*/${forced}.includes(${modelVar}.model)||(${originalCondition})){`;
}
const next = text.replace(match[0], replacement);
if (!next.includes(marker) || !models.every((model) => next.includes(model))) {
  process.stderr.write('custom-model-patch-verification-failed\n');
  process.exit(2);
}
fs.writeFileSync(file, next);
process.stdout.write('patched');
