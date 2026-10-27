// Runs before the first paint (a classic script in <head>, so it blocks): puts the theme on <html>
// the way `initialTheme` (src/app/shared.ts) will, so the page background (index.html) is the right
// one from the first frame and the window does not flash light before it turns dark. The key is
// `THEME_KEY` there. Without it, or with storage blocked, the system's theme.
(function () {
  var theme = "light";
  try {
    var stored = localStorage.getItem("qiyas.theme");
    if (stored === "light" || stored === "dark") theme = stored;
    else if (matchMedia("(prefers-color-scheme: dark)").matches) theme = "dark";
  } catch (e) {
    try {
      if (matchMedia("(prefers-color-scheme: dark)").matches) theme = "dark";
    } catch (e2) {
      // Light.
    }
  }
  document.documentElement.dataset.theme = theme;
})();
