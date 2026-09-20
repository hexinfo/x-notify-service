import { defineConfig } from 'vite'

export default defineConfig({
  build: {
    // 浏览器基线钉 Chrome/Chromium 87,esbuild 负责语法降级(两格式同基线)
    target: 'chrome87',
    lib: {
      entry: 'src/index.ts',
      // umd 兼容 AMD 加载器(RequireJS 等)与普通 script 标签(全局 XNotifyServiceSdk)
      name: 'XNotifyServiceSdk',
      formats: ['es', 'umd'],
      fileName: (format) =>
        format === 'es' ? 'x-notify-service-sdk.js' : 'x-notify-service-sdk.umd.js',
    },
    minify: true,
  },
})
