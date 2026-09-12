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

## Generated compatibility reference

The compatibility probe page is generated from live executions of the entire
granular corpus. Site builds, Astro checks, and development startup first compile
and run Ferric, run every probe against the CLIPS Docker reference, and write
`src/generated/compatibility.json`. This file is ignored by Git; a failed probe or
reference check stops the build instead of publishing old results.

In addition to Node, install the repository's Rust toolchain, `uv` with Python
3.12 or newer, and Docker. From the repository root, prepare the reference once:

```sh
docker build -t ferric-rules/clips-reference:latest docker/clips-reference/
just compat-docs
```

Then use the normal site commands. `npm run build` always regenerates the data.
After changing engine code or probes while a development server is running, run
`npm run compatibility:generate` again. The page includes the source revision,
input digest, generation time, CLIPS version, and immutable reference image ID.
CI checks and publishing rebuild the reference image and regenerate the page
when the engine, corpus, generator, reference image, or site changes.

Mismatch policy lives in `tests/clips_compat/corpus/dispositions.json`; probe
source, expected reference output, and characterization assertions remain in the
corpus. A fix requires updating its characterization before the site can build.

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
