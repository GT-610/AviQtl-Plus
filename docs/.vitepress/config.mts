import { defineConfig } from 'vitepress'

export default defineConfig({
  lang: 'zh-CN',
  title: 'AviQtl',
  description: 'AviQtl 用户与开发者文档',
  cleanUrls: true,
  themeConfig: {
    nav: [
      { text: '首页', link: '/' }
    ]
  }
})

