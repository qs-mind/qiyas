import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./fonts/fonts.css";
import { LanguageProvider } from "./i18n";
import { installSafePaste } from "./shell/safePaste";
import "./shell/theme.css";

installSafePaste();

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <LanguageProvider>
      <App />
    </LanguageProvider>
  </StrictMode>,
);
