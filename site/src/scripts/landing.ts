const navToggle =
  document.querySelector<HTMLButtonElement>("[data-nav-toggle]");
const navPanel = document.querySelector<HTMLElement>("[data-nav-panel]");
const navLinks = document.querySelectorAll<HTMLElement>("[data-nav-link]");
const navScrim = document.querySelector<HTMLElement>("[data-nav-scrim]");

function setNavOpen(open: boolean): void {
  if (!navToggle || !navPanel) {
    return;
  }

  navToggle.setAttribute("aria-expanded", String(open));
  navToggle.setAttribute(
    "aria-label",
    open ? "Close navigation" : "Open navigation",
  );
  navPanel.hidden = !open;
  navPanel.dataset.open = String(open);
  if (navScrim) {
    navScrim.dataset.open = String(open);
  }
}

navToggle?.addEventListener("click", () => {
  setNavOpen(navToggle.getAttribute("aria-expanded") !== "true");
});

navLinks.forEach((link) => {
  link.addEventListener("click", () => setNavOpen(false));
});

navScrim?.addEventListener("click", () => setNavOpen(false));

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    setNavOpen(false);
    navToggle?.focus();
  }
});
