function initializeProbes(): void {
  const root = document.querySelector<HTMLElement>(
    "[data-compatibility-probes]",
  );
  if (!root || root.dataset.initialized === "true") return;

  const form = root.querySelector<HTMLFormElement>("[data-probe-filters]");
  const search = root.querySelector<HTMLInputElement>("[data-probe-search]");
  const result = root.querySelector<HTMLSelectElement>("[data-probe-result]");
  const disposition = root.querySelector<HTMLSelectElement>(
    "[data-probe-disposition]",
  );
  const level = root.querySelector<HTMLSelectElement>("[data-probe-level]");
  const count = root.querySelector<HTMLElement>("[data-probe-count]");
  const empty = root.querySelector<HTMLElement>("[data-probe-empty]");
  if (!form || !search || !result || !disposition || !level || !count || !empty)
    return;

  const groups = Array.from(
    root.querySelectorAll<HTMLElement>("[data-probe-group]"),
  );
  const probes = Array.from(
    root.querySelectorAll<HTMLDetailsElement>("[data-probe]"),
  ).map((element) => ({
    element,
    text: (element.textContent ?? "").toLocaleLowerCase(),
  }));

  function filter(): void {
    if (!search || !result || !disposition || !level || !count || !empty)
      return;
    const terms = search.value
      .trim()
      .toLocaleLowerCase()
      .split(/\s+/)
      .filter(Boolean);
    let visible = 0;
    for (const { element, text } of probes) {
      const matches =
        terms.every((term) => text.includes(term)) &&
        (result.value === "all" || result.value === element.dataset.result) &&
        (disposition.value === "all" ||
          disposition.value === element.dataset.disposition) &&
        (level.value === "all" || level.value === element.dataset.level);
      element.hidden = !matches;
      if (matches) visible += 1;
    }
    for (const group of groups) {
      group.hidden = !group.querySelector("[data-probe]:not([hidden])");
    }
    count.textContent =
      visible === probes.length
        ? `Showing all ${probes.length} probes.`
        : `Showing ${visible} of ${probes.length} probes.`;
    empty.hidden = visible !== 0;
  }

  function clear(): void {
    if (!search || !result || !disposition || !level) return;
    search.value = "";
    result.value = "all";
    disposition.value = "all";
    level.value = "all";
    filter();
  }

  function revealHash(): void {
    let id: string;
    try {
      id = decodeURIComponent(window.location.hash.slice(1));
    } catch {
      return;
    }
    if (!id) return;
    const target = document.getElementById(id);
    if (!target || !root?.contains(target)) return;
    if (target.matches("[data-probe], [data-probe-group]")) {
      clear();
      if (target instanceof HTMLDetailsElement) target.open = true;
      // Reveal first so the browser scrolls to the expanded content, including
      // when a linked probe was hidden by an active filter.
      requestAnimationFrame(() => target.scrollIntoView({ block: "start" }));
    }
  }

  form.hidden = false;
  root.dataset.initialized = "true";
  form.addEventListener("submit", (event) => event.preventDefault());
  form.addEventListener("input", filter);
  form.addEventListener("change", filter);
  form.addEventListener("reset", (event) => {
    event.preventDefault();
    clear();
  });
  root.addEventListener("click", (event) => {
    const link =
      event.target instanceof Element ? event.target.closest("a[href]") : null;
    if (link?.getAttribute("href") === window.location.hash) revealHash();
  });
  window.addEventListener("hashchange", revealHash);
  filter();
  revealHash();
}

initializeProbes();
document.addEventListener("astro:page-load", initializeProbes);
