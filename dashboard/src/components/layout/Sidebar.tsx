import { NavLink, useLocation } from "react-router-dom";
import { useEffect } from "react";
import {
  LayoutDashboard,
  Users,
  Cpu,
  Key,
  Settings as SettingsIcon,
  Activity,
  BarChart3,
  Sliders,
  Bot,
  CreditCard,
  Globe,
  Filter,
  Plug,
  Database,
  LogOut,
  X,
  Sun,
  Moon,
  Zap,
  Terminal,
  Layers,

} from "lucide-react";
import { cn } from "@/lib/utils";
import { useTheme } from "@/hooks/useTheme";
import { useWsStatus } from "@/hooks/useWebSocket";

interface NavItem {
  label: string;
  path: string;
  icon: React.ComponentType<{ className?: string }>;
}

interface NavSection {
  title: string;
  items: NavItem[];
}

// Navigation grouped to match the Luminus reference layout:
//   MAIN     — Overview, Usage, Providers (accounts + BYOK), Model Studio
//   CONTROL  — filter rules, API keys, proxy requests, and integrations
//   SYSTEM   — Console Log, Customization (9router import), Settings
const navSections: NavSection[] = [
  {
    title: "MAIN",
    items: [
      { label: "Overview", path: "/", icon: LayoutDashboard },
      { label: "Usage", path: "/usage", icon: BarChart3 },
      { label: "Providers", path: "/accounts", icon: Users },
      { label: "Model Studio", path: "/models", icon: Cpu },
      { label: "Image Studio", path: "/image-studio", icon: Layers },
    ],
  },
  {
    title: "CONTROL",
    items: [
      { label: "Filter Rules", path: "/filter-rules", icon: Filter },
      { label: "API Key", path: "/api-key", icon: Key },
      { label: "Proxy & Requests", path: "/requests", icon: Activity },
      { label: "Integration", path: "/integration", icon: Plug },
    ],
  },
  {
    title: "SYSTEM",
    items: [
      { label: "Console Log", path: "/console-log", icon: Terminal },
      { label: "Login Logs", path: "/bot-logs", icon: Bot },
      { label: "Import 9router", path: "/migration", icon: Database },
      { label: "Proxy Settings", path: "/settings", icon: Sliders },
    ],
  },
];

interface SidebarProps {
  onLogout?: () => void;
  open?: boolean;
  onClose?: () => void;
}

/**
 * Luminus sidebar (matches Cartethyia reference layout):
 *  - Desktop (md+): full-width 240px with icon + label + section headers.
 *  - Mobile: slide-in drawer (240px), same content.
 */
export default function Sidebar({ onLogout, open, onClose }: SidebarProps) {
  const location = useLocation();
  const { theme, toggleTheme } = useTheme();
  const wsStatus = useWsStatus();

  useEffect(() => {
    onClose?.();
  }, [location.pathname]);

  const wsMeta =
    wsStatus === "open"
      ? { color: "var(--success)", label: "Live" }
      : wsStatus === "connecting"
        ? { color: "var(--warning)", label: "Connecting" }
        : { color: "var(--error)", label: "Offline" };

  return (
    <aside
      className={cn(
        "fixed top-0 left-0 h-screen w-[240px] bg-[var(--sidebar-bg)] border-r border-[var(--sidebar-border)] flex flex-col z-50 transition-transform duration-200",
        open ? "translate-x-0" : "-translate-x-full md:translate-x-0"
      )}
    >
      {/* Logo / Brand */}
      <div className="flex items-center justify-between gap-2 p-3 border-b border-[var(--sidebar-border)]">
        <div className="flex items-center gap-2.5 min-w-0">
          <div className="relative w-9 h-9 rounded-md border border-[var(--border)] bg-[var(--card)] flex items-center justify-center flex-shrink-0">
            <Zap className="w-4 h-4 text-[var(--primary)]" strokeWidth={2.5} />
          </div>
          <div className="min-w-0">
            <h1 className="text-base font-bold text-[var(--foreground)] tracking-tight leading-none truncate">
              Luminus
            </h1>
            <span className="flex items-center gap-1.5 text-[11px] text-[var(--muted-foreground)] mt-1">
              <span
                className="inline-block w-1.5 h-1.5 rounded-full flex-shrink-0"
                style={{ backgroundColor: wsMeta.color }}
              />
              <span className="truncate">{wsMeta.label}</span>
            </span>
          </div>
        </div>
        {onClose && (
          <button
            onClick={onClose}
            className="p-1 rounded-md text-[var(--muted-foreground)] hover:text-[var(--foreground)] hover:bg-[var(--sidebar-hover)] md:hidden"
            aria-label="Close menu"
          >
            <X className="w-4 h-4" />
          </button>
        )}
      </div>

      {/* Navigation */}
      <nav className="flex-1 overflow-y-auto py-3 px-2">
        {navSections.map((section) => (
          <div key={section.title} className="mb-5 last:mb-0">
            <h2 className="text-[10px] font-semibold text-[var(--muted-foreground)] uppercase tracking-[0.18em] px-3 mb-1.5">
              {section.title}
            </h2>
            <ul className="space-y-0.5">
              {section.items.map((item) => (
                <li key={item.path}>
                  <NavLink
                    to={item.path}
                    end={item.path === "/"}
                    className={({ isActive }) =>
                      cn(
                        "flex items-center gap-3 rounded-md text-sm transition-colors relative group",
                        "px-3 py-2",
                        isActive
                          ? "bg-[var(--sidebar-active)] text-[var(--primary)] font-medium"
                          : "text-[var(--muted-foreground)] hover:text-[var(--foreground)] hover:bg-[var(--sidebar-hover)]"
                      )
                    }
                  >
                    {({ isActive }) => (
                      <>
                        {isActive && (
                          <span
                            className="absolute left-0 top-1/2 -translate-y-1/2 h-5 w-0.5 rounded-r bg-[var(--primary)]"
                          />
                        )}
                        <item.icon className="w-4 h-4 flex-shrink-0" />
                        <span className="truncate">{item.label}</span>
                      </>
                    )}
                  </NavLink>
                </li>
              ))}
            </ul>
          </div>
        ))}
      </nav>

      {/* Bottom: Theme & Logout */}
      <div className="p-2 border-t border-[var(--sidebar-border)] space-y-0.5">
        <button
          onClick={toggleTheme}
          className={cn(
            "flex items-center gap-3 rounded-md text-sm transition-colors text-[var(--muted-foreground)] hover:text-[var(--foreground)] hover:bg-[var(--sidebar-hover)] w-full",
            "px-3 py-2"
          )}
          aria-label="Toggle theme"
        >
          {theme === "dark" ? <Sun className="w-4 h-4 flex-shrink-0" /> : <Moon className="w-4 h-4 flex-shrink-0" />}
          <span>{theme === "dark" ? "Light Mode" : "Dark Mode"}</span>
        </button>

        {onLogout && (
          <button
            onClick={onLogout}
            className={cn(
              "flex items-center gap-3 rounded-md text-sm transition-colors text-[var(--muted-foreground)] hover:text-[var(--destructive)] hover:bg-[var(--destructive)]/10 w-full",
              "px-3 py-2"
            )}
            aria-label="Logout"
          >
            <LogOut className="w-4 h-4 flex-shrink-0" />
            <span>Logout</span>
          </button>
        )}
      </div>
    </aside>
  );
}
