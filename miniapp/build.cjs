// Bundle the audited cipher for the native runtime. No runtime npm or web API dependency.
const { buildSync } = require('esbuild');
const { writeFileSync } = require('node:fs');
const { join } = require('node:path');
const root = __dirname;
buildSync({
  absWorkingDir: root,
  stdin: {
    contents: "module.exports = require('@noble/ciphers/aes').gcm;",
    resolveDir: root
  },
  bundle: true,
  platform: 'browser',
  format: 'cjs',
  target: 'es2020',
  minify: true,
  legalComments: 'eof',
  outfile: join(root, 'miniprogram/lib/aes.js')
});
if (process.env.HUB_MOBILE_ORIGIN) {
  const url = new URL(process.env.HUB_MOBILE_ORIGIN);
  if (url.protocol !== 'https:' || url.href !== url.origin + '/')
    throw Error('HUB_MOBILE_ORIGIN 需要 HTTPS 域名，不含路径或凭据');
  writeFileSync(
    join(root, 'miniprogram/config.js'),
    'module.exports = ' + JSON.stringify(url.origin) + ';\n'
  );
}
