import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

type DocsPage = {
  title: string;
  description: string;
  slug: string;
  href: string;
};

const origin = "http://127.0.0.1:4321";
const projectTitle = "ferric-rules";
const projectDescription =
  "A mostly CLIPS-compatible forward-chaining rules engine for embedding in applications. Each engine instance owns its state.";
const basePath: string = "/ferric-rules";
const normalizedBasePath = basePath === "/" ? "" : basePath;
// prettier-ignore
const docsPages: DocsPage[] = [
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
  ];
const pagesToCheck = ["/", ...docsPages.map((page) => page.href)];
const pagesToAudit = ["/", docsPages[0]?.href].filter(Boolean);

function sitePath(path = "/"): string {
  const cleanPath = path.startsWith("/") ? path : `/${path}`;
  return `${normalizedBasePath}${cleanPath}`;
}

function isSkippableHref(href: string): boolean {
  return (
    href === "" ||
    href.startsWith("mailto:") ||
    href.startsWith("tel:") ||
    href.startsWith("javascript:")
  );
}

test.describe("rendered site", () => {
  test("exposes core document and landmark properties", async ({ page }) => {
    await page.goto(sitePath("/"));

    expect(await page.title()).toContain(projectTitle);
    await expect(page.locator('meta[name="description"]')).toHaveAttribute(
      "content",
      projectDescription,
    );
    await expect(page.getByRole("main")).toBeVisible();
    await expect(
      page.getByRole("navigation", { name: /primary/i }),
    ).toBeVisible();
    await expect(
      page.getByRole("heading", { level: 1, name: projectTitle }),
    ).toBeVisible();
    await expect(page.locator(".skip-link")).toHaveAttribute("href", "#main");
  });

  test("links source installation, Swift, and corpus evidence", async ({
    page,
  }) => {
    const installCommand =
      "cargo add --git https://github.com/plx/ferric-rules ferric-rules";
    await page.goto(sitePath("/"));
    await expect(page.locator("[data-copy-text]")).toHaveAttribute(
      "data-copy-text",
      installCommand,
    );
    const codeExamples = page.locator(".code-panel__body");
    await expect(codeExamples.nth(0)).toContainText("(test (> ?t 75))");
    await expect(codeExamples.nth(1)).toContainText(
      "use ferric_rules::runtime::{Engine, RunLimit};",
    );
    await page.goto(sitePath("docs/getting-started/"));
    await expect(page.getByRole("main")).toContainText(installCommand);
    await page.goto(sitePath("docs/embedding/"));
    await expect(
      page.getByRole("link", { name: "Swift package and build guide" }),
    ).toHaveAttribute(
      "href",
      "https://github.com/plx/ferric-rules/blob/main/bindings/swift/README.md",
    );
    await page.goto(sitePath("docs/compatibility/"));
    await expect(
      page.getByRole("link", { name: "granular corpus" }),
    ).toHaveAttribute(
      "href",
      "https://github.com/plx/ferric-rules/blob/main/tests/clips_compat/corpus/README.md",
    );
  });

  test("keeps primary pages inside the viewport", async ({ page }) => {
    for (const pagePath of pagesToCheck) {
      await page.goto(sitePath(pagePath));
      await expect(page.getByRole("main")).toBeVisible();
      const hasHorizontalOverflow = await page.evaluate(
        () => document.documentElement.scrollWidth > window.innerWidth + 1,
      );
      expect(
        hasHorizontalOverflow,
        `${pagePath} should not overflow horizontally`,
      ).toBe(false);
    }
  });

  test("manages the mobile navigation expanded state accessibly", async ({
    page,
  }) => {
    await page.goto(sitePath("/"));

    const toggle = page.locator("[data-nav-toggle]");
    if (!(await toggle.isVisible())) {
      return;
    }

    const panel = page.locator("[data-nav-panel]");
    await expect(toggle).toHaveAttribute("aria-controls", "mobile-nav");
    await expect(toggle).toHaveAttribute("aria-expanded", "false");
    await expect(panel).toBeHidden();

    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-expanded", "true");
    await expect(panel).toBeVisible();

    await page.keyboard.press("Escape");
    await expect(toggle).toHaveAttribute("aria-expanded", "false");
    await expect(panel).toBeHidden();
  });

  test("validates rendered links and internal link targets", async ({
    page,
    request,
  }) => {
    const failures: string[] = [];

    for (const pagePath of pagesToCheck) {
      const response = await page.goto(sitePath(pagePath));
      expect(response?.status(), `${pagePath} should load`).toBeLessThan(400);

      const links = await page.locator("a[href]").evaluateAll((anchors) =>
        anchors.map((anchor) => ({
          href: anchor.getAttribute("href") ?? "",
          label: anchor.textContent?.trim() ?? "",
        })),
      );

      for (const link of links) {
        if (isSkippableHref(link.href)) {
          continue;
        }

        const resolved = new URL(link.href, `${origin}${sitePath(pagePath)}`);
        if (!["http:", "https:"].includes(resolved.protocol)) {
          failures.push(
            `${pagePath}: unsupported link protocol in ${link.href}`,
          );
          continue;
        }

        if (resolved.origin !== origin) {
          if (!link.label) {
            failures.push(
              `${pagePath}: external link ${link.href} has no text label`,
            );
          }
          continue;
        }

        if (
          normalizedBasePath &&
          resolved.pathname !== normalizedBasePath &&
          !resolved.pathname.startsWith(`${normalizedBasePath}/`)
        ) {
          failures.push(
            `${pagePath}: internal link escapes base path: ${link.href}`,
          );
          continue;
        }

        const targetPath = `${resolved.pathname}${resolved.search}`;
        const targetResponse = await request.get(targetPath);
        if (targetResponse.status() >= 400) {
          failures.push(
            `${pagePath}: ${link.href} returned ${targetResponse.status()}`,
          );
          continue;
        }

        if (resolved.hash) {
          await page.goto(`${targetPath}${resolved.hash}`);
          const targetExists = await page.evaluate((hash) => {
            const id = decodeURIComponent(hash.slice(1));
            return Boolean(
              document.getElementById(id) ||
              document.querySelector(`[name="${id}"]`),
            );
          }, resolved.hash);
          if (!targetExists) {
            failures.push(
              `${pagePath}: ${link.href} hash target does not exist`,
            );
          }
        }
      }
    }

    expect(failures).toEqual([]);
  });

  for (const pagePath of pagesToAudit) {
    test(`has no detectable accessibility violations on ${pagePath}`, async ({
      page,
    }) => {
      await page.goto(sitePath(pagePath));

      const results = await new AxeBuilder({ page }).analyze();
      expect(results.violations).toEqual([]);
    });
  }
});
