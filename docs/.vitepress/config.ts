import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'Raya',
  description: 'TypeScript syntax with goroutine-style concurrency',
  
  themeConfig: {
    logo: '/raya-logo.svg',
    siteTitle: false,
    
    search: {
      provider: 'local'
    },
    
    nav: [
      { text: 'Home', link: '/' },
      { text: 'Guide', link: '/getting-started' },
      { text: 'Language', link: '/language/types' },
      { text: 'Stdlib', link: '/stdlib/overview' },
      { text: 'GitHub', link: 'https://github.com/rizqme/raya' }
    ],

    sidebar: [
      {
        text: 'Introduction',
        items: [
          { text: 'Getting Started', link: '/getting-started' },
          { text: 'Why Raya?', link: '/why-raya' },
        ]
      },
      {
        text: 'Language',
        items: [
          { text: 'Type System', link: '/language/types' },
          { text: 'Concurrency', link: '/language/concurrency' },
        ]
      },
      {
        text: 'Standard Library',
        collapsed: false,
        items: [
          { text: 'Overview', link: '/stdlib/overview' },
          { text: 'std:io', link: '/stdlib/io' },
        ]
      }
    ],

    socialLinks: [
      { icon: 'github', link: 'https://github.com/rizqme/raya' }
    ],

    footer: {
      message: 'MIT OR Apache-2.0 License',
      copyright: 'Copyright © 2026'
    }
  },

  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/raya-logo.svg' }]
  ]
})
