// SDK 语义测试(node --test,直接测 dist 产物;使用独立端口区间,不干扰本机服务)
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import http from 'node:http'
import { afterEach, test } from 'node:test'

import { createNotifyService } from '../dist/x-notify-service-sdk.js'

const servers = []

// 每个用例独立端口:避免 undici 连接池复用上一用例已关闭服务的陈旧 socket
function mockServer({ app = 'x-notify-service', notifyStatus = 200, port = 24520 } = {}) {
  const calls = { notify: [], close: 0 }
  const server = http.createServer((req, res) => {
    if (req.url === '/health') {
      res.end(JSON.stringify({ app, version: '0.1.0-test', port }))
    } else if (req.url === '/notify') {
      let body = ''
      req.on('data', (c) => (body += c))
      req.on('end', () => {
        calls.notify.push(JSON.parse(body))
        res.statusCode = notifyStatus
        res.end(JSON.stringify({ ok: notifyStatus === 200, via: 'system' }))
      })
    } else if (req.url === '/close') {
      calls.close += 1
      res.end('{"ok":true}')
    } else {
      res.statusCode = 404
      res.end()
    }
  })
  return new Promise((resolve) =>
    server.listen(port, '127.0.0.1', () => resolve({ server, calls, port })),
  )
}

afterEach(() => {
  while (servers.length > 0) {
    servers.pop().close()
  }
})

test('服务未运行:notify 静默失败返回 ok:false,不抛错', async () => {
  const bridge = createNotifyService({ basePort: 24590, portRange: 3 })
  const r = await bridge.notify({ title: 't', body: 'b' })
  assert.equal(r.ok, false)
  assert.equal(r.via, undefined)
})

test('伪服务(应用身份不符):discover 拒绝,notify 静默失败', async () => {
  const { server, port } = await mockServer({ app: 'other-service', port: 24530 })
  servers.push(server)
  const bridge = createNotifyService({ basePort: port, portRange: 3 })
  assert.equal(await bridge.discover(true), null)
  const r = await bridge.notify({ title: 't' })
  assert.equal(r.ok, false)
})

test('真服务:discover 命中并缓存,notify 透传尺寸颜色,close 幂等', async () => {
  const { server, calls, port } = await mockServer({ port: 24540 })
  servers.push(server)
  const bridge = createNotifyService({ basePort: port, portRange: 3 })

  const base = await bridge.discover(true)
  assert.equal(base, `http://127.0.0.1:${port}`)

  const r = await bridge.notify({
    title: '工单',
    body: '<b>紧急</b>',
    width: 320,
    height: 120,
    headerBackgroundColor: '#112233',
    headerTextColor: '#FFFFFF',
    bodyBackgroundColor: '#F0F1F2',
    bodyTextColor: '#334455',
  })
  assert.equal(r.ok, true)
  assert.equal(calls.notify.length, 1)
  assert.deepEqual(
    calls.notify[0],
    {
      title: '工单',
      body: '<b>紧急</b>',
      width: 320,
      height: 120,
      headerBackgroundColor: '#112233',
      headerTextColor: '#FFFFFF',
      bodyBackgroundColor: '#F0F1F2',
      bodyTextColor: '#334455',
    },
    '字段应完整透传',
  )

  await bridge.close()
  await bridge.close()
  assert.equal(calls.close, 2, 'close 每次都请求(幂等由服务端保证)')
})

test('空标题:抛出参数错误(编程错误应显式暴露)', async () => {
  const bridge = createNotifyService({ basePort: 24590, portRange: 3 })
  await assert.rejects(() => bridge.notify({ title: ' ' }), /title/)
})

test('UMD 产物:AMD 分支(define.amd)与普通 script 全局分支可用', async () => {
  const src = await readFile(
    new URL('../dist/x-notify-service-sdk.umd.js', import.meta.url),
    'utf8',
  )
  assert.match(src, /define\.amd/, 'UMD 包装须含 AMD 分支')

  // 模拟 AMD 加载器(RequireJS 行为):命中 define.amd 分支,匿名模块 + 依赖 ['exports']
  const captured = {}
  const fakeDefine = (deps, factory) => {
    captured.deps = deps
    captured.exports = {}
    factory(captured.exports)
  }
  fakeDefine.amd = true
  globalThis.define = fakeDefine
  try {
    new Function(src)()
  } finally {
    delete globalThis.define
  }
  assert.deepEqual(captured.deps, ['exports'])
  assert.equal(typeof captured.exports.createNotifyService, 'function')
  assert.equal(captured.exports.DEFAULT_BASE_PORT, 17320)
  assert.equal(captured.exports.PROTOCOL_URL, 'x-notify://launch')

  // 普通脚本分支:暴露全局 XNotifyServiceSdk(new Function 体内 this 指向 globalThis)
  new Function(src)()
  assert.equal(typeof globalThis.XNotifyServiceSdk?.createNotifyService, 'function')
  delete globalThis.XNotifyServiceSdk
})
