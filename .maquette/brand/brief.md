# ferric-rules Brand Brief

## Product Summary

`ferric-rules` is a mostly CLIPS-compatible forward-chaining rules engine written in Rust. It preserves the practical rule-authoring model of CLIPS while making the runtime easier to embed in modern applications: independent engine instances, no global C runtime assumptions, normal Rust crate integration, and binding-friendly architecture for environments such as mobile apps, services, and native SDKs.

The brand should signal that the project is serious infrastructure: fast, rigorous, inspectable, and built for repeated use by engineers who care about correctness.

## Audience

- Rust developers evaluating a rules engine for embedded or application-local decision logic.
- Teams migrating CLIPS rule sets into safer, easier-to-package runtime contexts.
- Engineers building SDKs, language bindings, or product-decision systems that need multiple isolated rule-engine instances.
- Documentation readers who may spend long sessions in guides, compatibility matrices, and API references.

## Tone Adjectives

- Industrial
- Precise
- Durable
- Restrained
- Technical
- Architectural
- Non-fatiguing
- Quietly confident

## Visual Direction

The requested direction combines steel, oxidized metal, bridge/skyscraper construction, and drafting-room precision. The visual system should feel like engineering documentation for serious infrastructure, not like a developer toy or a heavy industrial poster.

Useful motifs:

- Steel grey and graphite surfaces with selective ferric/rust accents.
- Fine drafting lines, protractor arcs, tick marks, section lines, and faint construction guides.
- Structural steel, girders, trusses, rivets, and open bridge geometry.
- A sense of incomplete but deliberate construction: exposed beams, alignment marks, and carefully measured joints.
- Rete/rule-network structure expressed as nodes, joins, paths, and ordered facts, but kept abstract enough to avoid looking like a generic node-graph SaaS product.

Avoid:

- One-note orange/brown rust palettes.
- Gritty post-apocalyptic decay, distressed textures, or corroded chaos.
- Oversized marketing hero treatment that would fatigue documentation readers.
- Mascots, humanoid figures, or literal Vitruvian-man imagery.
- Decorative technical diagrams that reduce readability.

## Logo Direction

Logo work is intentionally separate from the Maquette brand-board phase. The logo can later derive from the approved visual system, but the brand board itself must not contain a logo, monogram, badge, seal, app icon, or large `ferric-rules` wordmark.

Candidate logo concepts to explore after the brand board is approved:

- A compact bridge-leaf or bascule-bridge silhouette built from two angular rule-network segments.
- A girder joint forming a subtle `F` without relying on a literal monogram.
- A drafting compass/protractor arc crossing a steel truss segment, simplified into a small-scale mark.
- A Rete join-node diagram constrained inside an architectural floorplan corner or beam plate.

The logo must work as a vector mark, scale down to favicon size, and stay recognizable in single-color use.

## Landing Page Direction

The landing page should be a usable project/documentation entry point rather than a marketing splash page. It should introduce the product, show an embedding-first code example, and make documentation paths immediately available.

Expected first screen:

- Brand/product name as the first-viewport signal.
- Concise position: mostly CLIPS-compatible rules engine in modern Rust, designed for embedding.
- Primary actions for getting started and reading compatibility/migration docs.
- Hero imagery or background treatment based on precise architectural drafting and steel-girder construction, with a hint of the next section visible.

Expected page sections:

- Compatibility-focused value proposition.
- Embedding model: independent `Engine` instances and normal Rust integration.
- Small code example using real project language.
- Documentation routes: User's Guide, Compatibility, Migration, Performance, Bindings.
- Status honesty: early but functional, most core CLIPS behavior implemented, known exclusions.

## Accessibility Requirements

- Documentation text must pass WCAG AA contrast against both light and dark surfaces.
- Rust/rust-metal accents must remain accents, not body-copy colors.
- Link, focus, active, selected, warning, and error states must be distinguishable without relying only on hue.
- Fine drafting-line decoration must stay behind content and never compete with reading.
- Motion, if used later, should be subtle and avoid constant ambient animation in documentation contexts.

## Implementation Constraints

- All Maquette-owned outputs live under `.maquette/`.
- The eventual implementation target is Astro plus Starlight, but this ideation pass should produce portable HTML/CSS artifacts and design contracts first.
- Brand board approval is required before deriving tokens.
- Component and page work must use the approved brand system and Maquette component references rather than inventing a separate visual language.
