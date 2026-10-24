// The page works without JavaScript; this only adds screenshot switching.
const appearances = {
  light: {
    image: "screenshots/reader-light.png",
    alt: "Bible Reader in Light: John 3 with red-letter text and a highlighted verse.",
    caption: "Light theme · Adjustable fonts and reading width.",
  },
  sepia: {
    image: "screenshots/reader-sepia-notes.png",
    alt: "Bible Reader in Sepia: Romans 8 with a book sidebar and footnotes panel.",
    caption: "Sepia theme · Footnotes alongside the text.",
  },
  dark: {
    image: "screenshots/reader-dark.png",
    alt: "Bible Reader in Dark: Psalm 23 in an editorial serif font.",
    caption: "Dark theme · Six bundled font pairings.",
  },
};

const picker = document.querySelector(".theme-picker");
const preview = document.querySelector("#reader-preview");
const caption = document.querySelector("#preview-caption");
const frame = document.querySelector(".window-frame");

if (picker && preview && caption && frame) {
  picker.hidden = false;
  picker.addEventListener("click", (event) => {
    const button = event.target.closest("button[data-theme]");
    if (!button) return;
    const appearance = appearances[button.dataset.theme];
    if (!appearance) return;

    preview.src = appearance.image;
    preview.alt = appearance.alt;
    caption.textContent = appearance.caption;
    frame.dataset.previewTheme = button.dataset.theme;
    for (const option of picker.querySelectorAll("button")) {
      option.setAttribute("aria-pressed", String(option === button));
    }
  });
}
