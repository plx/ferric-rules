document.querySelectorAll("[data-copy-command]").forEach((button) => {
  const command = button.getAttribute("data-copy-command") || "";
  button.addEventListener("click", async () => {
    try {
      await navigator.clipboard?.writeText(command);
    } catch {
      // Clipboard can be unavailable for local file previews.
    }

    const strip = button.closest(".install-command");
    strip?.classList.add("is-copied");
    window.setTimeout(() => strip?.classList.remove("is-copied"), 1200);
  });
});
