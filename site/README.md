# ferric-rules site

Static Astro/Starlight site generated from `static-tool-page-template`.

## Common commands

```sh
just install
just dev
just check
just test
just build
```

The site is configured for `https://plx.github.io/ferric-rules/` with the GitHub Pages base path `/ferric-rules`.

The generated Playwright suite runs against mobile, tablet, and desktop projects.
Use `just install-browsers` once locally before `just test`.

## Toolchain notes

- **Astro 7 / Starlight 0.41 / TypeScript 7.** The site targets Node 24 (Active
  LTS). `just build` uses Astro 7 (Vite 8 + the Rust compiler).
- **Two type-checkers, on purpose.** `npm run check` runs `astro check`, which is
  Volar-based and still requires the TypeScript 6 programmatic API (Volar tools
  cannot consume TypeScript 7 until its stable programmatic API lands, tracked in
  [withastro/roadmap#1321](https://github.com/withastro/roadmap/discussions/1321)).
  `npm run typecheck` runs the TypeScript 7 native compiler (`tsgo`, from
  `@typescript/native-preview`) over the plain `.ts`/`.mjs` sources. Both run in
  CI. Once `@astrojs/check` supports TypeScript 7, collapse these back into a
  single `typescript@^7` dependency and drop `@typescript/native-preview`.
- **`astro dev` daemonizes in agent/CI-like environments.** Astro 7 detects such
  environments and starts the dev server in the background, returning
  immediately. Manage it with `astro dev status`, `astro dev logs`, and
  `astro dev stop`. Because of this, the Playwright suite serves the built site
  with `astro preview` (always foreground) rather than `astro dev`.
