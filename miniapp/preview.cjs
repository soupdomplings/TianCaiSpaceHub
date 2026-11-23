// Build an isolated native WeChat project for an operator who has no AppID yet.
const fs = require('node:fs'),
  path = require('node:path');
const root = __dirname,
  out = path.join(root, 'preview'),
  app = path.join(out, 'miniprogram');
fs.mkdirSync(out, { recursive: true });
fs.cpSync(path.join(root, 'miniprogram'), app, { recursive: true });
fs.copyFileSync(
  path.join(root, 'preview-src/api.js'),
  path.join(app, 'lib/api.js')
);
const config = JSON.parse(
  fs.readFileSync(path.join(root, 'project.config.json'))
);
config.appid = 'touristappid';
config.projectname = 'tiancai-hub-local-preview';
fs.writeFileSync(
  path.join(out, 'project.config.json'),
  JSON.stringify(config, null, 2)
);
fs.copyFileSync(
  path.join(root, 'preview-src/environment.js'),
  path.join(app, 'lib/environment.js')
);
for (const name of ['devices', 'sessions', 'chat', 'squad', 'recharge']) {
  const file = path.join(app, 'pages', name, 'index.json'),
    cfg = JSON.parse(fs.readFileSync(file));
  cfg.navigationBarTitleText = '本地演示 · ' + cfg.navigationBarTitleText;
  fs.writeFileSync(file, JSON.stringify(cfg));
}
fs.writeFileSync(path.join(app, 'config.js'), "module.exports='';\n");
