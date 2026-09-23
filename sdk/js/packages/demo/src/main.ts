import { createNotifyService } from '@hexinfo/x-notify-service-sdk'

const bridge = createNotifyService()

const $ = (id: string): HTMLElement => {
  const el = document.getElementById(id)
  if (el === null) {
    throw new Error(`element #${id} not found`)
  }
  return el
}

function log(message: string): void {
  const el = $('log')
  const time = new Date().toTimeString().slice(0, 8)
  el.textContent += `[${time}] ${message}\n`
}

$('btn-discover').addEventListener('click', () => {
  $('status').textContent = '探测中…'
  bridge
    .discover(true)
    .then((base) => {
      const text = base === null ? '未运行' : `在线 ${base}`
      $('status').textContent = text
      log(`探测结果: ${text}`)
    })
    .catch((e: unknown) => log(`探测异常: ${String(e)}`))
})

$('btn-launch').addEventListener('click', () => {
  $('status').textContent = '拉起中…'
  bridge.start(10000).then((ok) => {
    $('status').textContent = ok ? '在线' : '未安装/超时'
    log(ok ? '服务已就绪' : '拉起失败(未安装或超时,静默)')
  })
})

function send(): void {
  const title = ($('inp-title') as HTMLInputElement).value
  const body = ($('inp-body') as HTMLInputElement).value
  // 宽高可选:留空不传字段(走服务端配置与默认)
  const opts: { title: string; body: string; width?: number; height?: number } = { title, body }
  const width = ($('inp-width') as HTMLInputElement).value.trim()
  const height = ($('inp-height') as HTMLInputElement).value.trim()
  if (width !== '') {
    opts.width = Number(width)
  }
  if (height !== '') {
    opts.height = Number(height)
  }
  bridge
    .notify(opts)
    .then((result) =>
      log(result.ok ? `发送成功: via=${result.via}` : '未送达(服务未运行,静默失败)'),
    )
    .catch((e: unknown) => log(`发送失败: ${String(e)}`))
}

$('btn-send').addEventListener('click', () => {
  send()
})
