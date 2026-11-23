const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { snapshot } = require('../miniprogram/lib/conversation');

test('phone ignores old local choices and follows target changes within one event batch', async () => {
  let definition;
  const source = path.resolve(__dirname, '../miniprogram/pages/chat/index.js');
  const localReads = [];
  vm.runInNewContext(fs.readFileSync(source, 'utf8'), {
    Page: value => { definition = value; },
    wx: { setNavigationBarTitle() {} },
    setTimeout: () => 1,
    clearTimeout() {},
    require: name => {
      if (name.endsWith('/platform')) return {
        get: key => { localReads.push(key); return {tool:true,artifacts:true}; },
        set() {}, decoded: value => value, scale: () => 17, failure: e => e.message
      };
      if (name.endsWith('/api')) return { rpc() {} };
      if (name.endsWith('/crypto')) return { uuid() {} };
      return require(path.resolve(path.dirname(source), name));
    }
  });
  const page = { ...definition, data:{...definition.data}, setData(data) { Object.assign(this.data, data); } };
  page.onLoad({binding:'paired',client:'codex',session:'original'});
  assert.deepEqual(localReads, []);
  assert.equal(page.flags.tool, false);
  page.state = snapshot({items:[],cursor:1,status:'running'});
  page.baseline = new Set();
  page.data.caps = {liveActivity:true};
  page.data.connection = '已连接';
  page.seq = 1;
  const event = (id, tool, text) => ({
    id, clientId:'codex',sessionId:'original',type:'cursor.advanced',
    data:{displayContent:{tool},liveActivity:{category:'tool',itemId:'tool',mode:'append',text}}
  });
  page.api = async () => ({events:[event(2,false,'hidden'),event(3,true,'shown'),event(4,false,'hidden-again')]});
  await page.poll(1);
  assert.deepEqual(page.state.items.map(i=>i.text), ['shown']);
  assert.equal(page.data.displayManaged, true);
  assert.equal(page.flags.tool, false);
  assert.equal(page.state.cursor, 4);
  assert.equal(page.settings, undefined);
  assert.equal(page.switchFlag, undefined);
  const older = {id:'partial',type:'message',role:'assistant',text:'',files:[]};
  page.state = snapshot({items:[older],cursor:4,status:'running'});
  page.receiveDisplay({artifact:true});
  const finished = {...older,files:[{id:'earlier-file',kind:'artifact'}],status:'completed'};
  page.api = async () => ({events:[{id:5,type:'item.updated',clientId:'codex',sessionId:'original',data:{displayContent:{artifact:true},item:finished,completed:true}}]});
  await page.poll(1);
  assert.equal(page.state.items.some(i=>i.files?.length), false);
  assert.equal(page.state.registry.files.has('earlier-file'), true);
});
