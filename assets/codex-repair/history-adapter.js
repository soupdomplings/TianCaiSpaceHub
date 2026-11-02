// Pure, optional source adaptation. A changed/ambiguous layout stays native.
(function adaptHubHistorySource(source) {
  const result = { source, historyResume: false, historyRetry: false };
  const runtime = 'globalThis.__CODEXHUB_HISTORY_RECOVERY__';
  const guard = 'globalThis.__CODEXHUB_ENHANCED_MODE__?.historyRecoveryEnabled===true';
  const unique = (text, pattern) => {
    const matches = [...text.matchAll(pattern)];
    return matches.length === 1 ? matches[0] : null;
  };
  if (!source.includes('CODEXHUB_HISTORY_PREPARE_V1')) {
    const resume = unique(source, /([\w$]+)\(\{manager:this,hostMode:this\.runtime\.threadMode\(this\.hostId\),threadRecognition:this\.threadRecognition,workspace:this\.projectlessConversations,params:([\w$]+),product:([\w$]+),historyPolicy:this\.history,requireBoundedHistory:([\w$]+)===`follow-up`,schedule:this\.scheduler\.schedule,isReconnectRecovery:([\w$]+),canAcquireOwnership:([\w$]+),isCurrentResumeAttempt:\(\)=>!this\.disposed&&this\.inFlightConversationResumes\.get\(([\w$]+)\.conversationId\)===([\w$]+),setHydrationCleanup:([\w$]+)=>\{/g);
    if (resume && resume[2] === resume[7]) {
      const [matched, , params, product, intent, , , , attempt] = resume;
      const next = matched.replace(`product:${product},`, `product:(${runtime}?.product(${product},{manager:this,conversationId:${params}.conversationId,source:${intent},isCurrent:()=>!this.disposed&&this.inFlightConversationResumes.get(${params}.conversationId)===${attempt}})??${product})/*CODEXHUB_HISTORY_PREPARE_V1*/,`);
      result.source = source.slice(0, resume.index) + next + source.slice(resume.index + matched.length);
      result.historyResume = true;
    }
  }
  // Apply page coalescing, partial-history error visibility and the Retry CTA
  // together. Keep native transcript-projection errors on their original path.
  if (!source.includes('CODEXHUB_HISTORY_RETRY_V1')) {
    const page = unique(result.source, /await ([\w$]+)\(([\w$]+),([\w$]+)\)\.loadOlderConversationHistoryPage\(([\w$]+),\[\],([\w$]+),([\w$]+)\(\3\),([\w$]+)\?\.trace\)/g);
    const failure = unique(result.source, /warning\(`Failed to load older thread history`,\{safe:\{conversationId:([\w$]+)\},sensitive:\{error:([\w$]+)\}\}\),([\w$]+)\(\1\),`stop`/g);
    const retry = unique(result.source, /onPrimaryCtaClick:\(\)=>\{([\w$]+)===`error`\?window\.location\.reload\(\):([\w$]+)\(null\)\},primaryCtaColor:`ghostActive`,primaryCtaText:[^;]{0,180}?id:`localConversation\.retryHistoryLoad`/g);
    const props = unique(result.source, /onLoadOlderHistory:([\w$]+),composerPresentation:[\w$]+,composerSubmitDisabled:[\w$]+\|\|[\w$]+,dotTaskKey:[\w$]+\?[\w$]+:void 0,conversationId:([\w$]+),hostId:([\w$]+),isThreadHistoryLoading:/g);
    if (page && failure && retry && props && page[4] === failure[1] && page[4] === props[2]
      && failure.index > page.index && failure.index - page.index < 900
      && retry.index > failure.index && retry.index - failure.index < 8500
      && props.index > retry.index && props.index - retry.index < 2000) {
      const [, getManager, scope, host, thread] = page;
      const call = page[0].slice('await '.length);
      const context = `{hostId:${props[3]},conversationId:${thread}}`;
      const local = `(${guard}&&${props[3]}===\`local\`)`;
      const changes = [
        [page.index, page[0], `await (${runtime}?.page({hostId:${host},conversationId:${thread}},()=>${call})??${call})`],
        [failure.index, failure[0], failure[0].replace(`${failure[3]}(${thread}),\`stop\``, `(${local}?(${runtime}?.showError(${failure[2]})===false?void 0:${retry[2]}(${thread})):${failure[3]}(${thread})),\`stop\``)],
        [retry.index, retry[0], retry[0].replace(`${retry[2]}(null)`, `(${local}&&${runtime}?${runtime}.retry(${context},()=>${retry[2]}(null),${props[1]}):${retry[2]}(null))/*CODEXHUB_HISTORY_RETRY_V1*/`)],
      ];
      for (const [index, original, next] of changes.sort((a, b) => b[0] - a[0])) {
        result.source = result.source.slice(0, index) + next + result.source.slice(index + original.length);
      }
      result.historyRetry = true;
    }
  }
  if (result.historyResume || result.historyRetry) {
    const flags = Object.fromEntries(Object.entries(result).filter(([name, value]) => name !== 'source' && value === true));
    result.source += '\n;globalThis.__CODEXHUB_HISTORY_RECOVERY_SOURCE__=Object.assign(globalThis.__CODEXHUB_HISTORY_RECOVERY_SOURCE__??{},{version:1},' + JSON.stringify(flags) + ');\n';
  }
  return result;
})
