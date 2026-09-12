import { readFileSync } from "node:fs";
import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

const path = "/ferric-rules/docs/compatibility-probes/";
type Probe = {
  id: string;
  path: string;
  source: string;
  clips_output: string;
  ferric_output: string;
  ferric_phase: string;
  diagnostics: string[];
  input: string | null;
  matches: boolean;
  level: string;
  disposition: { kind: string; explanation: string; issues: string[] };
};

function evidence(): Probe[] {
  const report = JSON.parse(
    readFileSync("src/generated/compatibility.json", "utf8"),
  ) as { groups: { cases: Probe[] }[] };
  return report.groups.flatMap((group) => group.cases);
}

test("renders every executed probe and preserves its source and outputs", async ({
  page,
}) => {
  const probes = evidence();
  const manifest = JSON.parse(
    readFileSync("../tests/clips_compat/corpus/manifest.json", "utf8"),
  ) as { cases: { path: string }[] };
  expect(probes.map((probe) => probe.path).sort()).toEqual(
    manifest.cases.map((probe) => probe.path).sort(),
  );
  await page.goto(path);
  await expect(page.locator("[data-probe]")).toHaveCount(probes.length);
  const rendered = await page.locator("[data-probe]").evaluateAll((rows) =>
    rows.map((row) => ({
      id: row.id,
      source: row.querySelector("[data-source]")?.textContent,
      clips_output: row.querySelector('[data-output="clips"]')?.textContent,
      ferric_output: row.querySelector('[data-output="ferric"]')?.textContent,
      ferric_phase: row.querySelector("[data-ferric-phase]")?.textContent,
      diagnostics: Array.from(row.querySelectorAll("[data-diagnostic]")).map(
        (node) => node.textContent,
      ),
      input: row.querySelector("[data-input]")?.textContent ?? null,
    })),
  );
  expect(rendered).toEqual(
    probes.map(
      ({
        id,
        source,
        clips_output,
        ferric_output,
        ferric_phase,
        diagnostics,
        input,
      }) => ({
        id,
        source,
        clips_output,
        ferric_output,
        ferric_phase,
        diagnostics,
        input,
      }),
    ),
  );
  await expect(page.locator("[data-probe-total]")).toHaveText(
    String(probes.length),
  );
  await expect(page.locator("[data-probe-matching]")).toHaveText(
    String(probes.filter((probe) => probe.matches).length),
  );
  await expect(page.locator("[data-probe-different]")).toHaveText(
    String(probes.filter((probe) => !probe.matches).length),
  );
  const cardTops = await page
    .locator(".probe-stats > div")
    .evaluateAll((cards) =>
      cards.map((card) => card.getBoundingClientRect().top),
    );
  expect(Math.max(...cardTops) - Math.min(...cardTops)).toBeLessThan(1);
});

test("combines filters, handles empty results, and clears them", async ({
  page,
}) => {
  const probes = evidence();
  await page.goto(path);
  await page.locator("[data-probe-result]").selectOption("different");
  await page
    .locator("[data-probe-disposition]")
    .selectOption("deliberate-boundary");
  await page.locator("[data-probe-level]").selectOption("boundary");
  const expected = probes.filter(
    (probe) =>
      !probe.matches &&
      probe.disposition.kind === "deliberate-boundary" &&
      probe.level === "boundary",
  );
  expect(expected.length).toBeGreaterThan(0);
  await expect(page.locator("[data-probe]:not([hidden])")).toHaveCount(
    expected.length,
  );
  await page.locator("[data-probe-search]").fill("no-probe-has-this-phrase");
  await expect(page.locator("[data-probe-empty]")).toBeVisible();
  await expect(page.locator("[data-probe-group]:not([hidden])")).toHaveCount(0);
  await page.locator("[data-probe-clear]").click();
  await expect(page.locator("[data-probe]:not([hidden])")).toHaveCount(
    probes.length,
  );
  await expect(page.locator("[data-probe-count]")).toHaveText(
    `Showing all ${probes.length} probes.`,
  );
  const probe = probes.find(
    (probe) => probe.disposition.kind === "tracked-defect",
  )!;
  await page.locator("[data-probe-search]").fill(probe.path);
  await expect(page.locator("[data-probe]:not([hidden])")).toHaveCount(1);
  await expect(page.locator(`[id="${probe.id}"]`)).toBeVisible();
});

test("opens a linked mismatch with its explanation and fits the viewport", async ({
  page,
}) => {
  const probe = evidence().find(
    (probe) => probe.diagnostics.length > 0 && probe.ferric_output === "",
  )!;
  await page.goto(`${path}#${probe.id}`);
  const row = page.locator(`[id="${probe.id}"]`);
  await expect(row).toHaveAttribute("open", "");
  await expect(row.locator(".probe-disposition")).toContainText(
    probe.disposition.explanation,
  );
  await expect(row.locator('[data-empty-output="ferric"]')).toBeVisible();
  for (const issue of probe.disposition.issues) {
    await expect(row.locator(`a[href="${issue}"]`)).toBeVisible();
  }
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > window.innerWidth + 1,
    ),
  ).toBe(false);
  if ((page.viewportSize()?.width ?? 0) > 640) {
    const outputTops = await row
      .locator(".probe-output > h3")
      .evaluateAll((headings) =>
        headings.map((heading) => heading.getBoundingClientRect().top),
      );
    expect(Math.max(...outputTops) - Math.min(...outputTops)).toBeLessThan(1);
  }

  // Hash navigation must also reveal a probe hidden by the active filter.
  await page.locator("[data-probe-result]").selectOption("match");
  await expect(row).toBeHidden();
  await page.evaluate((id) => {
    window.location.hash = "group-facts";
    window.location.hash = id;
  }, probe.id);
  await expect(row).toBeVisible();
  await expect(row).toHaveAttribute("open", "");

  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});

test("keeps all examples and disclosure controls available without JavaScript", async ({
  browser,
}) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  try {
    const origin = `http://127.0.0.1:${process.env.SITE_TEST_PORT ?? "4321"}`;
    await page.goto(`${origin}${path}`);
    await expect(page.locator("[data-probe]")).toHaveCount(evidence().length);
    await expect(page.locator("[data-probe-filters]")).toBeHidden();
    const first = page.locator("[data-probe]").first();
    await first.locator("summary").click();
    await expect(first.locator("[data-source]")).toBeVisible();
  } finally {
    await context.close();
  }
});
