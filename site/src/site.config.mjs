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
    "tagline": "CLIPS-style rules in Rust.",
    "description": "A mostly CLIPS-compatible forward-chaining rules engine for embedding in applications. Each engine instance owns its state.",
    "installCommand": "cargo add --git https://github.com/plx/ferric-rules ferric-rules"
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
        "label": "Documentation",
        "href": "docs/"
      },
      {
        "label": "Compatibility",
        "href": "docs/compatibility/"
      },
      {
        "label": "Example",
        "href": "#syntax"
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
      "label": "Getting started",
      "href": "docs/getting-started/"
    },
    "secondaryCta": {
      "label": "Read the docs",
      "href": "docs/"
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
            "label": "CLIPS compatibility",
            "slug": "docs/compatibility"
          },
          {
            "label": "Embedding API",
            "slug": "docs/embedding"
          },
          {
            "label": "Performance",
            "slug": "docs/performance"
          },
          {
            "label": "Internals",
            "slug": "docs/internals"
          }
        ]
      }
    ],
    "pages": [
      {
        "title": "Overview",
        "description": "How the engine works and where to start.",
        "slug": "docs",
        "href": "docs/"
      },
      {
        "title": "Getting started",
        "description": "Run a small Rust program with one rule and one fact.",
        "slug": "docs/getting-started",
        "href": "docs/getting-started/"
      },
      {
        "title": "CLIPS compatibility",
        "description": "Implemented features, exclusions, and known differences from CLIPS.",
        "slug": "docs/compatibility",
        "href": "docs/compatibility/"
      },
      {
        "title": "Embedding API",
        "description": "Engine lifecycle, thread ownership, output, and language bindings.",
        "slug": "docs/embedding",
        "href": "docs/embedding/"
      },
      {
        "title": "Performance",
        "description": "Run the benchmarks and scaling checks.",
        "slug": "docs/performance",
        "href": "docs/performance/"
      },
      {
        "title": "Internals",
        "description": "The main crates and the path from source to rule execution.",
        "slug": "docs/internals",
        "href": "docs/internals/"
      }
    ]
  }
};
