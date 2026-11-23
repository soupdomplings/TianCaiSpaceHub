const fs = require('node:fs'),
  path = require('node:path'),
  vm = require('node:vm');
const { wxmlToJs, wxssToJs } = require('miniprogram-compiler');
const root = path.resolve(__dirname, process.argv[2] || 'miniprogram');
for (const kind of [wxmlToJs, wxssToJs]) {
  const js = kind(root);
  new Function('global', js);
}
function check(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, entry.name);
    if (entry.isDirectory()) check(file);
    else if (file.endsWith('.js'))
      new vm.Script(fs.readFileSync(file, 'utf8'), { filename: file });
    else if (file.endsWith('.json')) JSON.parse(fs.readFileSync(file, 'utf8'));
  }
}
check(root);
console.log(
  'WXML, WXSS, JavaScript and JSON checked: ' +
    path.basename(path.dirname(root)) +
    '/' +
    path.basename(root)
);
