// Change the existing function body so LiveEdit updates its live module binding.
// No new module binding, page reload, frozen bridge mutation or auth replacement.
(function adaptTiangongRunFetch(source) {
  const marker = 'TIANCAISPACEHUB_ASSISTANCE_RUN_FETCH_V1';
  const unchanged = reason => ({ adapted: false, reason });
  if (typeof source !== 'string') return unchanged('invalid_source');
  if (source.includes(marker)) return { adapted: true, present: true };
  if (source.includes('window.__tiancaiAssistanceTransportV1=true;') &&
      source.includes('async function __tchNativeRunFetch(path, options = {}) {')) {
    return { adapted: true, present: true };
  }
  const start = 'async function runFetch(path, options = {}) {';
  const end = '\nasync function request$3(method, path, body, signal) {';
  if (source.split(start).length !== 2 || source.split(end).length !== 2) return unchanged('unsupported_renderer');
  const begin = source.indexOf(start), finish = source.indexOf(end, begin);
  if (finish <= begin || source.slice(finish - 2, finish) !== '\n}') return unchanged('unsupported_renderer');
  const original = source.slice(begin + start.length, finish - 2);
  if (!['const api = window.electronAPI;', 'api.chatRunOpen(', 'api.chatRunRead(',
    'api.chatRunCancel(', 'const { base, token } = await connection();',
    'new Response(body,', 'const signal = options.signal;'].every(value => original.includes(value))) {
    return unchanged('unsupported_transport');
  }
  const body = `
  // ${marker}
  const proceed = async (options) => {${original}
  };
  const assistance = window.__tiancaiAssistance;
  return window.electronAPI?.chatRunOpen && typeof assistance?.runRequest === 'function'
    ? assistance.runRequest(path, options, proceed)
    : proceed(options);
}\n`;
  return { adapted: true, source: source.slice(0, begin) + start + body + source.slice(finish) };
})
