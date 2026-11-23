const { rpc } = require('./api');
const { bytes, uuid } = require('./crypto');
const { invoke } = require('./platform');
async function download(binding, path, file) {
  const fs = wx.getFileSystemManager();
  const dir = wx.env.USER_DATA_PATH + '/hub-files';
  fs.mkdirSync(dir, true);
  const name =
    String(file.name || '文件')
      .replace(/[\\/<>:"|?*\x00-\x1f]/g, '_')
      .replace(/^\.+/, '')
      .slice(-100) || '文件';
  const out = dir + '/' + (await uuid()) + '-' + name;
  let offset = 0;
  try {
    for (;;) {
      const res = await rpc(
        binding,
        'GET',
        path + '/files/' + encodeURIComponent(file.id) + '?download=1',
        undefined,
        { offset }
      );
      const chunk = bytes(res.chunk);
      if (
        res.nextOffset != null &&
        (!chunk.length || res.nextOffset !== offset + chunk.length)
      )
        throw Error('产物分片不连续');
      if (!offset) fs.writeFileSync(out, chunk.buffer);
      else fs.appendFileSync(out, chunk.buffer);
      if (res.nextOffset == null) break;
      offset = res.nextOffset;
    }
    if (typeof wx.shareFileMessage === 'function')
      await invoke('shareFileMessage', { filePath: out, fileName: name });
    else await invoke('openDocument', { filePath: out, showMenu: true });
  } finally {
    try {
      fs.unlinkSync(out);
    } catch {}
  }
}
module.exports = download;
