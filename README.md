# x-notify-service

浏览器网页调用 → 屏幕右下角置顶弹窗(系统通知兜底)。Windows / Linux(UOS、麒麟)。

```
┌──────────┐  HTTP(127.0.0.1)  ┌─────────────────┐
│ 业务页面  │ ───────────────→ │ x-notify-service │──→ 右下角置顶弹窗
│ JSSDK    │ ←── /health 验身 └─────────────────┘└─→ 系统通知(弹窗不可用时)
└──────────┘        服务不在线 → x-notify:// 协议拉起
```

## CLI

```
x-notify-service              # 无参数:帮助
x-notify-service serve        # 运行服务(常驻进程)
x-notify-service install      # 注册自启 + 协议,并启动;uninstall 反向清理
x-notify-service start|stop|restart   # 生命周期(幂等)
x-notify-service info         # 诊断:实例/端口/工作区/落点/注册状态/安全配置
x-notify-service notify -t "标题" [-b "**正文**"] [-f]   # 手测通知;-f 走系统通知
x-notify-service close        # 关闭当前弹窗
```

- 用户级注册,免 root/管理员;单实例,端口 `17320` 起向后探测 10 个。
- 日志按天滚动保留 3 天(Linux `~/.local/state/x-notify-service/logs`,Windows `%LOCALAPPDATA%\x-notify-service\logs`)。
- 配置:`--config` > 二进制同目录 `config.toml` > 用户配置目录;模板见 `scripts/templates/config.toml`。

### 安全参数(不配置 = 全开放无鉴权)

```toml
cors_origins = ["http://oa.example.com"]   # CORS 白名单;默认 ["*"]
token = "xxx"                              # /notify //close 需 X-Token 头;默认无鉴权
allow_private_network = false              # 关闭本地网络预检应答;默认 true
```

SDK 侧 `createNotifyService({ token: 'xxx' })` 同步配置;`info` 可查看生效值。

## HTTP API(127.0.0.1)

```
GET  /            → 演示页(内嵌)
GET  /sdk.js      → SDK 产物(内嵌,演示页同源引用)
GET  /health → {"app":"x-notify-service","version":"…","port":17320}
POST /notify → {"ok":true,"via":"popup"|"system"}   body: {"title":"≤200字", "body":"≤2000字,Markdown", "width":"220-800可选", "height":"80-600可选", "headerBackgroundColor":"#RRGGBB可选", "headerTextColor":"#RRGGBB可选", "bodyBackgroundColor":"#RRGGBB可选", "bodyTextColor":"#RRGGBB可选"}
POST /close  → {"ok":true}
```

`body` 使用 CommonMark/GFM 兼容的 Markdown 正文，包括粗体、标题、列表、引用、行内代码、代码块和换行；例如 `**加粗**`。旧 HTML 子集不再兼容：`<b>`、`<font>`、`<span style>` 和 `<br>` 不再作为格式语法，原样 HTML 不会被解释为格式。链接只显示样式，不会打开浏览器；图片和远程资源不会加载，图片仅显示替代文本。正文颜色由 `bodyTextColor` 统一控制，不支持正文内设置颜色或字号。弹窗正文按高度限制行数，超出内容省略；弹窗常驻不超时，新通知顶掉旧的(不堆叠)。尺寸缺省仍为 220×100，四个颜色字段及其现有默认值保持不变。

自绘弹窗使用 GPUI GPU 渲染；GPU/图形驱动不可用或 GPUI 初始化失败时，服务继续运行并改用系统通知。Linux Wayland 是否能显示自绘弹窗取决于桌面环境的 Layer Shell 支持，不支持时使用系统通知。Windows 和 Linux 的图形运行时要求及桌面行为仍需在目标机器确认；编译成功本身不代表桌面运行时已验证。

## JSSDK

```ts
import { createNotifyService } from './sdk.js'

const svc = createNotifyService()
await svc.start()                      // 页面初始化提前拉起(幂等,未装静默 false)
await svc.notify({ title: '工单提醒', body: '**紧急**工单\n\n第二行' })
// 服务未装/未跑时返回 { ok: false },不拉起不抛错
```

服务端启用 token 鉴权时,SDK 同步配置:

```ts
const svc = createNotifyService({ token: '与服务端 config.toml 一致' })
```

ESM 主产物 + UMD 兼容产物(`sdk.umd.js`,AMD 加载器/普通 script 标签),浏览器基线 Chrome 87;完整 API 见发行包内 `sdk-使用手册.md`。开发:`cd sdk/js && pnpm install && pnpm build`,演示页 `pnpm demo`。

## 已知限制

- Wayland 会话需桌面环境支持 Layer Shell 才能显示自绘弹窗;否则自动走系统通知
- Windows 兜底系统通知来源默认显示为 PowerShell,`--app-id` 可自定义
- 正文不加载图片或远程资源;链接不触发浏览器导航
