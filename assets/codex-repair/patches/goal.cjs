const fs = require('node:fs');
const [composerFileArg, slashFileArg] = process.argv.slice(2);
const hasComposerFile = composerFileArg && composerFileArg !== '__none__';
const composerFile = hasComposerFile ? composerFileArg : null;
const slashFile = slashFileArg && slashFileArg !== '__none__' ? slashFileArg : composerFile;
const composerText = composerFile ? fs.readFileSync(composerFile, 'utf8') : '';
const slashText = slashFile ? fs.readFileSync(slashFile, 'utf8') : '';

const goalPatchedRe = /(\w+)=([A-Za-z_$][\w$]*)!==`cloud`(?:&&!?\w+)?,(\w+)=([^,]+),/;
const currentSplitGoalPatchedRe = /(\w+)=([A-Za-z_$][\w$]*)!==`cloud`&&!\w+,(\w+)=([^,]+),(\w+)=([^,]+),/;
const goalOriginalRe = /(\w+)=([A-Za-z_$][\w$]*)\(`3074100722`\)&&([A-Za-z_$][\w$]*)\((\w+)\?\.config,`goals`\)===!0&&(\w+)!==`cloud`,(\w+)=([^,]+),/;
const currentGoalOriginalRe = /(\w+)=([A-Za-z_$][\w$]*)\(`3074100722`\)&&([A-Za-z_$][\w$]*)\((\w+)\?\.config,`goals`\)===!0&&(\w+)!==`cloud`(&&!\w+)?,(\w+)=([^,]+),/;
const slashOriginal = 'function Nx(e,t){let n=t.trim();if(n.length===0)return e;let r=new Map;return e.forEach(e=>{let t=e.group??null;r.has(t)||r.set(t,r.size)}),(0,Tx.default)(e.map(e=>({command:e,score:zi(e.title,n)})).filter(e=>e.score>0),[e=>r.get(e.command.group??null)??2**53-1,e=>-e.score,e=>e.command.title]).map(e=>e.command)}';
const slashPatched = 'function Nx(e,t){let n=t.trim().replace(/^\\/+/,"");if(n.length===0)return e;let r=new Map;return e.forEach(e=>{let t=e.group??null;r.has(t)||r.set(t,r.size)}),(0,Tx.default)(e.map(e=>({command:e,score:Math.max(zi(e.title,n),zi(e.id,n))})).filter(e=>e.score>0),[e=>r.get(e.command.group??null)??2**53-1,e=>-e.score,e=>e.command.title]).map(e=>e.command)}';
const slashOriginalRe = /function (\w+)\(e,t\)\{let (\w+)=t\.trim\(\);if\(\2\.length===0\)return e;let (\w+)=new Map;return e\.forEach\(e=>\{let t=e\.group\?\?null;\3\.has\(t\)\|\|\3\.set\(t,\3\.size\)\}\),\(0,([A-Za-z_$][\w$]*)\.default\)\(e\.map\(e=>\(\{command:e,score:([A-Za-z_$][\w$]*)\(e\.title,\2\)\}\)\)\.filter\(e=>e\.score>0\),\[e=>\3\.get\(e\.command\.group\?\?null\)\?\?2\*\*53-1,e=>-e\.score,e=>e\.command\.title\]\)\.map\(e=>e\.command\)\}/;
const slashPatchedRe = /score:Math\.max\([A-Za-z_$][\w$]*\(e\.title,\w+\),[A-Za-z_$][\w$]*\(e\.id,\w+\)\)/;
const cmdkSlashRe = /cmdk-item/;
const cmdkKeywordSearchRe = /keywords:\w+|keywords,\.\.\./;
const goalCommandRe = /id:`goal`,title:[^,]+,description:[^,]+,requiresEmptyComposer:!1,[^}]*enabled:[^,]+/;
const currentGoalAlreadyOpenRe = /[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*!==`cloud`&&!?[A-Za-z_$][\w$]*,[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*\),[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*\)\?\?!1/;

let nextComposer = composerText;
let nextSlash = slashText;
let changedComposer = false;
let changedSlash = false;

if (!nextSlash.includes(slashPatched) && !slashPatchedRe.test(nextSlash)) {
  const slashMatch = nextSlash.match(slashOriginalRe);
  if (slashMatch) {
    const [, fn, queryVar, groupOrderVar, sortByVar, scoreFn] = slashMatch;
    nextSlash = nextSlash.replace(slashOriginalRe, `function ${fn}(e,t){let ${queryVar}=t.trim().replace(/^\\/+/,"");if(${queryVar}.length===0)return e;let ${groupOrderVar}=new Map;return e.forEach(e=>{let t=e.group??null;${groupOrderVar}.has(t)||${groupOrderVar}.set(t,${groupOrderVar}.size)}),(0,${sortByVar}.default)(e.map(e=>({command:e,score:Math.max(${scoreFn}(e.title,${queryVar}),${scoreFn}(e.id,${queryVar}))})).filter(e=>e.score>0),[e=>${groupOrderVar}.get(e.command.group??null)??2**53-1,e=>-e.score,e=>e.command.title]).map(e=>e.command)}`);
    changedSlash = true;
  } else if (nextSlash.includes(slashOriginal)) {
    nextSlash = nextSlash.replace(slashOriginal, slashPatched);
    changedSlash = true;
  } else if (cmdkSlashRe.test(nextSlash) && (cmdkKeywordSearchRe.test(nextSlash) || nextSlash.includes('keywords:r'))) {
    // Codex 26.519+ moved slash filtering to cmdk keywords; command id matching is already handled there.
  } else if (nextSlash.includes('requiresEmptyComposer') &&
             /\w\.getSearchQuery\?\.\(/.test(nextSlash) &&
             /Math\.max\([A-Za-z_$][\w$]*\(e\.title,[A-Za-z_$][\w$]*\),[A-Za-z_$][\w$]*\(e\.id,[A-Za-z_$][\w$]*\),\.\.\.\(e\.searchAliases\?\?\[\]\)\.map/.test(nextSlash)) {
    // Codex 26.831+ splits the command registry (app-primary) from the unified scorer (app-initial);
    // the scorer already matches command ids and search aliases, so /goal is searchable by id.
  } else if (nextSlash.includes('id:`goal`') &&
             nextSlash.includes('getSearchQuery') &&
             /Math\.max\([A-Za-z_$][\w$]*\(e\.title,[A-Za-z_$][\w$]*\),[A-Za-z_$][\w$]*\(e\.id,[A-Za-z_$][\w$]*\),\.\.\.\(e\.searchAliases\?\?\[\]\)\.map/.test(nextSlash)) {
    // Current unified command registry scores title, id, and aliases, so /goal is already searchable by id.
  } else if (nextSlash.includes('sourceMappingURL=slash-command-item') &&
             !nextSlash.includes('score:') &&
             !nextSlash.includes('e.command.group??null')) {
    // Codex 26.609.4994+ keeps this chunk but moved the legacy slash scorer elsewhere.
    // The composer gate below is the required Goal enablement patch for this shape.
  } else {
    process.stderr.write('slash-match-patch-target-not-found\n');
    process.exit(2);
  }
}

if (hasComposerFile) {
  if (goalOriginalRe.test(nextComposer)) {
    nextComposer = nextComposer.replace(goalOriginalRe, (_match, goalGateVar, _statsigFn, _configAccessFn, _configVar, modeVar, hasGoalVar, hasGoalExpr) => `${goalGateVar}=${modeVar}!==\`cloud\`,${hasGoalVar}=${hasGoalExpr},`);
    changedComposer = true;
  } else if (currentGoalOriginalRe.test(nextComposer)) {
    nextComposer = nextComposer.replace(currentGoalOriginalRe, (_match, goalGateVar, _statsigFn, _configAccessFn, _configVar, modeVar, sideChatGuard = '', hasGoalVar, hasGoalExpr) => `${goalGateVar}=${modeVar}!==\`cloud\`${sideChatGuard},${hasGoalVar}=${hasGoalExpr},`);
    changedComposer = true;
  } else if (!(goalPatchedRe.test(nextComposer) ||
               currentSplitGoalPatchedRe.test(nextComposer) ||
               currentGoalAlreadyOpenRe.test(nextComposer) ||
               (goalCommandRe.test(nextComposer) && nextComposer.includes('threadGoalObjective')))) {
    process.stderr.write('goal-patch-target-not-found\n');
    process.exit(2);
  }
}

if (!changedComposer && !changedSlash) {
  process.stdout.write('already-patched');
  process.exit(0);
}
if (changedComposer && composerFile) fs.writeFileSync(composerFile, nextComposer);
if (changedSlash) fs.writeFileSync(slashFile, nextSlash);
process.stdout.write('patched');
