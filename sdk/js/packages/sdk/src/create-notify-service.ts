import { APP_ID, DEFAULT_BASE_PORT, DEFAULT_PORT_RANGE, PROTOCOL_URL } from './constants'
import type { NotifyOptions, NotifyResult, NotifyService, NotifyServiceOptions } from './types'

/** /health 响应体(仅校验身份字段) */
interface HealthBody {
  readonly app?: unknown
}

/**
 * 身份校验:响应体是否来自本服务
 *
 * @param data - /health 解析出的 JSON
 * @returns 是否为 x-notify-service 实例
 */
function isHealth(data: unknown): data is HealthBody {
  if (data === null || typeof data !== 'object') {
    return false
  }
  return (data as HealthBody).app === APP_ID
}

/**
 * 睡眠等待
 *
 * @param ms - 等待毫秒数
 */
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms)
  })
}

/**
 * 创建通知服务客户端
 *
 * 经端口区间探测 + /health 身份校验发现本机服务;
 * notify 静默失败(未安装/未运行返回 { ok: false },不拉起不抛错),
 * 页面初始化时应对需要通知能力的角色调用 start() 提前拉起。
 *
 * @param options - 端口与超时配置(须与服务端一致)
 * @returns 通知服务客户端实例
 */
export function createNotifyService(options: NotifyServiceOptions = {}): NotifyService {
  const basePort = options.basePort ?? DEFAULT_BASE_PORT
  const portRange = options.portRange ?? DEFAULT_PORT_RANGE
  const requestTimeoutMs = options.requestTimeoutMs ?? 3000
  const authHeaders = options.token === undefined ? {} : { 'X-Token': options.token }

  let cachedBaseUrl: string | null = null
  let discovering: Promise<string | null> | null = null

  /** GET /health,只认响应中的 APP_ID 身份标识 */
  function fetchHealth(baseUrl: string, timeoutMs: number): Promise<unknown> {
    return new Promise((resolve) => {
      const controller = new AbortController()
      const timer = setTimeout(() => {
        controller.abort()
      }, timeoutMs)
      fetch(`${baseUrl}/health`, { signal: controller.signal, cache: 'no-store' })
        .then((res) => {
          if (res.ok) {
            return res.json() as Promise<unknown>
          }
          return null
        })
        .then((data) => {
          clearTimeout(timer)
          resolve(isHealth(data) ? data : null)
        })
        .catch(() => {
          clearTimeout(timer)
          resolve(null)
        })
    })
  }

  /** 顺序探测端口区间,命中即停:默认端口在线时零失败请求(控制台无报错) */
  async function discover(force = false): Promise<string | null> {
    if (!force && cachedBaseUrl) {
      return cachedBaseUrl
    }
    if (discovering) {
      return discovering
    }
    discovering = (async () => {
      for (let i = 0; i < portRange; i++) {
        const port = basePort + i
        // 300ms 超时仅对"有监听但慢响应"有效;回环上无人监听是立刻 refused
        if ((await fetchHealth(`http://127.0.0.1:${port}`, 300)) !== null) {
          cachedBaseUrl = `http://127.0.0.1:${port}`
          return cachedBaseUrl
        }
      }
      cachedBaseUrl = null
      return null
    })()
    const result = await discovering
    discovering = null
    return result
  }

  /** 经隐藏 iframe 触发 x-notify:// 协议拉起 */
  function launchViaProtocol(): void {
    if (typeof document === 'undefined') {
      return
    }
    const iframe = document.createElement('iframe')
    iframe.style.display = 'none'
    iframe.src = PROTOCOL_URL
    document.body.appendChild(iframe)
    setTimeout(() => {
      if (iframe.parentNode !== null) {
        iframe.parentNode.removeChild(iframe)
      }
    }, 5000)
  }

  /**
   * 提前拉起服务:业务页面初始化时(对需要通知能力的角色)调用,
   * 避免等到发通知那一刻才冷启动。幂等;未安装时静默返回 false。
   */
  async function start(timeoutMs = 5000): Promise<boolean> {
    if ((await discover(true)) !== null) {
      return true
    }
    if (typeof document === 'undefined') {
      return false
    }
    launchViaProtocol()
    const deadline = Date.now() + timeoutMs
    while (Date.now() < deadline) {
      await sleep(300)
      if ((await discover(true)) !== null) {
        return true
      }
    }
    return false // 未安装(协议未注册)或启动超时,静默
  }

  /** POST /notify —— 不设置 Content-Type(默认 text/plain),属 CORS 简单请求,零预检 */
  function postNotify(baseUrl: string, payload: NotifyOptions): Promise<NotifyResult | null> {
    // 服务端字段为 snake_case
    const body: Record<string, string | number> = { title: payload.title }
    if (payload.body !== undefined) {
      body.body = payload.body
    }
    if (payload.width !== undefined) {
      body.width = payload.width
    }
    if (payload.height !== undefined) {
      body.height = payload.height
    }
    return new Promise((resolve) => {
      const controller = new AbortController()
      const timer = setTimeout(() => {
        controller.abort()
      }, requestTimeoutMs)
      fetch(`${baseUrl}/notify`, {
        method: 'POST',
        body: JSON.stringify(body),
        headers: authHeaders,
        signal: controller.signal,
      })
        .then((res) => {
          if (!res.ok) {
            clearTimeout(timer)
            resolve(null)
            return
          }
          return (res.json() as Promise<unknown>).then((data) => {
            clearTimeout(timer)
            if (
              data !== null &&
              typeof data === 'object' &&
              (data as { ok?: unknown }).ok === true
            ) {
              const via = (data as { via?: unknown }).via
              resolve({ ok: true, via: via === 'system' ? 'system' : 'popup' })
              return
            }
            resolve(null)
          })
        })
        .catch(() => {
          clearTimeout(timer)
          resolve(null)
        })
    })
  }

  /** 静默失败语义:服务未安装/未运行时不拉起、不抛错,返回 { ok: false } */
  async function notify(opts: NotifyOptions): Promise<NotifyResult> {
    if (typeof opts.title !== 'string' || opts.title.trim() === '') {
      throw new Error('title 不能为空')
    }

    let base = await discover()
    if (base === null) {
      // 可能是服务刚启动(install/拉起后的头一两秒,HTTP 尚未就绪):
      // 短暂重探覆盖启动窗口;仍未果视为未运行,静默交还业务
      for (let i = 0; i < 3 && base === null; i++) {
        await sleep(500)
        base = await discover(true)
      }
      if (base === null) {
        return { ok: false }
      }
    }
    const result = await postNotify(base, opts)
    if (result !== null) {
      return result
    }
    cachedBaseUrl = null // 缓存失效(服务疑似已退出),同样静默
    return { ok: false }
  }

  /** 显式关闭当前弹窗(幂等);服务不在线时视为无弹窗可关,静默成功 */
  async function close(): Promise<void> {
    const base = await discover()
    if (base === null) {
      return
    }
    await fetch(`${base}/close`, { method: 'POST', headers: authHeaders }).catch(() => undefined)
  }

  /** 清空已缓存的 baseUrl(下次调用重新探测) */
  function destroy(): void {
    cachedBaseUrl = null
  }

  return {
    discover,
    start,
    notify,
    close,
    destroy,
  }
}
