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
x-notify-service notify -t "标题" [-b "<b>正文</b>"] [-f]   # 手测通知;-f 走系统通知
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
POST /notify → {"ok":true,"via":"popup"|"system"}   body: {"title":"≤200字", "body":"≤2000字,HTML子集", "width":"220-800可选", "height":"80-600可选"}
POST /close  → {"ok":true}
```

正文 HTML 子集:`<b>`、颜色、字号(11~18,按行)、`<br>`、实体;其余剥除,超行数截断加 …。弹窗常驻不超时,新通知顶掉旧的(不堆叠);尺寸缺省 327×106,请求 `width`/`height` 可覆盖。

## JSSDK

```ts
import { createNotifyService } from './sdk.js'

const svc = createNotifyService()
await svc.start()                      // 页面初始化提前拉起(幂等,未装静默 false)
await svc.notify({ title: '工单提醒', body: '<b>紧急</b>工单<br>第二行' })
// 服务未装/未跑时返回 { ok: false },不拉起不抛错
```

服务端启用 token 鉴权时,SDK 同步配置:

```ts
const svc = createNotifyService({ token: '与服务端 config.toml 一致' })
```

ESM 主产物 + UMD 兼容产物(`sdk.umd.js`,AMD 加载器/普通 script 标签),浏览器基线 Chrome 87;完整 API 见发行包内 `sdk-使用手册.md`。开发:`cd sdk/js && pnpm install && pnpm build`,演示页 `pnpm demo`。

## 已知限制

- Wayland 会话弹窗无法自定位,自动走系统通知(X11 正常)
- Windows 兜底系统通知来源默认显示为 PowerShell,`--app-id` 可自定义
- 正文不支持表格/图片/复杂 CSS(设计取舍)
