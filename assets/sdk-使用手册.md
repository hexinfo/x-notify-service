# x-notify-service JS SDK 使用手册

浏览器页面 → 本机右下角弹窗通知。本手册与 `sdk.js`、`sdk.umd.js` 随安装包发布。

## 1. 环境要求

- 现代浏览器，基线 Chrome/Chromium 87（同级即可：Edge ≥ 88、Firefox ≥ 78、Safari ≥ 14；不支持 IE）
- 本机已安装并运行 x-notify-service（未安装时 SDK 静默失败，不报错）
- Linux 发行包要求 glibc ≥ 2.28，并依赖系统的 XCB 与 xkbcommon 动态库；Debian/Ubuntu 可安装 `libxcb1 libxkbcommon0 libxkbcommon-x11-0`。自绘弹窗还需要可用的图形驱动。

## 2. 引入方式

两份文件由同一套源码构建，功能完全相同，只是模块格式不同；单个项目按接入方式选择其中一份即可，无需同时加载：

- `sdk.js`：ES Module，供原生 ESM 或现代构建工具使用。
- `sdk.umd.js`：独立完整的 UMD 产物，已包含全部 SDK 逻辑，不依赖 `sdk.js`；支持 AMD/RequireJS 和普通 `<script>`。

把 `sdk.js` 拷贝进你的项目（与页面同目录或任意静态路径），以 ES Module 引入：

```html
<script type="module">
  import { createNotifyService } from './sdk.js'
  const svc = createNotifyService()
</script>
```

不支持 ESM 的老项目只需引入 `sdk.umd.js`：

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
  body: '待办通知 **1** 条\n\n15:21:05',
  // 弹窗尺寸可选(逻辑像素):缺省 220×100
  width: 400,
  height: 140,
  headerBackgroundColor: '#273449',
  headerTextColor: '#FFFFFF',
  bodyBackgroundColor: '#F7F9FC',
  bodyTextColor: '#3F4754',
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
| notify(opts) | `Promise<{ ok, via? }>` | 发送通知。title 必填（≤200 字符），body 可选（≤2000 字符）；width/height 可选（220-800 / 80-600，缺省 220×100）；四个颜色字段可选且格式为 #RRGGBB |
| close() | `Promise<void>` | 显式关闭当前弹窗（幂等） |
| destroy() | void | 清空已缓存的 baseUrl |

### 静默失败语义

`notify()` 在服务未安装/未运行时**不拉起、不抛错**，返回 `{ ok: false }` 交业务自理。
是否需要提示用户安装，由业务根据 `ok` 决定。

## 5. Markdown 正文

`body` 是 CommonMark/GFM 兼容的 Markdown 字符串（最长 2000 字符）。可使用粗体、标题、列表、引用、行内代码、代码块和 Markdown 换行/空行。示例：

```js
await svc.notify({
  title: '工单提醒',
  body: '待办通知 **1** 条\n\n15:21:05',
})
```

旧 HTML 子集语法不再兼容：`<b>`、`<strong>`、`<font>`、`<span style>` 和 `<br>` 不会作为格式语法解析；原样 HTML 会显示为文本。正文颜色统一由 `bodyTextColor` 控制，不支持正文内设置颜色或字号。链接只显示链接样式，不会打开浏览器；图片和其他远程资源不会加载，图片节点仅显示替代文本。

弹窗默认尺寸仍为 220×100 逻辑像素，`width`/`height` 范围及缺省值不变；超出可见行数的正文会省略。标题和正文颜色字段、默认颜色保持不变，正文颜色统一由 `bodyTextColor` 控制。弹窗常驻不超时，点击关闭或被新通知顶掉（不堆叠）。系统通知兜底渠道（`via=system`）将 Markdown 投影为纯文本，不显示格式标记。

## 6. 行为与限制

- 端口：服务默认 17320，被占自动向后探测 10 个；SDK 须与服务端配置一致才能发现
- 同机多用户：可能命中另一用户会话的实例（v1 已知限制）
- 自绘弹窗由 GPUI 使用 GPU 渲染。GPU/图形驱动不可用或 GPUI 初始化失败时，服务继续运行并改用系统通知（`via: 'system'`）。
- Linux Wayland：是否显示自绘弹窗取决于桌面环境是否支持 Layer Shell；不支持时自动走系统通知（`via: 'system'`）。Linux X11、Windows 及 macOS 的图形运行时/窗口行为应在目标桌面环境验证；仅通过编译或测试不能证明目标机器运行时正常。

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
`popup` = 右下角弹窗（主渠道）；`system` = 系统通知（兜底：无桌面会话、GPU/GPUI 初始化失败或 Wayland 桌面不支持 Layer Shell）。

**Q：RequireJS 报 Mismatched anonymous define / 模块加载超时？**
同一页面只用一种模块 ID 引用 SDK：配了 paths 别名就全程用别名，没配就全程用相对路径。两种混用会让同一文件按两个 ID 各加载一次，匿名模块在第二次注册时无脚本上下文即报错（匿名 UMD 库的通病，jQuery 同此）。也别把 `sdk.umd.js` 交给 r.js 优化器打进别的 bundle，按外部依赖独立加载即可。
