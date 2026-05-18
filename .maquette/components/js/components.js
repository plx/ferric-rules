document.querySelectorAll("[data-toggle-selected]").forEach((button) => {
  button.addEventListener("click", () => {
    button.classList.toggle("is-selected");
    button.setAttribute("aria-pressed", String(button.classList.contains("is-selected")));
  });
});

document.querySelectorAll("[data-nav-toggle]").forEach((toggle) => {
  const panel = document.getElementById(toggle.getAttribute("aria-controls"));
  const scrim = document.querySelector("[data-nav-scrim]");

  function setOpen(nextOpen) {
    toggle.setAttribute("aria-expanded", String(nextOpen));
    panel?.setAttribute("data-open", String(nextOpen));
    scrim?.setAttribute("data-open", String(nextOpen));
  }

  toggle.addEventListener("click", () => {
    setOpen(toggle.getAttribute("aria-expanded") !== "true");
  });

  scrim?.addEventListener("click", () => setOpen(false));

  window.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      setOpen(false);
      toggle.focus();
    }
  });
});
