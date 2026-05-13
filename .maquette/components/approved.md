# Component Approval

Status: approved

The component library phase is underway. Optional QA tooling is installed under `documentation/` by user preference rather than at the repository root. Component artifacts remain under repo-level `.maquette/components/`.

## QA Tooling

- Dependency root: `documentation/`
- Artifact root: repo-level `.maquette/`
- Local symlink for coupled Maquette scripts: `documentation/.maquette -> ../.maquette`
- Installed packages: `playwright`, `ajv`, `ajv-formats`
- Browser: Chromium launch verified through Maquette `ensure-qa-tooling.mjs`

## Current Batch

`core-actions` CSS-contract poster has been generated, inspected, transcribed, implemented, and reviewed.

- Poster: `.maquette/components/component-sheet-core-actions-css-contract-v1.png`
- Contract: `.maquette/components/contracts/core-actions.contract.css`
- Replica: `.maquette/components/core-actions.replica.html`
- Review: `.maquette/components/core-actions.review.md`
- Result: linked assets pass; responsive overflow pass at 390, 768, 1024, 1280, and 1440px.
- Schema: Maquette `design-system.json` and current `component-catalog.json` validate through `documentation/` QA tooling.

Navigation batch:

- Poster: `.maquette/components/component-sheet-navigation-layout-css-contract-v1.png`
- Contract: `.maquette/components/contracts/navigation-layout.contract.css`
- Replica: `.maquette/components/navigation-layout.replica.html`
- Review: `.maquette/components/navigation-layout.review.md`
- Result: linked assets pass; responsive overflow pass at 390, 768, 1024, 1280, and 1440px; open nav screenshots captured at 390, 768, and 1024px.

Landing composites batch:

- Poster: `.maquette/components/component-sheet-landing-composites-css-contract-v1.png`
- Contract: `.maquette/components/contracts/landing-composites.contract.css`
- Replica: `.maquette/components/landing-composites.replica.html`
- Review: `.maquette/components/landing-composites.review.md`
- Result: linked assets pass; document-level overflow is 0px at tested widths. Code panel internal horizontal scroll at 390px is accepted for long source lines. Generic nav failures were allowed only for the isolated composite batch because navigation has its own passing batch.

Merged component library:

- Gallery: `.maquette/components/replica-gallery.html`
- CSS: `.maquette/components/css/components.css`
- JS: `.maquette/components/js/components.js`
- Catalog: `.maquette/components/component-catalog.json`
- Result: linked assets pass; component gallery check passes; page-consumption smoke passes; Maquette schema validation passes.
- Responsive QA: document-level overflow is 0px at 390, 768, 1024, 1280, and 1440px. Mobile/tablet nav opens and changes `aria-expanded` at 390, 768, and 1024px. Code panel uses intentional internal scroll at 390px only.
- Accessibility correction: selected icon-button contrast was raised from 4.05 to 5.32 by using the darker ferric action-hover token; footer links were given 44px minimum hit areas so compact nav checks pass; inverse-header ghost actions now use inverse text/border colors.

Footer/social batch:

- Poster: `.maquette/components/component-sheet-footer-social-css-contract-v1.png`
- Contract: `.maquette/components/contracts/footer-social.contract.css`
- Replica: `.maquette/components/footer-social.replica.html`
- Review: `.maquette/components/footer-social.review.md`
- Result: linked assets pass; responsive overflow pass at 390, 768, 1024, 1280, and 1440px.
- Coverage: rich footer with brand summary, link columns, newsletter form, recognizable social icons, and legal/meta bottom row.

Merged component library after footer/social:

- Gallery: `.maquette/components/replica-gallery.html`
- CSS: `.maquette/components/css/components.css`
- JS: `.maquette/components/js/components.js`
- Catalog: `.maquette/components/component-catalog.json`
- Result: linked assets pass; component gallery check passes; page-consumption smoke passes; Maquette schema validation passes.
- Responsive QA: document-level overflow is 0px at 390, 768, 1024, 1280, and 1440px. Mobile/tablet nav opens and changes `aria-expanded` at 390, 768, and 1024px. Code panel uses intentional internal scroll at 390px only.

Remaining component coverage before page work:

- None for the approved landing-page concept.
