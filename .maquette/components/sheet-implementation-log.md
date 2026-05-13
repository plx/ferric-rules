# Component Sheet Implementation Log

## core-actions

- Status: implemented and reviewed.
- Poster path: `.maquette/components/component-sheet-core-actions-css-contract-v1.png`
- Contract path: `.maquette/components/contracts/core-actions.contract.css`
- Replica path: `.maquette/components/core-actions.replica.html`
- Batch CSS path: `.maquette/components/css/core-actions.components.css`
- Batch JS path: `.maquette/components/js/core-actions.components.js`
- Catalog snapshot path: `.maquette/components/core-actions.component-catalog.json`
- Review path: `.maquette/components/core-actions.review.md`
- Review artifacts:
  - `.maquette/components/core-actions.linked-assets.json`
  - `.maquette/components/core-actions.responsive-audit.json`
  - `.maquette/components/core-actions.validate-artifacts.json`
  - `.maquette/components/core-actions-responsive/responsive-390.png`
  - `.maquette/components/core-actions-responsive/responsive-768.png`
  - `.maquette/components/core-actions-responsive/responsive-1024.png`
  - `.maquette/components/core-actions-responsive/responsive-1280.png`
  - `.maquette/components/core-actions-responsive/responsive-1440.png`
- Rubric: coverage 5, visual match 4, anatomy 5, responsive 5, implementation quality 5.
- Corrections: normalized token names, added responsive wrapping, added inverse-surface ghost-button contrast.
- Simplifications: coded reference follows the contract and approved brand system rather than reproducing the black CSS poster layout.
- Deferred: navigation, cards/composites, code display, footer modules.
- Completed before next sheet: true.

## navigation-layout

- Status: implemented and reviewed.
- Poster path: `.maquette/components/component-sheet-navigation-layout-css-contract-v1.png`
- Contract path: `.maquette/components/contracts/navigation-layout.contract.css`
- Replica path: `.maquette/components/navigation-layout.replica.html`
- Batch CSS path: `.maquette/components/css/navigation-layout.components.css`
- Batch JS path: `.maquette/components/js/navigation-layout.components.js`
- Catalog snapshot path: `.maquette/components/navigation-layout.component-catalog.json`
- Review path: `.maquette/components/navigation-layout.review.md`
- Review artifacts:
  - `.maquette/components/navigation-layout.linked-assets.json`
  - `.maquette/components/navigation-layout.responsive-audit.json`
  - `.maquette/components/navigation-layout-responsive/responsive-390.png`
  - `.maquette/components/navigation-layout-responsive/responsive-768.png`
  - `.maquette/components/navigation-layout-responsive/responsive-1024.png`
  - `.maquette/components/navigation-layout-responsive/responsive-1280.png`
  - `.maquette/components/navigation-layout-responsive/responsive-1440.png`
  - `.maquette/components/navigation-layout-responsive/responsive-nav-open-390.png`
  - `.maquette/components/navigation-layout-responsive/responsive-nav-open-768.png`
  - `.maquette/components/navigation-layout-responsive/responsive-nav-open-1024.png`
- Rubric: coverage 5, visual match 4, anatomy 5, responsive 5, implementation quality 5.
- Corrections: normalized token names, added responsive drawer/scrim behavior, verified aria-expanded state changes.
- Simplifications: nav mark is placeholder pending final vector logo.
- Completed before next sheet: true.

## landing-composites

- Status: implemented and reviewed.
- Poster path: `.maquette/components/component-sheet-landing-composites-css-contract-v1.png`
- Contract path: `.maquette/components/contracts/landing-composites.contract.css`
- Replica path: `.maquette/components/landing-composites.replica.html`
- Batch CSS path: `.maquette/components/css/landing-composites.components.css`
- Batch JS path: `.maquette/components/js/landing-composites.components.js`
- Catalog snapshot path: `.maquette/components/landing-composites.component-catalog.json`
- Review path: `.maquette/components/landing-composites.review.md`
- Review artifacts:
  - `.maquette/components/landing-composites.linked-assets.json`
  - `.maquette/components/landing-composites.responsive-audit.json`
  - `.maquette/components/landing-composites-responsive/responsive-390.png`
  - `.maquette/components/landing-composites-responsive/responsive-768.png`
  - `.maquette/components/landing-composites-responsive/responsive-1024.png`
  - `.maquette/components/landing-composites-responsive/responsive-1280.png`
  - `.maquette/components/landing-composites-responsive/responsive-1440.png`
- Rubric: coverage 5, visual match 4, anatomy 5, responsive 5, implementation quality 4.
- Corrections: removed browser-default doc-card link underlines; added doc-card focus state; documented code-panel internal scroll.
- Simplifications: isolated composite reference omits navigation because it was covered in the previous batch.
- Completed before next sheet: true.

## final-gallery

- Status: approved after footer-social merge.
- Gallery path: `.maquette/components/replica-gallery.html`
- CSS path: `.maquette/components/css/components.css`
- JS path: `.maquette/components/js/components.js`
- Catalog path: `.maquette/components/component-catalog.json`
- Review artifacts:
  - `.maquette/components/replica-gallery.linked-assets.json`
  - `.maquette/components/replica-gallery.responsive-audit.json`
  - `.maquette/components/replica-gallery.check.json`
  - `.maquette/components/page-consumption-smoke.json`
  - `.maquette/components/validate-artifacts.json`
- Responsive: pass at 390, 768, 1024, 1280, and 1440px with accepted internal code scroll at 390px.
- Component smoke: pass.
- Page consumption smoke: pass.
- Schema validation: pass.
- Rich footer/social module is present in the merged gallery and ready for page use.

## footer-social

- Status: implemented and reviewed.
- Poster path: `.maquette/components/component-sheet-footer-social-css-contract-v1.png`
- Contract path: `.maquette/components/contracts/footer-social.contract.css`
- Replica path: `.maquette/components/footer-social.replica.html`
- Batch CSS path: `.maquette/components/css/footer-social.components.css`
- Batch JS path: `.maquette/components/js/footer-social.components.js`
- Catalog snapshot path: `.maquette/components/footer-social.component-catalog.json`
- Review path: `.maquette/components/footer-social.review.md`
- Review artifacts:
  - `.maquette/components/footer-social.linked-assets.json`
  - `.maquette/components/footer-social.responsive-audit.json`
  - `.maquette/components/footer-social-responsive/responsive-390.png`
  - `.maquette/components/footer-social-responsive/responsive-768.png`
  - `.maquette/components/footer-social-responsive/responsive-1024.png`
  - `.maquette/components/footer-social-responsive/responsive-1280.png`
  - `.maquette/components/footer-social-responsive/responsive-1440.png`
- Rubric: coverage 5, visual match 4, anatomy 5, responsive 5, implementation quality 5.
- Corrections: added `.sr-only` helper for the newsletter label; preserved 44px hit targets for links and social controls.
- Simplifications: no app/download or device module because the approved landing concept does not include one.
- Completed before page coding: true.
