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

async function copyText(text: string): Promise<void> {
  if (!navigator.clipboard) {
    throw new Error("Clipboard API is unavailable.");
  }

  await navigator.clipboard.writeText(text);
}

document
  .querySelectorAll<HTMLButtonElement>("[data-copy-text]")
  .forEach((button) => {
    let resetTimer: number | undefined;
    const visibleLabel =
      button.querySelector<HTMLElement>("span:not(.sr-only)");
    const status = button.querySelector<HTMLElement>("[data-copy-status]");
    const defaultVisibleText = visibleLabel?.textContent || "Copy";

    button.addEventListener("click", async () => {
      const text = button.dataset.copyText;
      if (!text) {
        return;
      }

      window.clearTimeout(resetTimer);
      try {
        await copyText(text);
        button.closest(".install-command")?.classList.add("is-copied");
        if (visibleLabel) {
          visibleLabel.textContent = "Copied";
        }
        if (status) {
          status.textContent = "Command copied to clipboard.";
        }
      } catch {
        if (visibleLabel) {
          visibleLabel.textContent = "Copy failed";
        }
        if (status) {
          status.textContent = "Copy failed.";
        }
      }

      resetTimer = window.setTimeout(() => {
        button.closest(".install-command")?.classList.remove("is-copied");
        if (visibleLabel) {
          visibleLabel.textContent = defaultVisibleText;
        }
        if (status) {
          status.textContent = "";
        }
      }, 2200);
    });
  });
