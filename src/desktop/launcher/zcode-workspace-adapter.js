(function adaptZCodeWorkspace(source, options) {
  const unchanged = reason => ({ adapted: false, source, reason });
  const valid = value => Array.isArray(value?.models) && value.models.length > 0 &&
    value.models.every(model => typeof model.modelId === "string" && model.modelId.length > 0 &&
      /^tiancai-hub-[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(model.providerId));
  if (typeof source !== "string" || !valid(options)) return unchanged("invalid-options");
  if (source.includes("__tiancaiZCodeWorkspace")) return unchanged("already-adapted");
  // The native startup hook must expose this exact registry-based decision.
  // Capture minified names so a rebuild alone does not require another patch.
  const gate = /let ([\w$]+)=([\w$]+)\(\{modelSelectionView:([\w$]+)\.forceRefresh\?await ([\w$]+)\(\):([\w$]+)\}\),\{hasUsableProvider:([\w$]+),providerCount:([\w$]+)\}=\1,([\w$]+)=!([\w$]+)\|\|!([\w$]+)&&!\6;/g;
  const matches = [...source.matchAll(gate)];
  if (matches.length !== 1) return unchanged("startup-gate-not-unique");
  const m = matches[0];
  const replacement = `let __tiancaiZCodeView=${m[3]}.forceRefresh?await ${m[4]}():${m[5]},${m[1]}=${m[2]}({modelSelectionView:__tiancaiZCodeView}),{hasUsableProvider:${m[6]},providerCount:${m[7]}}=${m[1]},${m[8]}=!globalThis.__tiancaiZCodeWorkspace.accepts(__tiancaiZCodeView)&&(!${m[9]}||!${m[10]}&&!${m[6]});`;

  function install(initial) {
    let models = initial.models.map(({ providerId, modelId }) => ({ providerId, modelId }));
    let observed = [];
    const matches = () => models.some(model => observed.some(actual =>
      actual.providerId === model.providerId && actual.modelId === model.modelId));
    const state = {
      version: 1,
      available: false,
      accepts(view) {
        // Keep only identities; the native view can contain provider credentials.
        observed = (Array.isArray(view?.providers) ? view.providers : []).flatMap(provider =>
          (Array.isArray(provider.models) ? provider.models : []).map(model => ({
            providerId: provider.providerId, modelId: model.modelId
          })));
        state.available = matches();
        return state.available;
      },
      updateModels(next) {
        if (!Array.isArray(next?.models) || next.models.length === 0) return false;
        models = next.models.map(({ providerId, modelId }) => ({ providerId, modelId }));
        state.available = matches();
        return state.available;
      }
    };
    globalThis.__tiancaiZCodeWorkspace = state;
  }
  const changed = source.slice(0, m.index) + replacement + source.slice(m.index + m[0].length);
  return {
    adapted: true,
    source: `;(${install.toString()})(${JSON.stringify(options)});\n${changed}`,
    reason: ""
  };
})
