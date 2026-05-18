# Component Sheet Inventory

## Batch: core-actions

Status: implemented and reviewed.

Poster:

- `.maquette/components/component-sheet-core-actions-css-contract-v1.png`

Selector allowlist:

- `.btn`
- `.btn--primary`
- `.btn--secondary`
- `.btn--ghost`
- `.btn--danger`
- `.btn--sm`
- `.btn--md`
- `.btn--lg`
- `.btn__icon`
- `.icon-btn`
- `.icon-btn.is-selected`
- `.badge`
- `.badge--neutral`
- `.badge--accent`
- `.badge--success`
- `.badge--warning`
- `.badge--danger`

Required coverage:

- Action buttons with primary, secondary, ghost, and danger variants.
- Small, medium, and large button sizing.
- Icon slot and icon-only control behavior.
- Badge primitives for neutral, accent, success, warning, and danger states.
- Hover, focus-visible, active, selected, disabled, and loading states where relevant.

Decision:

- Implemented. The poster was readable and sufficiently scoped, but its generated token names were normalized to approved `tokens.css` variables during transcription.

Evidence:

- Contract: `.maquette/components/contracts/core-actions.contract.css`
- Replica: `.maquette/components/core-actions.replica.html`
- CSS: `.maquette/components/css/core-actions.components.css`
- JS: `.maquette/components/js/core-actions.components.js`
- Catalog snapshot: `.maquette/components/core-actions.component-catalog.json`
- Review: `.maquette/components/core-actions.review.md`

## Batch: navigation-layout

Status: implemented and reviewed.

Poster:

- `.maquette/components/component-sheet-navigation-layout-css-contract-v1.png`

Selector allowlist:

- `.site-header`
- `.site-nav`
- `.site-nav__brand`
- `.site-nav__mark`
- `.site-nav__links`
- `.site-nav__link`
- `.site-nav__link[aria-current="page"]`
- `.site-nav__actions`
- `.site-nav__toggle`
- `.site-nav__toggle[aria-expanded="true"]`
- `.site-nav__panel`
- `.site-nav__panel[data-open="true"]`
- `.site-nav__panel-link`
- `.site-nav__drawer-scrim`
- `.skip-link`

Required coverage:

- Desktop inline primary navigation.
- Tablet/mobile collapsed navigation with menu toggle.
- Expanded stacked panel or drawer behavior.
- Active/current link state, focus-visible state, and tap-target sizing.
- No document-level horizontal scrolling.

Decision:

- Implemented. The poster was readable and scoped to navigation selectors. The large title is acceptable for a component CSS-contract poster and was not used as brand-board evidence.

Evidence:

- Contract: `.maquette/components/contracts/navigation-layout.contract.css`
- Replica: `.maquette/components/navigation-layout.replica.html`
- CSS: `.maquette/components/css/navigation-layout.components.css`
- JS: `.maquette/components/js/navigation-layout.components.js`
- Catalog snapshot: `.maquette/components/navigation-layout.component-catalog.json`
- Review: `.maquette/components/navigation-layout.review.md`

## Batch: landing-composites

Status: implemented and reviewed.

Poster:

- `.maquette/components/component-sheet-landing-composites-css-contract-v1.png`

Selector allowlist:

- `.feature-card`
- `.feature-card__eyebrow`
- `.feature-card__title`
- `.feature-card__body`
- `.doc-card`
- `.doc-card__title`
- `.doc-card__body`
- `.doc-card__meta`
- `.code-panel`
- `.code-panel__header`
- `.code-panel__dot`
- `.code-panel__body`
- `.code-panel code`
- `.site-footer`
- `.site-footer__brand`
- `.site-footer__links`
- `.site-footer__link`
- `.site-footer__meta`

Required coverage:

- Equal-height feature/documentation cards with stable title/body/meta anatomy.
- Code panel for Rust and CLIPS examples.
- Compact footer module with brand, links, and license/meta row.
- Responsive wrapping and no document-level horizontal overflow.

Decision:

- Implemented. The poster was readable and scoped to landing composites. The generated responsive note contained a typo, which was normalized during transcription.

Evidence:

- Contract: `.maquette/components/contracts/landing-composites.contract.css`
- Replica: `.maquette/components/landing-composites.replica.html`
- CSS: `.maquette/components/css/landing-composites.components.css`
- JS: `.maquette/components/js/landing-composites.components.js`
- Catalog snapshot: `.maquette/components/landing-composites.component-catalog.json`
- Review: `.maquette/components/landing-composites.review.md`

## Batch: footer-social

Status: implemented and reviewed.

Poster:

- `.maquette/components/component-sheet-footer-social-css-contract-v1.png`

Selector allowlist:

- `.footer-rich`
- `.footer-rich__brand`
- `.footer-rich__summary`
- `.footer-rich__columns`
- `.footer-rich__column`
- `.footer-rich__heading`
- `.footer-rich__link`
- `.footer-rich__newsletter`
- `.footer-rich__input`
- `.footer-rich__social`
- `.footer-rich__social-link`
- `.footer-rich__bottom`
- `.footer-rich__meta`

Required coverage:

- Rich footer shown in the approved landing concept.
- Brand/summary block, link columns, newsletter input/action row, social icon controls, legal/meta bottom row.
- Responsive wrapping without document-level horizontal overflow.

Decision:

- Implemented. The poster was readable and focused on the rich footer/social selector family. The coded replica preserves the approved landing concept's richer footer anatomy while using approved tokens and reusable action/navigation primitives.

Evidence:

- Contract: `.maquette/components/contracts/footer-social.contract.css`
- Replica: `.maquette/components/footer-social.replica.html`
- CSS: `.maquette/components/css/footer-social.components.css`
- JS: `.maquette/components/js/footer-social.components.js`
- Catalog snapshot: `.maquette/components/footer-social.component-catalog.json`
- Review: `.maquette/components/footer-social.review.md`
