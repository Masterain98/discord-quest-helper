import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import vm from 'node:vm'

const source = await fs.readFile(new URL('../src-tauri/src/super_properties/cdp_snapshot.js', import.meta.url), 'utf8')
function read({ properties = {}, heartbeat = null, known = true, focused = false, globals = {}, ids = {} } = {}) {
  let writeCalls = 0
  const api = {getSuperPropertiesBase64: () => Buffer.from(JSON.stringify(properties)).toString('base64'), getSuperProperties: () => properties, extendSuperProperties: () => writeCalls++}
  const req = {
    c: {
      a: {exports:{default:api}},
      u: {exports:{default:{getCurrentUser:()=>({id:'synthetic-account'}),getUser:()=>null}}},
      w: {exports:{A:{isAppFocused:()=>focused}}},
      h: {exports:known ? {Vc:()=>heartbeat, Ht:()=>{writeCalls++}} : {}},
    },
    m: {h:function heartbeatFactory(){return 'LAST_CLIENT_HEARTBEAT_SESSION'}},
  }
  const chunks = []
  chunks.push = chunk => { Array.prototype.push.call(chunks,chunk); return chunk[2](req) }
  const context = vm.createContext({
    performance:{timeOrigin:1}, location:{href:'https://discord.com/channels/@me'},
    document:{hasFocus:()=>!focused},navigator:{languages:['zh-TW'],language:'en-US',userAgent:'synthetic Electron/42.0.0'},
    TextDecoder,Uint8Array,atob,
    webpackChunkdiscord_app:chunks,
    DiscordNative:{process:{platform:'win32'},os:{release:'10.0.12345',arch:'x64',appArch:'arm64'},app:{getVersion:()=> 'synthetic',getBuildNumber:()=>123,getReleaseChannel:()=> 'stable'}},
    ...globals,
  })
  vm.runInContext('window=globalThis',context)
  const result = new vm.Script(source.replace('__GENERATION__','"1"').replace('__MODULE_IDS__',JSON.stringify(ids))).runInContext(context)
  return { result:JSON.parse(JSON.stringify(result)), writeCalls, chunkCount:chunks.length }
}

test('reads cached JSON including unknown keys and explicit null without modifying client stores',()=>{
  const properties={launch_signature:null,client_launch_id:'client-launch',future:'中文🍃'}
  const {result,writeCalls,chunkCount}=read({properties,heartbeat:{uuid:'client-session'}})
  assert.deepEqual(result.decoded,properties)
  assert.equal(result.heartbeat_session_id,'client-session')
  assert.equal(result.heartbeat_known,true)
  assert.equal(result.focused,false)
  assert.equal(writeCalls,0)
  assert.equal(chunkCount,0)
  assert.equal(result.fallback.browser_version,'42.0.0')
  assert.equal(result.fallback.app_arch,'arm64')
  assert.equal(result.fallback.os_sdk_version,'12345')
})

test('distinguishes an intentionally absent heartbeat from an unsupported getter',()=>{
  assert.equal(read({heartbeat:null}).result.heartbeat_known,true)
  assert.equal(read({known:false}).result.heartbeat_known,false)
})

test('legacy mods and signature probes have different null and jQuery semantics',()=>{
  const nullVencord=read({globals:{Vencord:null}}).result
  assert.equal(nullVencord.fallback.has_client_mods,false)
  assert.deepEqual(nullVencord.mod_bit_indices,[43])
  const jquery=read({globals:{jQuery:{}}}).result
  assert.equal(jquery.fallback.has_client_mods,true)
  assert.deepEqual(jquery.mod_bit_indices,[])
  assert.deepEqual(read({globals:{$:{fn:{jquery:'synthetic'}}}}).result.mod_bit_indices,[8])
})

test('uses cached module IDs and rediscovers invalidated IDs without relying on fixed module numbers',()=>{
  const first=read().result
  assert.equal(first.module_ids.properties,'a')
  assert.equal(read({ids:first.module_ids,focused:true}).result.focused,true)
  assert.equal(read({ids:{properties:'old-module'}}).result.module_ids.properties,'a')
})

test('generation guard rejects renderer reloads',()=>{
  const script=source.replace('__GENERATION__','"previous-renderer"').replace('__MODULE_IDS__','{}')
  assert.throws(()=>new vm.Script(script).runInNewContext({performance:{timeOrigin:1}}),/renderer_changed/)
})
