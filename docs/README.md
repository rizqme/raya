# Raya Documentation

VitePress documentation site for the Raya programming language.

## Development

```bash
cd docs
npm install           # Install dependencies (already done)
npm run docs:dev      # Start dev server at http://localhost:5173
npm run docs:build    # Build for production
npm run docs:preview  # Preview production build
```

## Structure

```text
docs/
├── index.md                  # Homepage (hero + features + examples)
├── getting-started.md        # Installation + first programs
├── why-raya.md              # Philosophy + design rationale
├── language/
│   ├── types.md             # Type system reference
│   └── concurrency.md       # Tasks + goroutine-style concurrency
├── stdlib/
│   ├── overview.md          # Stdlib organization
│   └── io.md                # std:io API reference
├── .vitepress/
│   ├── config.ts            # Site configuration
│   └── theme/
│       ├── index.ts         # Theme entry point
│       └── custom.css       # Red accent theme
└── public/
    ├── raya-logo.svg        # Logo
    └── icons/               # Flat SVG icons for features
        ├── zap.svg
        ├── target.svg
        ├── code.svg
        ├── cpu.svg
        ├── link.svg
        └── package.svg
```

## Content Verification

All code examples have been verified against actual implementations:

### Logger Module
✅ `import logger from "std:logger"` - singleton instance
✅ Methods: `debug()`, `info()`, `warn()`, `error()`
✅ Location: `crates/raya-stdlib/raya/logger.raya`

### IO Module
✅ `import io from "std:io"` - singleton instance
✅ Methods: `writeln()`, `write()`, `readLine()`, `readAll()`, `writeErr()`, `writeErrln()`
✅ Location: `crates/raya-stdlib-posix/raya/io.raya`

### Time Module
✅ `import time from "std:time"` 
✅ Methods: `monotonic()`, `elapsed()`, `sleep()`

### Concurrency
✅ `async` prefix creates Tasks immediately
✅ `await` suspends current Task
✅ Work-stealing scheduler

## Theme

Custom theme with Raya brand colors:
- Primary: `#E70000` (red from logo)
- Light mode: `#d60000`
- Dark mode: `#E70000`

Features:
- Logo-only navigation (no text)
- Centered hero section
- Flat icon set (Lucide-based)
- Local search enabled
- Responsive layout

## Adding Content

### New Documentation Page

1. Create markdown file in appropriate directory
2. Update `.vitepress/config.ts` sidebar
3. Use VitePress markdown features:
   - Code syntax highlighting
   - Custom containers (`::: warning`, `::: tip`)
   - Frontmatter for metadata

Example:

```markdown
# Page Title

## Section

Content here...

::: warning Early Project
Raya is in active development.
:::
```

### New Stdlib Module Doc

1. Check actual implementation in `crates/raya-stdlib*/src/`
2. Verify API in corresponding `.raya` file
3. Create `docs/stdlib/modulename.md`
4. Add to sidebar in config.ts
5. Include:
   - Import statement
   - Method signatures
   - Code examples
   - Notes about blocking/async

## Navigation Structure

```text
Home
├─ Guide
│  ├─ Getting Started
│  └─ Why Raya?
├─ Language
│  ├─ Type System
│  └─ Concurrency
└─ Stdlib
   ├─ Overview
   └─ std:io
```

## Build & Deploy

For GitHub Pages:

```bash
cd docs
npm run docs:build
# Output in docs/.vitepress/dist/
```

Configure GitHub Pages to serve from `/docs/.vitepress/dist` or push to `gh-pages` branch.

## Notes

- All I/O operations are synchronous (concurrency via `async` prefix)
- Examples use `io.writeln()` instead of `console.log()`
- Type system is fully static (no `any` type)
- Discriminated unions require explicit discriminant field
- Generics are monomorphized at compile time
