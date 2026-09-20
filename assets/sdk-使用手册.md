# x-notify-service JS SDK 使用手册

浏览器页面 → 本机右下角弹窗通知。本手册与 `sdk.js` 随安装包发布。

## 1. 环境要求

- 现代浏览器，基线 Chrome/Chromium 87（同级即可：Edge ≥ 88、Firefox ≥ 78、Safari ≥ 14；不支持 IE）
- 本机已安装并运行 x-notify-service（未安装时 SDK 静默失败，不报错）

## 2. 引入方式

把 `sdk.js` 拷贝进你的项目（与页面同目录或任意静态路径），以 ES Module 引入：

```html
<script type="module">
  import { createNotifyService } from './sdk.js'
  const svc = createNotifyService()
</script>
```

不支持 ESM 的老项目用 `sdk.umd.js`（UMD 格式，兼容 RequireJS 等 AMD 加载器与普通 `<script>` 标签）：

```html
<!-- AMD(RequireJS):文件不在页面同目录时,先配 paths 别名(值不带 .js 后缀) -->
<script src="require.js"></script>
<script>
  requirejs.config({ paths: { 'x-notify-sdk': '/assets/sdk.umd' } })
  require(['x-notify-sdk'], function (XNotifyServiceSdk) {
    var svc = XNotifyServiceSdk.createNotifyService()
  })
</script>

<!-- 与页面同目录时可省略 config,直接按路径引用 -->
<script src="require.js"></script>
<script>
  require(['sdk.umd'], function (XNotifyServiceSdk) {
    var svc = XNotifyServiceSdk.createNotifyService()
  })
</script>

<!-- 普通脚本标签(暴露全局 XNotifyServiceSdk) -->
<script src="./sdk.umd.js"></script>
<script>
  var svc = window.XNotifyServiceSdk.createNotifyService()
</script>
```

sdk.js / sdk.umd.js 获取途径：发行包内、Linux `~/.local/share/x-notify-service/`、Windows 安装目录。

## 3. 快速开始

```ts
const svc = createNotifyService()

// 页面初始化时提前拉起服务(仅对需要通知能力的角色调用;幂等)
await svc.start()

// 发送通知
const result = await svc.notify({
  title: '工单提醒',
  body: '<b>加粗</b> <font color="#d93025">红色</font><br>第二行',
})
// result: { ok: true, via: 'popup' | 'system' }
```

服务自带演示页,地址见 `x-notify-service info` 输出的「演示页」一行。

## 4. API

### createNotifyService(options?)

| 参数 | 类型 | 默认 | 说明 |
|---|---|---|---|
| basePort | number | 17320 | 端口探测起始值，须与服务端配置一致 |
| portRange | number | 10 | 探测端口个数，须与服务端配置一致 |
| requestTimeoutMs | number | 3000 | notify 单次请求超时（毫秒） |
| token | string | — | 与服务端 config.toml 的 token 一致时自动携带 X-Token；不配置则按无鉴权调用 |

返回实例方法：

| 方法 | 返回 | 说明 |
|---|---|---|
| discover(force?) | `Promise<string \| null>` | 探测服务，返回 baseUrl（如 `http://127.0.0.1:17321`）；带缓存，force 强制重探 |
| start(timeoutMs?) | `Promise<boolean>` | 提前拉起服务（经 `x-notify://` 协议），避免通知时刻才冷启动；幂等，未安装/超时静默返回 false |
| notify(opts) | `Promise<{ ok, via? }>` | 发送通知。title 必填（≤200 字符），body 可选（≤2000 字符） |
| close() | `Promise<void>` | 显式关闭当前弹窗（幂等） |
| destroy() | void | 清空已缓存的 baseUrl |

### 静默失败语义

`notify()` 在服务未安装/未运行时**不拉起、不抛错**，返回 `{ ok: false }` 交业务自理。
是否需要提示用户安装，由业务根据 `ok` 决定。

## 5. 正文 HTML 子集

body 只支持以下标记，其余标签一律剥除保留内文：

- 加粗：`<b>`、`<strong>`
- 颜色：`<font color="#d93025">`、`<span style="color: red">`（#RGB/#RRGGBB 及常见颜色名）
- 字号：`<font size="16">`、`style="font-size: 16px"`（11–18，按行生效）
- 换行：`<br>`
- HTML 实体：`&amp;` `&lt;` `&gt;` `&quot;` `&nbsp;` `&#65;` `&#x41;`

排版：最多 5 行，超出截断加 `…`；行首标点禁则。弹窗常驻不超时，点击关闭或被新通知顶掉（不堆叠）。
系统通知兜底渠道（via=system）自动剥除全部标记为纯文本。

## 6. 行为与限制

- 端口：服务默认 17320，被占自动向后探测 10 个；SDK 须与服务端配置一致才能发现
- 同机多用户：可能命中另一用户会话的实例（v1 已知限制）
- Wayland 会话：弹窗不可用，自动走系统通知（`via: 'system'`）

## 7. 服务端排查命令

```bash
x-notify-service info                 # 诊断快照:实例/端口/工作区/落点/注册状态
x-notify-service notify -t "手测"     # 本机直发一条(经运行中服务)
x-notify-service start|stop|restart   # 服务生命周期
x-notify-service uninstall            # 停止服务并清理全部注册
```

## 8. 常见问题

**Q：notify 返回 `{ ok: false }`？**
本机未安装或服务未运行。页面初始化先 `await svc.start()`；仍 false 则引导安装。

**Q：服务端配置了 token 怎么办？**
`createNotifyService({ token: '与服务端一致' })` 即可，SDK 会自动在 notify/close 请求头带上 X-Token。

**Q：`via` 是什么？**
`popup` = 右下角弹窗（主渠道）；`system` = 系统通知（兜底：无桌面会话/Wayland/弹窗初始化失败）。

**Q：RequireJS 报 Mismatched anonymous define / 模块加载超时？**
同一页面只用一种模块 ID 引用 SDK：配了 paths 别名就全程用别名，没配就全程用相对路径。两种混用会让同一文件按两个 ID 各加载一次，匿名模块在第二次注册时无脚本上下文即报错（匿名 UMD 库的通病，jQuery 同此）。也别把 `sdk.umd.js` 交给 r.js 优化器打进别的 bundle，按外部依赖独立加载即可。
