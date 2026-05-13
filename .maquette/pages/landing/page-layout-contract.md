# Landing Page Layout Contract

Status: ready for implementation.

## Global Rules

- Use `.maquette/brand/tokens.css` and `.maquette/components/css/components.css` before page CSS.
- Keep the page quiet and documentation-forward: steel graphite neutrals, ferric accents, fine drafting grid texture, and low-chroma surfaces.
- Do not introduce negative letter spacing or viewport-scaled font sizes.
- Maximum content width is the approved `--layout-content-width-max` with 24px desktop gutters and 16px mobile gutters.
- Main sections use compact vertical rhythm: 48-64px desktop padding, 40-48px tablet padding, 32-40px mobile padding.
- Cards are 8px radius or less and are not nested inside decorative cards.
- Use the project-local SVG logo `.maquette/logo/ferric-rules-mark-v1.svg`.

## Section Order

1. Sticky inverse header and responsive navigation.
2. Hero with value proposition, badges, CTAs, install command, and drafting/girder/Rete visual.
3. Why ferric-rules feature card grid.
4. Syntax and embedding code comparison section.
5. Documentation card grid.
6. Status and known exclusions with honest-by-design callout.
7. Rich footer with brand block, link columns, newsletter, social icons, legal/license/version meta.

## Header And Navigation

- Desktop: inverse 64px sticky header, brand left, inline links center/right, two actions right.
- Tablet/mobile: hide inline links/actions at 1024px and below; show 44px toggle and stacked drawer/panel.
- Open drawer must be scrollable, keyboard reachable, and must not cause document-level horizontal overflow.
- Header uses actual nav behavior; static responsive nav callouts from the concept are represented through captured QA screenshots, not page content.

## Hero

- Desktop: two-column 12-column layout. Left copy takes roughly 5 columns; visual takes 7 columns.
- Mobile/tablet: stack copy before visual.
- First viewport should show a hint of the next section on normal desktop heights.
- Hero visual aspect ratio: about 16:10 desktop, 4:3 mobile. It must fill its container with vector content and no blank media bands.
- Visual content must include a light drafting grid, protractor arcs, steel-girder truss lines, and abstract Rete nodes/links with ferric join nodes.
- The install command strip is compact and code-like; long text may wrap on very narrow screens but must not force page overflow.

## Feature Cards

- Three equal-height cards on desktop, one column on mobile.
- Shared anatomy: icon/top mark, title, body, checklist, link/action row.
- Action rows align visually by using flex column layout and `margin-top: auto` for terminal rows.

## Syntax And Code

- Desktop: explanatory rail plus large inverse code panel.
- Mobile: stack content, keep the code panel internally scrollable for long source lines only.
- Code panel may use intentional internal horizontal scrolling at 390px; document-level overflow remains a failure.

## Documentation Grid

- Six cards in a 3x2 desktop grid, 2 columns on tablet, 1 column on mobile.
- Cards preserve title/body/meta/action anatomy; meta/action row is bottom-pinned.
- Icons may be simple inline SVGs matching the approved ferric line style.

## Status Region

- Desktop: status grid/table plus side callout. Mobile: single-column stacked rows.
- Known exclusions must be explicit and copy should reflect repository positioning: COOL is intentionally out of scope; prototype is early but functional.
- Keep this region compact and factual, not a large marketing CTA.

## Footer

- Use the approved rich-footer module.
- Desktop footer has brand summary/social column, three link columns, newsletter column, and bottom meta strip.
- Mobile stacks all columns in the same order; all text and social links keep 44px hit targets.
- Footer must not be simplified into a generic link list. Newsletter and social icons are required because they are visible in the concept.

## Assets And Media

- Concept image is a reference only and is not embedded in the page.
- Logo SVG is embedded through an `<img>` element.
- Hero drafting/girder/Rete media is code-generated SVG/CSS; no generated raster asset is required.
- Page screenshots captured during QA become page screenshot assets.
