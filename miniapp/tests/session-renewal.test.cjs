const test=require('node:test');
const assert=require('node:assert/strict');
const {randomBytes,createCipheriv}=require('node:crypto');
const config=require.resolve('../miniprogram/config');
require.cache[config]={exports:'https://example.invalid'};
const {rpc}=require('../miniprogram/lib/api');
test('long-poll RPC refreshes expired WeChat login once; the same expiry never replays a submitted turn',async()=>{
 const key=randomBytes(32);
 let logins=0, calls=0, rejectFirst=true;
 global.wx={
   getStorageSync:()=>key.toString('base64url'),
   arrayBufferToBase64:ab=>Buffer.from(ab).toString('base64'),
   base64ToArrayBuffer:s=>Uint8Array.from(Buffer.from(s,'base64')).buffer,
   getRandomValues:({length,success})=>success({randomValues:Uint8Array.from(randomBytes(length)).buffer}),
   login:({success})=>success({code:'session'+(++logins)}),
   request:({url,data,success})=>{
     if(url.endsWith('/login'))return success({statusCode:200,data:{token:data.code}});
     calls++;
     if(rejectFirst){rejectFirst=false;return success({statusCode:401,data:{message:'微信登录已过期'}});}
     const nonce=randomBytes(12),gcm=createCipheriv('aes-256-gcm',key,nonce);
     gcm.setAAD(Buffer.from(['tc-hub-v1',data.bindingId,data.requestId,'res'].join('|')));
     const body=Buffer.concat([gcm.update(JSON.stringify({status:200,body:{events:[{id:3}]}})),gcm.final(),gcm.getAuthTag()]);
     success({statusCode:200,data:{...data,nonce:nonce.toString('base64url'),ciphertext:body.toString('base64url')}});
   }
 };
 assert.deepEqual(await rpc('pc','GET','/clients/demo/sessions/example/events?after=2'),{events:[{id:3}]});
 assert.equal(logins,2);assert.equal(calls,2);
 rejectFirst=true;
 await assert.rejects(()=>rpc('pc','POST','/clients/demo/sessions/example/turns',{text:'once'}),/过期/);
 assert.equal(logins,2);assert.equal(calls,3);
});
