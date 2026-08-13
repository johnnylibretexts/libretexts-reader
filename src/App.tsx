import { useEffect } from "react";
import { AppShell } from "./components/AppShell";
import { useSettingsStore } from "./stores/settings";

function App() {
  const hydrate = useSettingsStore((state) => state.hydrate);
  const theme = useSettingsStore((state) => state.theme);

  useEffect(() => {
    void hydrate();
  }, [hydrate]);

  useEffect(() => {
    const root = document.documentElement;
    const media = window.matchMedia("(prefers-color-scheme: dark)");

    const applyTheme = () => {
      const shouldUseDark =
        theme === "dark" || (theme === "system" && media.matches);
      root.classList.toggle("dark", shouldUseDark);
    };

    applyTheme();
    media.addEventListener("change", applyTheme);
    return () => media.removeEventListener("change", applyTheme);
  }, [theme]);

  return <AppShell />;
}

export default App;
