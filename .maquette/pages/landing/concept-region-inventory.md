# Landing Concept Region Inventory

Status: ready for implementation.

Concept source: `.maquette/pages/landing/concept.png`

## Regions

| Region | Visible In Concept | Implementation Status | Notes |
| --- | --- | --- | --- |
| Sticky inverse header | Yes | implemented | Use responsive navigation component with approved vector mark substituted for the concept placeholder mark. |
| Desktop nav links and actions | Yes | implemented | Links: Docs, Compatibility, Embedding, Benchmarks, GitHub. Actions: Star on GitHub, Read the docs. |
| Tablet/mobile collapsed nav callout | Yes | implemented differently with reason | The rendered page uses the actual responsive nav component rather than a static side callout; QA will capture closed and open nav screenshots. |
| Tablet/mobile expanded nav callout | Yes | implemented differently with reason | Actual drawer/panel behavior is implemented and measured instead of reproducing explanatory callout art inside the page. |
| Hero title and value proposition | Yes | implemented | H1 is `ferric-rules`; supporting copy emphasizes CLIPS-style rules and modern Rust embedding. |
| Hero badges | Yes | implemented | Use existing badge component for Rust crate, CLIPS syntax, and independent engines. |
| Hero CTAs | Yes | implemented | Use approved primary and ghost/secondary button styles. |
| Install command strip | Yes | implemented | Normalize command to `cargo add ferric` because the public facade crate is `ferric`; include copy affordance. |
| Drafting/girder/Rete hero visual | Yes | implemented differently with reason | Implement as page-local CSS/SVG vector composition so no additional raster asset is required; preserve girder, protractor arcs, grid, and abstract Rete network. |
| First viewport marker | Yes | intentionally omitted with reason | Concept marker is a design annotation, not user-facing product content. |
| Why ferric-rules section | Yes | implemented | Three equal-height feature cards for compatibility, embedding, and rigor. |
| Syntax/Rust performance section | Yes | implemented | Two-column explanatory copy plus code panel using component library. |
| Code comparison tabs | Yes | implemented differently with reason | Static CLIPS/Rust split is implemented inside one code panel; interactive tabs are not required for the ideation landing page. |
| Documentation card grid | Yes | implemented | Six documentation cards with stable meta/action rows. |
| Status and known exclusions | Yes | implemented | Implement as a compact status grid with explicit prototype, core coverage, validation, and exclusions rows. |
| Honest by design callout | Yes | implemented | Side card beside status grid. |
| Rich footer | Yes | implemented | Use newly approved `rich-footer` component coverage. |
| Footer social links | Yes | implemented | Recognizable inline SVG icons with accessible names. |
| Footer newsletter | Yes | implemented | Static newsletter form with visible input and accessible label. |
| Footer legal/version/date row | Yes | implemented differently with reason | Normalize to repo facts: version `0.1.0`, dual license, current project status. Do not preserve placeholder 2024 date from generated concept. |

## Component Coverage

- Responsive navigation: covered by `responsive-navigation`.
- Buttons, badges, and icon controls: covered by `button` and core actions.
- Feature cards, documentation cards, and code panel: covered by `feature-card`, `doc-card`, and `code-panel`.
- Rich footer, newsletter, and social links: covered by `rich-footer`.
- Page-specific composites: hero drafting visual, status grid, and section labels are page composition, not reusable component gaps.
