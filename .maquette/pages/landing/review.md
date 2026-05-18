# Landing Page Review

Status: concept approved

## Concept

- Concept image: `.maquette/pages/landing/concept.png`
- Source image: `/Users/prb/.codex/generated_images/019e1ca4-23b6-7f10-ab9a-6987a3d1d0ae/ig_071b3337829c93e9016a033ca8f2048197bda99ee33b67ca9b.png`

## Inspection Notes

The concept is a strong fit for the approved brand system:

- Full desktop landing page from header through footer.
- First viewport shows `ferric-rules`, a concise embedding-focused value proposition, primary actions, architectural drafting, steel girder imagery, and abstract Rete network geometry.
- Responsive nav callouts show collapsed and expanded tablet/mobile states.
- Sections are identifiable: hero, value pillars, code comparison, documentation cards, status/known exclusions, footer.
- The concept uses approved component families: nav, buttons, badges, feature cards, doc cards, code panel, status/table-like content, and footer.

Implementation notes if approved:

- The concept footer includes newsletter/social/icon details that are richer than the current compact footer component; this can be implemented in page-specific layout CSS or with a small footer expansion before coding.
- Placeholder copy such as dates, version strings, crate command, and footer copyright should be normalized to current project facts or omitted.
- The concept's logo mark is close to the approved direction but should use the project-local draft SVG `.maquette/logo/ferric-rules-mark-v1.svg` instead of inventing a new mark.

Approval decision: approved by user. Use this concept for page blueprint and implementation.

## Implementation

- Page HTML: `.maquette/pages/landing/page.html`
- Page CSS: `.maquette/pages/landing/page.css`
- Page JS: `.maquette/pages/landing/page.js`
- Blueprint: `.maquette/pages/landing/page-blueprint.json`
- Region inventory: `.maquette/pages/landing/concept-region-inventory.md`
- Layout contract: `.maquette/pages/landing/page-layout-contract.md`
- Asset manifest: `.maquette/pages/landing/asset-manifest.json`

Concept fidelity:

- Header, hero, badges, CTAs, install command, drafting/girder/Rete hero visual, feature cards, syntax/code section, documentation cards, status/known exclusions, honest-by-design callout, and rich footer are implemented.
- Static responsive-nav callouts from the concept are implemented as actual responsive navigation behavior and captured as open-nav screenshots.
- Placeholder version/date content was normalized to repository facts: version `0.1.0`, dual license `MIT OR Apache-2.0`, no generated 2024 date.

QA:

- Linked assets pass: `.maquette/pages/landing/page.linked-assets.json`
- Responsive audit pass: `.maquette/pages/landing/page.responsive-audit.json`
- Primary desktop screenshot: `.maquette/pages/landing/page.png`
- Schema validation pass for `.maquette/pages/landing/page-blueprint.json` and `.maquette/pages/landing/asset-manifest.json` against Maquette shared schemas.
- Screenshots:
  - `.maquette/pages/landing/page-responsive/responsive-390.png`
  - `.maquette/pages/landing/page-responsive/responsive-768.png`
  - `.maquette/pages/landing/page-responsive/responsive-1024.png`
  - `.maquette/pages/landing/page-responsive/responsive-1280.png`
  - `.maquette/pages/landing/page-responsive/responsive-1440.png`
- Open nav screenshots:
  - `.maquette/pages/landing/page-responsive/responsive-nav-open-390.png`
  - `.maquette/pages/landing/page-responsive/responsive-nav-open-768.png`
  - `.maquette/pages/landing/page-responsive/responsive-nav-open-1024.png`

Responsive findings:

- Document-level overflow is 0px at 390, 768, 1024, 1280, and 1440px.
- Mobile/tablet nav toggles `aria-expanded` and drawer scrollability passes at 390, 768, and 1024px.
- Accepted internal scroll: one Rust code block scrolls internally by about 15px at 390px only. This matches the layout contract for long code lines and does not create page-wide overflow.

Review summary:

- Page compactness: matches.
- Footer fidelity: matches; the rich footer includes brand summary, link columns, newsletter, social icons, and legal/version meta.
- Card alignment: matches; feature cards and doc cards use shared anatomy and stable terminal rows.
- Media fit/crop: matches; hero media is code-generated SVG/CSS and fills its container without blank raster bands.
- Font rationale: approved Inter/system sans and IBM Plex Mono/SFMono stacks are used; no novelty industrial display fonts.
