import {
  BookOpen,
  Clipboard,
  FileText,
  Library,
  Link,
  Monitor,
  Moon,
  Settings,
  Sun,
  Upload,
  type LucideIcon,
} from "lucide-react";
import type { RouteId } from "./AppShell";
import { type AppTheme, useSettingsStore } from "../stores/settings";

interface SidebarProps {
  activeRoute: RouteId;
  onNavigate: (route: RouteId) => void;
}

const primaryRoutes: Array<{
  id: RouteId;
  label: string;
  icon: LucideIcon;
}> = [
  { id: "library", label: "Library", icon: Library },
  { id: "reader", label: "Reader", icon: BookOpen },
];

const importRoutes: Array<{
  id: RouteId;
  label: string;
  icon: LucideIcon;
}> = [
  { id: "openstax", label: "OpenStax", icon: BookOpen },
  { id: "libretexts", label: "LibreTexts", icon: BookOpen },
  { id: "epub", label: "EPUB", icon: Upload },
  { id: "pdf", label: "PDF", icon: FileText },
  { id: "paste", label: "Paste", icon: Clipboard },
  { id: "url", label: "URL", icon: Link },
];

const themeOptions: Array<{
  theme: AppTheme;
  label: string;
  icon: LucideIcon;
}> = [
  { theme: "light", label: "Light", icon: Sun },
  { theme: "dark", label: "Dark", icon: Moon },
  { theme: "system", label: "System", icon: Monitor },
];

export function Sidebar({ activeRoute, onNavigate }: SidebarProps) {
  const theme = useSettingsStore((state) => state.theme);
  const setTheme = useSettingsStore((state) => state.setTheme);

  return (
    <aside className="flex w-20 shrink-0 flex-col border-r border-neutral-200 bg-white dark:border-neutral-800 dark:bg-neutral-950 md:w-72">
      <div className="flex min-h-16 items-center justify-center gap-3 border-b border-neutral-200 px-3 dark:border-neutral-800 md:justify-start md:px-4">
        <div className="grid size-9 place-items-center rounded-md bg-brand-700 text-white">
          <BookOpen className="size-5" aria-hidden="true" />
        </div>
        <div className="hidden min-w-0 md:block">
          <p className="truncate text-sm font-semibold">LibreTexts Reader</p>
          <p className="truncate text-xs text-neutral-500 dark:text-neutral-400">
            Offline narration
          </p>
        </div>
      </div>

      <nav
        className="flex min-h-0 flex-1 flex-col gap-6 overflow-auto px-2 py-4 md:px-3"
        aria-label="Main"
      >
        <div className="space-y-1">
          {primaryRoutes.map((item) => (
            <SidebarButton
              active={activeRoute === item.id}
              icon={item.icon}
              key={item.id}
              label={item.label}
              onClick={() => onNavigate(item.id)}
            />
          ))}
        </div>

        <div>
          <p className="hidden px-3 pb-2 text-xs font-semibold uppercase text-neutral-500 dark:text-neutral-400 md:block">
            Import
          </p>
          <div className="space-y-1">
            {importRoutes.map((item) => (
              <SidebarButton
                active={activeRoute === item.id}
                icon={item.icon}
                key={item.id}
                label={item.label}
                onClick={() => onNavigate(item.id)}
              />
            ))}
          </div>
        </div>
      </nav>

      <div className="border-t border-neutral-200 p-2 dark:border-neutral-800 md:p-3">
        <SidebarButton
          active={activeRoute === "settings"}
          icon={Settings}
          label="Settings"
          onClick={() => onNavigate("settings")}
        />

        <div className="mt-3 grid grid-cols-3 gap-1 rounded-md border border-neutral-200 bg-stone-100 p-1 dark:border-neutral-800 dark:bg-neutral-900">
          {themeOptions.map((option) => (
            <button
              aria-label={`${option.label} theme`}
              className={`grid h-8 place-items-center rounded text-neutral-600 transition hover:bg-white focus:outline-none focus:ring-2 focus:ring-brand-500 dark:text-neutral-300 dark:hover:bg-neutral-800 ${
                theme === option.theme
                  ? "bg-white text-brand-700 shadow-sm dark:bg-neutral-800 dark:text-brand-500"
                  : ""
              }`}
              key={option.theme}
              onClick={() => {
                // setTheme rethrows on a failed persist so an awaiting
                // caller can react; this button doesn't await it, so it
                // must catch here itself or the rejection goes unhandled.
                // The store already recorded the failure in its shared
                // `error` field.
                void setTheme(option.theme).catch(() => {});
              }}
              title={`${option.label} theme`}
              type="button"
            >
              <option.icon className="size-4" aria-hidden="true" />
            </button>
          ))}
        </div>
      </div>
    </aside>
  );
}

function SidebarButton({
  active,
  icon: Icon,
  label,
  onClick,
}: {
  active: boolean;
  icon: LucideIcon;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      aria-label={label}
      className={`flex h-10 w-full items-center justify-center gap-3 rounded-md px-0 text-left text-sm font-medium transition focus:outline-none focus:ring-2 focus:ring-brand-500 md:justify-start md:px-3 ${
        active
          ? "bg-brand-50 text-brand-700 dark:bg-neutral-900 dark:text-brand-500"
          : "text-neutral-700 hover:bg-stone-100 dark:text-neutral-300 dark:hover:bg-neutral-900"
      }`}
      onClick={onClick}
      type="button"
    >
      <Icon className="size-4 shrink-0" aria-hidden="true" />
      <span className="hidden min-w-0 truncate md:inline">{label}</span>
    </button>
  );
}
