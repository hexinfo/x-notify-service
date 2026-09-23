/** 实际展示渠道:popup=右下角置顶弹窗,system=系统通知兜底 */
export type NotifyVia = 'popup' | 'system'

export interface NotifyServiceOptions {
  /** 端口探测起始值,须与服务端配置一致,默认 17320 */
  readonly basePort?: number
  /** 探测端口个数,须与服务端配置一致,默认 10 */
  readonly portRange?: number
  /** notify 单次请求超时(毫秒),默认 3000 */
  readonly requestTimeoutMs?: number
  /** 与服务端 config.toml 的 token 一致时携带 X-Token;不配置则不鉴权(默认) */
  readonly token?: string
}

export interface NotifyOptions {
  /** 通知标题(必填,最长 200 字符) */
  readonly title: string
  /** 通知正文(最长 2000 字符);使用 CommonMark/GFM 兼容 Markdown。旧 HTML 子集不兼容,原样 HTML 不作为格式解析;链接不导航,图片及远程资源不加载(图片显示替代文本)。正文颜色由 bodyTextColor 统一控制。弹窗常驻不超时,点击关闭或被新通知顶掉 */
  readonly body?: string
  /** 弹窗宽度(逻辑像素,220-800);缺省 220 */
  readonly width?: number
  /** 弹窗高度(逻辑像素,80-600);缺省 100 */
  readonly height?: number
  /** 标题栏背景色(#RRGGBB) */
  readonly headerBackgroundColor?: string
  /** 标题文字颜色(#RRGGBB) */
  readonly headerTextColor?: string
  /** 正文背景色(#RRGGBB) */
  readonly bodyBackgroundColor?: string
  /** 正文文字颜色(#RRGGBB);Markdown 正文不支持内嵌颜色 */
  readonly bodyTextColor?: string
}

/** 通知结果:ok=false 表示未送达(服务未安装/未运行,SDK 静默失败不抛错,由业务自理) */
export interface NotifyResult {
  readonly ok: boolean
  readonly via?: NotifyVia
}

export interface NotifyService {
  /** 在端口区间内探测服务;返回 baseUrl(如 http://127.0.0.1:17321)或 null */
  discover(force?: boolean): Promise<string | null>
  /** 提前拉起服务:页面初始化时对接入角色调用,避免通知时刻才冷启动;
   *  幂等,未安装/超时静默返回 false */
  start(timeoutMs?: number): Promise<boolean>
  /** 发送通知(即打开/更新弹窗,新通知顶掉旧的)。
   *  静默失败语义:服务未安装/未运行时不拉起、不抛错,返回 { ok: false } 交给业务自理;
   *  页面初始化时先 start() 提前拉起。 */
  notify(options: NotifyOptions): Promise<NotifyResult>
  /** 显式关闭当前弹窗(幂等);对应老系统 ClosePopup() */
  close(): Promise<void>
  /** 清空已缓存的 baseUrl */
  destroy(): void
}
