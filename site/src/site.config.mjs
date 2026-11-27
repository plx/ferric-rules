// Site content and navigation.
// prettier-ignore
export const siteConfig = {
  "repository": {
    "owner": "plx",
    "name": "ferric-rules",
    "url": "https://github.com/plx/ferric-rules",
    "defaultBranch": "main"
  },
  "project": {
    "name": "ferric-rules",
    "title": "ferric-rules",
    "packageName": "ferric-rules-site",
    "category": "Rust rules engine",
    "tagline": "An experimental, mostly CLIPS-compatible rules engine in Rust.",
    "description": "An experimental forward-chaining Rete rules engine in Rust, modeled on CLIPS 6.30 and built for isolated per-engine state, C FFI embedding, and predictable matching."
  },
  "site": {
    "host": "https://plx.github.io",
    "basePath": "/ferric-rules",
    "url": "https://plx.github.io/ferric-rules/",
    "dir": "site",
    "language": "en"
  },
  "theme": {
    "accent": "#D1531A",
    "accent_2": "#2563EB",
    "ink": "#0F1115",
    "surface": "#FAFAF8",
    "muted": "#475569",
    "code": "#1C1F24"
  },
  "landing": {
    "nav": [
      {
        "label": "Overview",
        "href": "docs/"
      },
      {
        "label": "Embedding & FFI",
        "href": "docs/embedding/"
      },
      {
        "label": "Compatibility",
        "href": "docs/compatibility/"
      },
      {
        "label": "Example",
        "href": "#example"
      }
    ],
    "footerLinks": [
      {
        "label": "GitHub",
        "href": "https://github.com/plx/ferric-rules"
      },
      {
        "label": "Issues",
        "href": "https://github.com/plx/ferric-rules/issues"
      },
      {
        "label": "MIT",
        "href": "https://github.com/plx/ferric-rules/blob/main/LICENSE-MIT"
      },
      {
        "label": "Apache-2.0",
        "href": "https://github.com/plx/ferric-rules/blob/main/LICENSE-APACHE"
      },
      {
        "label": "Third-party notices",
        "href": "https://github.com/plx/ferric-rules/blob/main/THIRD_PARTY.md"
      }
    ],
    "primaryCta": {
      "label": "Read the documentation",
      "href": "docs/"
    },
    "secondaryCta": {
      "label": "View on GitHub",
      "href": "https://github.com/plx/ferric-rules"
    }
  },
  "docs": {
    "sidebar": [
      {
        "label": "Documentation",
        "items": [
          {
            "label": "Overview",
            "slug": "docs"
          },
          {
            "label": "Getting started",
            "slug": "docs/getting-started"
          },
          {
            "label": "Embedding & FFI",
            "slug": "docs/embedding"
          },
          {
            "label": "CLIPS compatibility",
            "slug": "docs/compatibility"
          },
          {
            "label": "Architecture & performance",
            "slug": "docs/architecture"
          }
        ]
      }
    ],
    "pages": [
      {
        "title": "Overview",
        "description": "Why ferric-rules exists, what is in the repository today, and what to expect from an experimental prototype.",
        "slug": "docs",
        "href": "docs/"
      },
      {
        "title": "Getting started",
        "description": "Add ferric-rules from Git, walk through a small forward-chaining program, or try the interactive REPL.",
        "slug": "docs/getting-started",
        "href": "docs/getting-started/"
      },
      {
        "title": "Embedding & FFI",
        "description": "Per-engine state isolation, checked host handles, CBOR snapshots, the C ABI, and language bindings.",
        "slug": "docs/embedding",
        "href": "docs/embedding/"
      },
      {
        "title": "CLIPS compatibility",
        "description": "Supported CLIPS 6.30 features, out-of-scope constructs, intentional differences, and differential testing.",
        "slug": "docs/compatibility",
        "href": "docs/compatibility/"
      },
      {
        "title": "Architecture & performance",
        "description": "Workspace crate layout, Rete indexing and safety boundaries, Criterion benchmarks, and scaling checks.",
        "slug": "docs/architecture",
        "href": "docs/architecture/"
      }
    ]
  }
};
