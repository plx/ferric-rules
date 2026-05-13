document.querySelectorAll("[data-toggle-selected]").forEach((button) => {
  button.addEventListener("click", () => {
    button.classList.toggle("is-selected");
    button.setAttribute("aria-pressed", String(button.classList.contains("is-selected")));
  });
});
