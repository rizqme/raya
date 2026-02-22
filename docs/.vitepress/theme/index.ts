import DefaultTheme from 'vitepress/theme'
import './custom.css'

// Import lucide icons
import { Zap, Target, Code2, Cpu, Link2, Package } from 'lucide-vue-next'

export default {
  extends: DefaultTheme,
  enhanceApp({ app }) {
    // Register icons globally
    app.component('IconZap', Zap)
    app.component('IconTarget', Target)
    app.component('IconCode', Code2)
    app.component('IconCpu', Cpu)
    app.component('IconLink', Link2)
    app.component('IconPackage', Package)
  }
}

