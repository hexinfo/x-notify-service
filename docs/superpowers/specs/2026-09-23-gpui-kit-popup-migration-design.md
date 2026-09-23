# GPUI Kit 通知窗口迁移设计

## 目标

将 `x-notify-service` 的自绘通知窗口从 iced 0.14 完整迁移到 GPUI Kit 0.6.6。除已明确改为 Markdown 的 `body` 格式语义外，保持现有字段契约和用户可见行为：

- HTTP、CLI、JS SDK 与演示页接口不变；
- 单窗口展示，后到通知覆盖当前内容；
- 请求级宽高和标题/正文颜色继续生效；
- `body` 改为 Markdown 正文，使用 GPUI Kit TextView 渲染并限制可见行数；
- 右下角定位、置顶、隐藏任务栏、关闭后可重新打开；
- 服务模式关闭窗口不退出进程，单发模式关闭窗口后退出；
- GUI 无法初始化或创建窗口失败时，服务继续运行并降级系统通知。

GPUI Kit 是 GPU 渲染框架。迁移后不再承诺无可用 GPU/驱动时显示自绘窗口；该场景走系统通知降级。

## 依赖选择

使用：

```toml
gpui-kit = { version = "0.6.6", default-features = false }
```

通过 `gpui-kit` 门面使用其固定配套的 GPUI 与 `gpui-base`。不启用 `gpui-component` 和默认图标资源，原因是通知窗口完全由请求颜色和既有布局定义，不需要完整组件主题、表单、表格或图标集合。

删除 iced 依赖及其专属 feature。保留 `x11rb` 只用于当前诊断能力确有需要的代码；GPUI 已覆盖的窗口类型和任务栏属性不再重复写入。

## 模块边界

### 保持不变

- `api.rs`：请求解析与校验；
- `server.rs`：HTTP 服务；
- `send.rs`：CLI 与服务转发；
- `fallback.rs`：系统通知；
- `popup.rs` 中与框架无关的尺寸、颜色和文本容量计算；
- SDK、演示页及安装协议。

### 重写

- `notify/app.rs`：GPUI Application、窗口实体、消息桥、生命周期、渲染和动画；
- `notify/popup.rs`：将 iced 类型替换为框架无关几何值或 GPUI 类型；
- `notify/window_icon.rs`：只保留图标解码；删除 GPUI 已原生处理的 X11 窗口类型、置顶和任务栏补丁；
- `html/**`：删除旧 HTML 子集解析器，新增轻量 Markdown 纯文本投影，仅供系统通知降级；
- `main.rs::serve`：协调 GPUI 初始化成功和系统通知降级模式。

## 运行模型

### 服务模式

1. 获取单实例锁并完成配置解析。
2. 启动 HTTP 服务，初始 `POPUP_AVAILABLE=false`。
3. 在主线程构造 `gpui_kit::application()`，设置 `QuitMode::Explicit` 并进入事件循环。
4. GPUI 初始化完成后创建消息接收任务，设置 `POPUP_AVAILABLE=true`。
5. HTTP 工作线程只向消息桥发送普通 Rust 数据，不持有或访问 GPUI `Entity`。
6. GPUI 退出、初始化异常或窗口创建失败时清除 `POPUP_AVAILABLE`；HTTP 服务继续运行，后续通知走系统通知。

GPUI 构造和运行边界使用受控的 `catch_unwind` 转换为明确错误；若 HTTP 服务已经启动，不重复绑定端口。失败后主线程进入现有无 GUI 驻留路径。

### 单发模式

单发命令直接启动 GPUI，预置一条通知。关闭按钮、整窗点击或系统关闭请求执行 `cx.quit()`；GUI 初始化失败则发送一次系统通知并退出。

### 跨线程消息桥

继续使用单生产接口 `post(Message) -> bool`。内部改为 `futures::channel::mpsc::UnboundedSender`：

- Sender 可安全存放在全局互斥槽并由 HTTP 线程调用；
- Receiver 在 GPUI 前台 `cx.spawn` 任务中消费；
- 每条消息只在 GPUI 线程更新窗口实体；
- 通道关闭或发送失败时返回 `false`，触发现有系统通知降级。

## 窗口生命周期

状态只保留一个窗口句柄和一个 `Entity<PopupView>`：

- 无窗口时收到通知：按请求尺寸和右下角位置创建窗口；
- 已有窗口时收到通知：更新实体内容、颜色和排版，调整窗口尺寸和位置，并重新激活置顶；
- 关闭时移除窗口、清空句柄和 hover/动画瞬时状态；
- 下一条通知重新创建窗口；任何时刻最多存在一个通知窗口。

GPUI 公共 API可运行时调整内容尺寸，但没有统一的跨平台窗口移动 API。位置与尺寸同步封装在单独的平台适配层：

- Windows：从 raw-window-handle 取得 HWND，用 `SetWindowPos` 原子更新位置、尺寸和 `HWND_TOPMOST`；
- macOS：通过现有 objc2 依赖设置 NSWindow frame；
- Linux X11：通过 raw XCB/Xlib 窗口句柄配置位置与尺寸；
- Wayland Layer Shell：通过右/下 anchor、margin 和 surface size 更新，不使用绝对坐标。

平台适配只处理 GPUI 尚未公开的运行时移动；窗口类型、任务栏和置顶优先依赖 GPUI 原生 `WindowKind`。

## 平台窗口策略

### Windows

使用 `WindowKind::PopUp`、无原生标题栏、不可缩放、创建时不抢焦点。GPUI Windows 后端会为 PopUp 使用 `WS_EX_TOOLWINDOW | WS_EX_TOPMOST`，因此不出任务栏并保持置顶，无需再次通过标题搜索窗口。

### Linux X11

使用 `WindowKind::PopUp`。GPUI X11 后端会创建 override-redirect 窗口并写入 `_NET_WM_WINDOW_TYPE_NOTIFICATION`。删除旧的窗口标题搜索和延迟补写逻辑，避免修改错误 XID。

### Linux Wayland

启用 GPUI 自带 Wayland 后端，优先使用 `WindowKind::LayerShell`：

- layer 为 Overlay；
- anchor 为 Right + Bottom；
- margin 为 14 逻辑像素；
- keyboard interactivity 关闭。

若 compositor 不支持 Layer Shell 或创建失败，标记弹窗不可用并降级系统通知。

### macOS

使用 `WindowKind::PopUp`、无系统装饰、不可缩放、不抢焦点。创建后把原生 NSPanel 样式收敛为 Borderless + Nonactivating，确保四角为直角且不激活应用；位置继续以可见工作区右下角计算，GPUI 使用 Metal 渲染。

## 渲染与字体

通知视图由 GPUI Kit 基础元素直接绘制：

- 根节点：固定尺寸、1px 外边框、纵向布局、四角直角；
- Header：38px 高，请求级背景和文字颜色；
- 标题：单行省略，平台 UI 字体，Semibold；
- 关闭区域：38×38 完整点击区，无静态分隔线，仅 hover 背景；
- 关闭图形：绘制两条对角线，不使用 `×` 字符，消除字体基线差异；
- Body：请求级背景与默认文字颜色，使用 `gpui_kit::base::TextView::markdown` 渲染正文。

TextView 使用 CommonMark/GFM 兼容解析，关闭选择和滚动，并用 `max_lines` 按窗口高度限制可见正文；超出部分由 TextView 省略。正文主题通过 `TextViewStyle` 覆盖 foreground、muted、link、code background 等语义颜色。Markdown 的 `**粗体**` 只改变 `FontWeight`，不切换字体族，因此 Windows DirectWrite、Linux 文本后端和 macOS CoreText 会在同一次 shaping 中对齐 baseline。

通知不加载 Markdown 图片或远程资源；图片节点显示简短替代文本，链接只渲染样式、不打开浏览器。标题、列表、引用、行内代码、代码块等 Markdown 结构采用紧凑通知样式，不能突破正文区域或窗口行数限制。

字体族继续按平台选择：

- macOS：PingFang SC；
- Windows：Microsoft YaHei UI；
- Linux：Noto Sans CJK SC，缺失时由 GPUI 字体回退。

标题使用 15px Semibold；正文默认 14px，行高 1.45。正文不再支持 HTML `font-size` 或内联 CSS；字号由通知 Markdown 样式统一控制。

## 正文契约迁移

HTTP 与 SDK 仍使用 `body: string`，但语义从旧 HTML 子集改为 Markdown：

- `<b>1</b>`、`<font>`、`<span style>` 等旧 HTML 不再作为格式语法；
- 粗体改为 `**1**`，换行使用 Markdown 换行或空行；
- 正文颜色仍由 `bodyTextColor` 统一控制，不支持 Markdown 内嵌任意颜色；
- 系统通知降级使用 Markdown AST 的纯文本投影，不显示 `**`、反引号、链接地址等格式标记；
- SDK 类型不变，但注释、演示页、使用手册和测试示例全部改为 Markdown。

这是明确的正文语义变更，不保留 HTML 兼容解析器。

## 动画与交互

- 入场动画保持 220ms ease-out cubic；仅内容从右侧偏移 26px，窗口和边框不移动；
- 点击窗口任意位置关闭；
- 关闭区域 hover 只改变背景；
- 新通知到达时不重新播放整窗动画，只更新内容、尺寸、位置和置顶状态；
- 关闭时清除 hover，避免重开后残留悬浮态。

动画使用 GPUI/GPUI Base 的前台定时和 `cx.notify()`，不再创建每帧一个 `std::thread`。

## 错误处理与降级

- GPUI 平台初始化失败：记录错误，`POPUP_AVAILABLE=false`，服务继续以系统通知工作；
- 窗口创建失败：当前通知立即走系统通知，后续通知保持降级，避免每条重复初始化；
- 消息桥未就绪或断开：`post` 返回 false，调用方走系统通知；
- 平台运行时定位失败：保留窗口显示并记录告警，不将已经展示的通知重复投递到系统通知；
- Wayland Layer Shell 不支持：降级系统通知。

## 测试与验收

### 自动测试

- 保留现有 API、尺寸、颜色和 HTTP 测试，并把正文用例迁移为 Markdown；
- 新增消息状态机测试：首次打开、可见更新、关闭清理、单发退出；
- 新增 Markdown 渲染和纯文本投影测试：普通文本、粗体、链接、列表、代码、换行、行数限制和图片替代文本；
- 使用 `gpui-kit` 的 `test-support` 覆盖布局：Header 38px、关闭区 38×38、Body 填充、请求尺寸；
- 验证任何生产代码和依赖树中不再引用 iced；
- Rust `fmt`、全量测试、Clippy `-D warnings`；
- JS SDK build/test/typecheck/lint；
- Windows、Linux x86_64/aarch64 CI 编译打包。

### 运行时验收

- macOS：本机实际弹窗截图、动态尺寸/颜色、关闭重开、hover 清理；
- Windows：真机确认不出任务栏、始终置顶、字体 baseline、动态尺寸和关闭重开；
- Linux X11：目标 WM 使用 `xprop` 确认通知类型且无任务栏条目；
- Linux Wayland：支持 Layer Shell 的桌面确认右下角锚定；不支持时确认降级系统通知。

编译或 headless 测试不能替代 Windows/Linux 桌面运行时证明。未取得目标机器证据时，交付说明必须明确该边界。

## 迁移顺序

1. 引入 GPUI Kit 最小依赖并建立可编译的无窗口 Application；
2. 删除 HTML 子集链路，接入 GPUI Base Markdown TextView 与纯文本投影；
3. 实现 GPUI PopupView 与交互；
4. 接入消息桥、服务/单发生命周期；
5. 实现跨平台窗口创建、定位和尺寸更新；
6. 删除 iced 和旧 X11 补丁；
7. 完成自动测试、macOS 视觉验收和三平台 CI。

## 非目标

- 不修改 HTTP/SDK 字段或认证/CORS 行为；`body` 的格式语义明确迁移为 Markdown；
- 不新增通知堆叠、历史列表、超时自动关闭或声音；
- 不引入完整 GPUI Component 主题系统；
- 不在本次迁移中改变安装器、协议、开机启动或系统通知样式；
- 不为无 GPU 环境保留另一套 iced/软件渲染窗口。
