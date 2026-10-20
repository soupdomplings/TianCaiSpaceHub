const fs = require('node:fs');
const [featureHookFile, sidebarAvailabilityFile, desktopFeatureSenderFile, desktopFeatureMainFile] = process.argv.slice(2);
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

function patchFeatureHook(file) {
  const before = read(file);
  if (!before.includes('featureName:`browser_use_external`') || !before.includes('featureName:`browser_use`')) {
    process.stderr.write('browser-use-feature-hook-target-not-found\n');
    process.exit(2);
  }

  const currentInAppPairOriginalRe = /let ([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*)\(([A-Za-z_$][\w$]*),([A-Za-z_$][\w$]*)\)\.isCapable,([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*)\(`410262010`\),([A-Za-z_$][\w$]*);/;
  const currentInAppPairPatchedRe = /let [A-Za-z_$][\w$]*=!0,[A-Za-z_$][\w$]*=\([A-Za-z_$][\w$]*\(`410262010`\),!0\),[A-Za-z_$][\w$]*;\/\*CODEX_BROWSER_IN_APP_GATES_V2\*\//;
  const currentInAppConfigOriginalRe = /let ([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*\),([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*\.runCodexInWsl\),([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*)\(([A-Za-z_$][\w$]*)\),([A-Za-z_$][\w$]*)=\1\.enabled&&!\1\.isLoading,([A-Za-z_$][\w$]*)=\1\.isLoading,([A-Za-z_$][\w$]*)=\2===!0\|\|\3\.kind===`wsl`,([A-Za-z_$][\w$]*);/;
  const currentInAppConfigPatchedRe = /let [A-Za-z_$][\w$]*=\{enabled:!0,isLoading:!1\},[A-Za-z_$][\w$]*=!1,[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*\),[A-Za-z_$][\w$]*=!0,[A-Za-z_$][\w$]*=!1,[A-Za-z_$][\w$]*=!1,[A-Za-z_$][\w$]*;\/\*CODEX_BROWSER_IN_APP_CONFIG_V2\*\//;
  const currentExternalGateOriginalRe = /([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*)\(`410065390`\),([A-Za-z_$][\w$]*);/;
  const currentExternalGatePatchedRe = /[A-Za-z_$][\w$]*=\([A-Za-z_$][\w$]*\(`410065390`\),!0\),[A-Za-z_$][\w$]*;\/\*CODEX_BROWSER_EXTERNAL_GATE_V2\*\//;
  const currentExternalConfigOriginalRe = /let ([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*\),([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*\.runCodexInWsl\),([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*)\(([A-Za-z_$][\w$]*)\),([A-Za-z_$][\w$]*)=\2===!0\|\|\3\.kind===`wsl`,([A-Za-z_$][\w$]*);/;
  const currentExternalConfigPatchedRe = /let [A-Za-z_$][\w$]*=\{enabled:!0,isLoading:!1\},[A-Za-z_$][\w$]*=!1,[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*\),[A-Za-z_$][\w$]*=!1,[A-Za-z_$][\w$]*;\/\*CODEX_BROWSER_EXTERNAL_CONFIG_V2\*\//;
  const hasCurrentCompiledShape = before.includes('browser.in-app') &&
    before.includes('410065390') &&
    before.includes('410262010') &&
    before.includes('.runCodexInWsl');
  const currentCompiledMarkers = [
    'CODEX_BROWSER_IN_APP_GATES_V2',
    'CODEX_BROWSER_IN_APP_CONFIG_V2',
    'CODEX_BROWSER_EXTERNAL_GATE_V2',
    'CODEX_BROWSER_EXTERNAL_CONFIG_V2'
  ];
  // 26.831+ splits each browser_use feature hook across several let statements
  // with React-compiler memo-cache blocks in between, so the hook result, the
  // WSL read, and the derived availability vars no longer sit in one let chain.
  // Match the two split declarations by their stable neighbor expressions and
  // force the WSL availability result separately.
  const inAppConfigV3OriginalRe = /let ([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*\)(,[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*\),([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*\?\.authMethod\?\?null,([A-Za-z_$][\w$]*);)/;
  const externalConfigV3OriginalRe = /let ([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*\)(,[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*\),([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*\|\|[A-Za-z_$][\w$]*===`chrome-extension`,)/;
  const wslResultV3Re = /([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*===!0\|\|[A-Za-z_$][\w$]*\.kind===`wsl`/g;
  const v3ConfigPatched = before.includes('/*CODEX_BROWSER_IN_APP_CONFIG_V3*/') &&
                          before.includes('/*CODEX_BROWSER_EXTERNAL_CONFIG_V3*/');
  const v3ConfigOriginal = inAppConfigV3OriginalRe.test(before) &&
                           externalConfigV3OriginalRe.test(before);
  if (hasCurrentCompiledShape && (
      !(currentInAppPairOriginalRe.test(before) || currentInAppPairPatchedRe.test(before)) ||
      !((currentInAppConfigOriginalRe.test(before) || currentInAppConfigPatchedRe.test(before)) || v3ConfigOriginal || v3ConfigPatched) ||
      !(currentExternalGateOriginalRe.test(before) || currentExternalGatePatchedRe.test(before)) ||
      !((currentExternalConfigOriginalRe.test(before) || currentExternalConfigPatchedRe.test(before)) || v3ConfigOriginal || v3ConfigPatched))) {
    process.stderr.write('browser-use-current-feature-hook-incomplete\n');
    process.exit(2);
  }

  let after = before;
  if (hasCurrentCompiledShape) {
    after = after.replace(
      currentInAppPairOriginalRe,
      'let $1=!0,$5=($6(`410262010`),!0),$7;/*CODEX_BROWSER_IN_APP_GATES_V2*/'
    );
    after = after.replace(
      currentInAppConfigOriginalRe,
      'let $1={enabled:!0,isLoading:!1},$2=!1,$3=$4($5),$6=!0,$7=!1,$8=!1,$9;/*CODEX_BROWSER_IN_APP_CONFIG_V2*/'
    );
    after = after.replace(
      currentExternalGateOriginalRe,
      '$1=($2(`410065390`),!0),$3;/*CODEX_BROWSER_EXTERNAL_GATE_V2*/'
    );
    after = after.replace(
      currentExternalConfigOriginalRe,
      'let $1={enabled:!0,isLoading:!1},$2=!1,$3=$4($5),$6=!1,$7;/*CODEX_BROWSER_EXTERNAL_CONFIG_V2*/'
    );
    after = after.replace(
      inAppConfigV3OriginalRe,
      'let $1={enabled:!0,isLoading:!1}/*CODEX_BROWSER_IN_APP_CONFIG_V3*/$2'
    );
    after = after.replace(
      externalConfigV3OriginalRe,
      'let $1={enabled:!0,isLoading:!1}/*CODEX_BROWSER_EXTERNAL_CONFIG_V3*/$2'
    );
    after = after.replace(
      wslResultV3Re,
      '$1=!1/*CODEX_BROWSER_WSL_RESULT_V3*/'
    );
  }

  after = after.replace(
    /let c=u\(s\),d=a===`chrome-extension`\|\|o&&c\.enabled&&!c\.isLoading,f=a===`chrome-extension`\?!1:c\.isLoading,p;/,
    'let c={enabled:!0,isLoading:!1},d=!0,f=!1,p;'
  );
  after = after.replace(
    /s=t\(c\),d=i\(`410262010`\),f;/,
    's=!0,d=!0,f;'
  );
  after = after.replace(
    /let p=u\(f\),m=o\(e\.runCodexInWsl\),h=p\.enabled&&!p\.isLoading,_=p\.isLoading,v=m===!0,y;/,
    'let p={enabled:!0,isLoading:!1},m=!1,h=!0,_=!1,v=!1,y;'
  );
  after = after.replace(
    /let s=g\(o\),l=i===`chrome-extension`\|\|a&&s\.enabled&&!s\.isLoading,u=i===`chrome-extension`\?!1:s\.isLoading,d;/,
    'let s={enabled:!0,isLoading:!1},l=!0,u=!1,d;'
  );
  after = after.replace(
    /let ([A-Za-z_$][\w$]*)=x\(([A-Za-z_$][\w$]*)\),([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*)===`chrome-extension`\|\|([A-Za-z_$][\w$]*)&&\1\.enabled&&!\1\.isLoading,([A-Za-z_$][\w$]*)=\4===`chrome-extension`\?!1:\1\.isLoading,([A-Za-z_$][\w$]*);/,
    'let $1={enabled:!0,isLoading:!1},$3=!0,$6=!1,$7;'
  );
  after = after.replace(
    /let ([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*\),([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*)===`chrome-extension`\|\|([A-Za-z_$][\w$]*)&&\1\.enabled&&!\1\.isLoading,([A-Za-z_$][\w$]*)=\3===`chrome-extension`\?!1:\1\.isLoading,([A-Za-z_$][\w$]*);/,
    'let $1={enabled:!0,isLoading:!1},$2=!0,$5=!1,$6;'
  );
  after = after.replace(
    /i=n\(m\),a=c\(`410262010`\),l;/,
    'i=!0,a=!0,l;'
  );
  after = after.replace(
    /([A-Za-z_$][\w$]*)=r\(g\),([A-Za-z_$][\w$]*)=u\(`410262010`\),([A-Za-z_$][\w$]*);/,
    '$1=!0,$2=!0,$3;'
  );
  after = after.replace(
    /([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*\),([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*\(`410262010`\),([A-Za-z_$][\w$]*);/,
    '$1=!0,$2=!0,$3;'
  );
  after = after.replace(
    /let u=g\(l\),d=s\(o\.runCodexInWsl\),f=u\.enabled&&!u\.isLoading,p=u\.isLoading,_=d===!0,v;/,
    'let u={enabled:!0,isLoading:!1},d=!1,f=!0,p=!1,_=!1,v;'
  );
  after = after.replace(
    /let ([A-Za-z_$][\w$]*)=x\(([A-Za-z_$][\w$]*)\),([A-Za-z_$][\w$]*)=l\(c\.runCodexInWsl\),([A-Za-z_$][\w$]*)=\1\.enabled&&!\1\.isLoading,([A-Za-z_$][\w$]*)=\1\.isLoading,([A-Za-z_$][\w$]*)=\3===!0,([A-Za-z_$][\w$]*);/,
    'let $1={enabled:!0,isLoading:!1},$3=!1,$4=!0,$5=!1,$6=!1,$7;'
  );
  after = after.replace(
    /let ([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*\),([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*\(c\.runCodexInWsl\),([A-Za-z_$][\w$]*)=\1\.enabled&&!\1\.isLoading,([A-Za-z_$][\w$]*)=\1\.isLoading,([A-Za-z_$][\w$]*)=\2===!0,([A-Za-z_$][\w$]*);/,
    'let $1={enabled:!0,isLoading:!1},$2=!1,$3=!0,$4=!1,$5=!1,$6;'
  );

  if (after === before &&
      !before.includes('let c={enabled:!0,isLoading:!1},d=!0,f=!1,p;') &&
      !before.includes('s=!0,d=!0,f;') &&
      !before.includes('let p={enabled:!0,isLoading:!1},m=!1,h=!0,_=!1,v=!1,y;') &&
      !before.includes('let s={enabled:!0,isLoading:!1},l=!0,u=!1,d;') &&
      !before.includes('i=!0,a=!0,l;') &&
      !before.includes('let u={enabled:!0,isLoading:!1},d=!1,f=!0,p=!1,_=!1,v;') &&
      !/\{enabled:!0,isLoading:!1\},[A-Za-z_$][\w$]*=!0,[A-Za-z_$][\w$]*=!1/.test(before) &&
      !/[A-Za-z_$][\w$]*=!0,[A-Za-z_$][\w$]*=!0,[A-Za-z_$][\w$]*;/.test(before) &&
      !currentCompiledMarkers.every(marker => before.includes(marker)) &&
      !v3ConfigPatched) {
    process.stderr.write('browser-use-feature-hook-patch-target-not-found\n');
    process.exit(2);
  }
  writeIfChanged(file, before, after);
}

function patchSidebarAvailability(file) {
  const before = read(file);
  if (!before.includes('in_app_browser')) {
    process.stderr.write('browser-sidebar-availability-target-not-found\n');
    process.exit(2);
  }

  let after = before.replace(
    /var i=`in_app_browser`,a=t\(n,\(\{get:t\}\)=>\{let\{data:n\}=t\(r,t\(e\)\),a=n\?\.find\(e=>e\.name===i\);return n!=null&&a\?\.enabled!==!1\}\);/,
    'var i=`in_app_browser`,a=t(n,()=>!0);'
  );
  after = after.replace(
    /var ([A-Za-z_$][\w$]*)=`in_app_browser`,([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*)\(([A-Za-z_$][\w$]*),\(\{get:[A-Za-z_$][\w$]*\}\)=>\{let\{data:[A-Za-z_$][\w$]*\}=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*,[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*\)\),[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\?\.find\([A-Za-z_$][\w$]*=>[A-Za-z_$][\w$]*\.name===\1\);return [A-Za-z_$][\w$]*!=null&&[A-Za-z_$][\w$]*\?\.enabled!==!1\}\);/,
    'var $1=`in_app_browser`,$2=$3($4,()=>!0);'
  );
  after = after.replace(
    /([A-Za-z_$][\w$]*)=`in_app_browser`,([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*)\(([A-Za-z_$][\w$]*),\(\{get:[A-Za-z_$][\w$]*\}\)=>\{let\{data:([A-Za-z_$][\w$]*)\}=[\s\S]*?,[A-Za-z_$][\w$]*=\5\?\.find\([A-Za-z_$][\w$]*=>[A-Za-z_$][\w$]*\.name===\1\);return \5!=null&&[A-Za-z_$][\w$]*\?\.enabled!==!1\}\)/,
    '$1=`in_app_browser`,$2=$3($4,()=>!0)'
  );
  after = after.replace(
    /([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*)\(([A-Za-z_$][\w$]*),([A-Za-z_$][\w$]*)\)\.isCapable,([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*)\(`410262010`\)/,
    '$1=!0,$5=($6(`410262010`),!0)/*CODEX_BROWSER_IN_APP_GATES_V2*/'
  );
  // 0.6.24+ prefixes the capability entry with accessPolicy, and on some builds
  // the `410262010` statsig literal no longer sits next to the isCapable read,
  // so anchor on the capability table instead. Strip every requirement key
  // rather than just configFeatures: the capability atom short-circuits to
  // isCapable:!0 only when the whole requirement list comes out empty.
  const modernCapabilityPattern = /("browser\.in-app":\{)(?:accessPolicy:`[\w-]+`,|configFeatures:\[\{key:`in_app_browser`,host:`default`\}\],)+/;
  // Leave an `in_app_browser` marker behind. In the shipped bundle that literal
  // occurs exactly once -- inside the configFeatures entry being stripped -- and
  // both the entry guard above and the target-file discovery in
  // patch_codex_fast_mode_windows_msix.ps1 key on it, so a bare strip would make
  // this patcher unable to find its own target on a second run.
  const modernCapabilityMarker = '/*in_app_browser CODEX_BROWSER_IN_APP_GATES_V2*/';
  const modernCapabilityPatched = before.includes('"browser.in-app":{' + modernCapabilityMarker);
  const beforeModernCapability = after;
  after = after.replace(modernCapabilityPattern, '$1' + modernCapabilityMarker);
  if (after === beforeModernCapability && !modernCapabilityPatched) {
    after = after.replace(
      // The capability-read helper is renamed by the minifier on every release,
      // so match any identifier rather than a fixed one.
      /(name:`browser\.in-app`[\s\S]{0,1800}?)(let|var) ([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*,[A-Za-z_$][\w$]*\)\.isCapable,/g,
      '$1$2 $3=!0,'
    );
  }
  const modernSidebarPatched = modernCapabilityPatched ||
    /name:`browser\.in-app`[\s\S]{0,1800}?(?:let|var) [A-Za-z_$][\w$]*=!0,/.test(before);
  if (after === before &&
       !before.includes('a=t(n,()=>!0)') &&
       !/`in_app_browser`,[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\([A-Za-z_$][\w$]*,\(\)=>!0\)/.test(before) &&
       !/([A-Za-z_$][\w$]*)=!0,([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*)\(`410262010`\)/.test(before) &&
       !before.includes('CODEX_BROWSER_IN_APP_GATES_V2') &&
       !modernSidebarPatched) {
    process.stderr.write('browser-sidebar-availability-patch-target-not-found\n');
    process.exit(2);
  }
  writeIfChanged(file, before, after);
}

function patchDesktopFeatureSender(file) {
  const before = read(file);
  const patchedSenderFragment = 'inAppBrowserUse:!0,inAppBrowserUseAllowed:!0,browserPane:!0,externalBrowserUse:!0,externalBrowserUseAllowed:!0,computerUse:';
  const patchedSenderPattern = /inAppBrowserUse:!0,(?:mcpAppsBrowserUse:[^,}]+,)?inAppBrowserUseAllowed:!0,((?:[A-Za-z_$][\w$]*:[^,}]+,){0,10}?)browserPane:!0,((?:[A-Za-z_$][\w$]*:[^,}]+,){0,8}?)externalBrowserUse:!0,externalBrowserUseAllowed:!0,((?:[A-Za-z_$][\w$]*:[^,}]+,){0,6}?)computerUse:/;
  if (!before.includes('browser_use_availability_resolved') || !before.includes('electron-desktop-features-changed')) {
    process.stderr.write('browser-use-desktop-feature-sender-target-not-found\n');
    process.exit(2);
  }

  let after = before.replace(
    /inAppBrowserUse:[^,}]+,(mcpAppsBrowserUse:[^,}]+,)?inAppBrowserUseAllowed:[^,}]+,((?:[A-Za-z_$][\w$]*:[^,}]+,){0,10}?)browserPane:[^,}]+,((?:[A-Za-z_$][\w$]*:[^,}]+,){0,8}?)externalBrowserUse:[^,}]+,externalBrowserUseAllowed:[^,}]+,((?:[A-Za-z_$][\w$]*:[^,}]+,){0,6}?)computerUse:/,
    'inAppBrowserUse:!0,$1inAppBrowserUseAllowed:!0,$2browserPane:!0,$3externalBrowserUse:!0,externalBrowserUseAllowed:!0,$4computerUse:'
  );
  after = after.replace(
    /browser_use_availability_resolved`,\{safe:\{available:[^,]+,platform:([^,]+),reason:[^,]+,release:([^}]+)\},sensitive:\{browserPane:[^}]+\}\}\)/,
    'browser_use_availability_resolved`,{safe:{available:!0,platform:$1,reason:`local-patched`,release:$2},sensitive:{browserPane:!0}})'
  );

  if (after === before &&
      !before.includes(patchedSenderFragment) &&
      !patchedSenderPattern.test(before)) {
    process.stderr.write('browser-use-desktop-feature-sender-patch-target-not-found\n');
    process.exit(2);
  }
  writeIfChanged(file, before, after);
}

function patchDesktopFeatureMain(file) {
  const before = read(file);
  const patchedMainFragment = 'browserPane:!0,inAppBrowserUse:!0,inAppBrowserUseAllowed:!0,externalBrowserUse:!0,externalBrowserUseAllowed:!0';
  // Desktop 26.917 dropped `computerUseNodeRepl` from the env gate, so only the slot-shaped
  // feature object is rewritten and the legacy fragment above never appears on a re-run.
  const patchedMainPattern = /inAppBrowserUse:!0,(?:mcpAppsBrowserUse:[^,}]+,)?inAppBrowserUseAllowed:!0,(?:[A-Za-z_$][\w$]*:[^,}]+,){0,10}?browserPane:!0,(?:[A-Za-z_$][\w$]*:[^,}]+,){0,8}?externalBrowserUse:!0,externalBrowserUseAllowed:!0,computerUse:/;
  const envGatePattern = /([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*)===`win32`&&([A-Za-z_$][\w$]*)\.CODEX_ELECTRON_ENABLE_WINDOWS_COMPUTER_USE===`1`\?\{\.\.\.([A-Za-z_$][\w$]*),computerUse:!0,computerUseNodeRepl:!0\}:\4/;
  if (!before.includes(patchedMainFragment) &&
      !patchedMainPattern.test(before) &&
      (!before.includes('CODEX_ELECTRON_ENABLE_WINDOWS_COMPUTER_USE') ||
       (!envGatePattern.test(before) &&
        !/inAppBrowserUse:[A-Za-z_$][\w$]*\.inAppBrowserUse,(?:mcpAppsBrowserUse:[^,}]+,)?inAppBrowserUseAllowed:[A-Za-z_$][\w$]*\.inAppBrowserUseAllowed,(?:[A-Za-z_$][\w$]*:[^,}]+,){0,10}?browserPane:[A-Za-z_$][\w$]*\.browserPane,(?:[A-Za-z_$][\w$]*:[^,}]+,){0,8}?externalBrowserUse:[A-Za-z_$][\w$]*\.externalBrowserUse,externalBrowserUseAllowed:[A-Za-z_$][\w$]*\.externalBrowserUseAllowed/.test(before)))) {
    process.stderr.write('browser-use-desktop-feature-main-target-not-found\n');
    process.exit(2);
  }

  let after = before.replace(
    envGatePattern,
    '$1=$2===`win32`?{...$4,browserPane:!0,inAppBrowserUse:!0,inAppBrowserUseAllowed:!0,externalBrowserUse:!0,externalBrowserUseAllowed:!0,...$3.CODEX_ELECTRON_ENABLE_WINDOWS_COMPUTER_USE===`1`?{computerUse:!0,computerUseNodeRepl:!0}:{}}:$4'
  );
  after = after.replace(
    /inAppBrowserUse:[A-Za-z_$][\w$]*\.inAppBrowserUse,(mcpAppsBrowserUse:[^,}]+,)?inAppBrowserUseAllowed:[A-Za-z_$][\w$]*\.inAppBrowserUseAllowed,((?:[A-Za-z_$][\w$]*:[^,}]+,){0,10}?)browserPane:[A-Za-z_$][\w$]*\.browserPane,((?:[A-Za-z_$][\w$]*:[^,}]+,){0,8}?)externalBrowserUse:[A-Za-z_$][\w$]*\.externalBrowserUse,externalBrowserUseAllowed:[A-Za-z_$][\w$]*\.externalBrowserUseAllowed,computerUse:/,
    'inAppBrowserUse:!0,$1inAppBrowserUseAllowed:!0,$2browserPane:!0,$3externalBrowserUse:!0,externalBrowserUseAllowed:!0,computerUse:'
  );

  if (after === before &&
      !before.includes(patchedMainFragment) &&
      !patchedMainPattern.test(before)) {
    process.stderr.write('browser-use-desktop-feature-main-patch-target-not-found\n');
    process.exit(2);
  }
  writeIfChanged(file, before, after);
}

patchFeatureHook(featureHookFile);
patchSidebarAvailability(sidebarAvailabilityFile);
patchDesktopFeatureSender(desktopFeatureSenderFile);
patchDesktopFeatureMain(desktopFeatureMainFile);

process.stdout.write(changed ? 'patched' : 'already-patched');
