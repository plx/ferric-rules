# core-actions Review

## Source Artifact

- CSS-contract poster: `.maquette/components/component-sheet-core-actions-css-contract-v1.png`
- Contract transcription: `.maquette/components/contracts/core-actions.contract.css`

## Implementation

- Replica/reference: `.maquette/components/core-actions.replica.html`
- CSS: `.maquette/components/css/core-actions.components.css`
- JS: `.maquette/components/js/core-actions.components.js`
- Catalog snapshot: `.maquette/components/core-actions.component-catalog.json`

## Review Mode

Screenshot and automated responsive audit using QA tooling installed under `documentation/`.

## Evidence

- Linked assets: `.maquette/components/core-actions.linked-assets.json`
- Responsive audit: `.maquette/components/core-actions.responsive-audit.json`
- Screenshots:
  - `.maquette/components/core-actions-responsive/responsive-390.png`
  - `.maquette/components/core-actions-responsive/responsive-768.png`
  - `.maquette/components/core-actions-responsive/responsive-1024.png`
  - `.maquette/components/core-actions-responsive/responsive-1280.png`
  - `.maquette/components/core-actions-responsive/responsive-1440.png`

## Responsive Results

No document-level horizontal overflow at 390, 768, 1024, 1280, or 1440px. The mobile layout stacks cards and wraps action rows cleanly. No text overlap or unreadable control labels observed in the inspected 390px and 1440px screenshots.

## Rubric

- Coverage: 5/5
- Visual match: 4/5
- Anatomy match: 5/5
- Responsive match: 5/5
- Implementation quality: 5/5

## Corrections Made

- Normalized generated poster token names to the approved `tokens.css` names.
- Added responsive wrapping for button and badge groups.
- Added inverse-surface ghost-button behavior so dark surfaces retain contrast.
- Added accessible names for icon-only controls.

## Simplifications

The poster is a CSS contract rather than a visual sheet. The coded reference is evaluated against the contract and approved brand system, not the poster's black-background layout.

## Deferred

- Forms, alerts, and tabs.
- Responsive navigation.
- Documentation/code display components.
- Landing-page cards, footer, and larger composites.

## Status

Implemented and reviewed. Concrete batch artifacts were created before any next component poster was generated.
