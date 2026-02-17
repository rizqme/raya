import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'Raya',
  description: 'A statically-typed language with TypeScript syntax, compiled to custom bytecode with goroutine-style concurrency.',
  base: '/raya/',

  markdown: {
    config(md) {
      // Escape angle brackets that look like generic type params (e.g. <T>, <Object>)
      // so Vue's template compiler doesn't treat them as HTML elements
      const defaultHtmlInline = md.renderer.rules.html_inline
      md.renderer.rules.html_inline = (tokens, idx, options, env, self) => {
        const content = tokens[idx].content
        if (/^<\/?[A-Z][a-zA-Z]*>$/.test(content)) {
          return content.replace(/</g, '&lt;').replace(/>/g, '&gt;')
        }
        return defaultHtmlInline
          ? defaultHtmlInline(tokens, idx, options, env, self)
          : content
      }
    }
  },

  themeConfig: {
    nav: [
      { text: 'Overview', link: '/overview' },
      { text: 'Language', link: '/language/lang' },
      { text: 'GitHub', link: 'https://github.com/rizqme/raya' }
    ],

    sidebar: [
      {
        text: 'Getting Started',
        items: [
          { text: 'Overview', link: '/overview' },
        ]
      },
      {
        text: 'Language',
        collapsed: false,
        items: [
          { text: 'Language Specification', link: '/language/lang' },
          { text: 'Numeric Types', link: '/language/numeric-types' },
          { text: 'typeof Design', link: '/language/typeof-design' },
          { text: 'JSON Type', link: '/language/json-type' },
          { text: 'Exception Handling', link: '/language/exception-handling' },
        ]
      },
      {
        text: 'Compiler',
        collapsed: false,
        items: [
          { text: 'Opcodes', link: '/compiler/opcode' },
          { text: 'Compilation Mapping', link: '/compiler/mapping' },
          { text: 'File Formats', link: '/compiler/formats' },
        ]
      },
      {
        text: 'Runtime',
        collapsed: false,
        items: [
          { text: 'VM Architecture', link: '/runtime/architecture' },
          { text: 'Module System', link: '/runtime/modules' },
          { text: 'Built-in Classes', link: '/runtime/builtin-classes' },
        ]
      },
      {
        text: 'Standard Library',
        collapsed: false,
        items: [
          { text: 'API Reference', link: '/stdlib/stdlib' },
        ]
      },
      {
        text: 'Metaprogramming',
        collapsed: false,
        items: [
          { text: 'Reflection API', link: '/metaprogramming/reflection' },
          { text: 'Reflection Security', link: '/metaprogramming/reflect-security' },
          { text: 'Decorators', link: '/metaprogramming/decorators' },
        ]
      },
      {
        text: 'Native',
        collapsed: true,
        items: [
          { text: 'Native ABI', link: '/native/abi' },
          { text: 'Native Bindings', link: '/native/native-bindings' },
        ]
      },
      {
        text: 'Advanced',
        collapsed: true,
        items: [
          { text: 'Inner VM', link: '/advanced/inner-vm' },
          { text: 'Dynamic VM Bootstrap', link: '/advanced/dynamic-vm-bootstrap' },
        ]
      },
      {
        text: 'Future',
        collapsed: true,
        items: [
          { text: 'Channels', link: '/future/channels' },
          { text: 'VM Snapshotting', link: '/future/snapshotting' },
          { text: 'TSX/JSX Support', link: '/future/tsx' },
        ]
      },
      {
        text: 'Tooling',
        collapsed: true,
        items: [
          { text: 'CLI', link: '/tooling/cli' },
        ]
      },
    ],

    socialLinks: [
      { icon: 'github', link: 'https://github.com/rizqme/raya' }
    ],

    search: {
      provider: 'local'
    },

    outline: {
      level: [2, 3]
    },

    editLink: {
      pattern: 'https://github.com/rizqme/raya/edit/main/docs/:path',
      text: 'Edit this page on GitHub'
    }
  }
})
