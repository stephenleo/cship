import { defineConfig } from 'vitepress'

const base = '/'

export default defineConfig({
  title: 'CShip',
  description: 'Beautiful, Blazing-fast, Customizable Claude Code Statusline',
  base,

  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: `${base}favicon.svg` }],
  ],

  themeConfig: {
    logo: '/logo.png',
    siteTitle: '⚓ CShip',

    nav: [
      { text: 'Guide', link: '/' },
      { text: 'Configuration', link: '/configuration' },
      { text: 'Passthrough', link: '/passthrough' },
      { text: 'Showcase', link: '/showcase' },
      { text: 'FAQ', link: '/faq' },
      { text: 'Contributing', link: '/contributing' },
      {
        text: 'GitHub',
        link: 'https://github.com/stephenleo/cship',
      },
    ],

    sidebar: [
      {
        text: 'Getting Started',
        items: [
          { text: 'Introduction', link: '/' },
          { text: 'Configuration', link: '/configuration' },
        ],
      },
      {
        text: 'Advanced',
        items: [
          { text: 'Starship Passthrough', link: '/passthrough' },
        ],
      },
      {
        text: 'Community',
        items: [
          { text: 'Showcase', link: '/showcase' },
          { text: 'FAQ', link: '/faq' },
          { text: 'Contributing', link: '/contributing' },
        ],
      },
    ],

    socialLinks: [
      { icon: 'github', link: 'https://github.com/stephenleo/cship' },
    ],

    footer: {
      message: 'Released under the Apache-2.0 License.',
      copyright: 'Copyright © CShip contributors',
    },

    search: {
      provider: 'local',
    },
  },
})
