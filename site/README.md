# ferric-rules site

Static Astro/Starlight site for ferric-rules.

## Commands

```sh
npm install
npm run dev
npm run build
```

The production config targets `https://plx.github.io/ferric-rules` using:

- `site: "https://plx.github.io"`
- `base: "/ferric-rules"`

The deploy workflow is configured in `.github/workflows/site.yml` and builds from this `site/` directory.
