import { Outlet, useLocation } from "react-router-dom";
import { NavList } from "@/components/application/app-navigation/base-components/nav-list";
import { navItems } from "./nav-config";

/** Persistent chrome around every screen: fixed sidebar plus the routed outlet. Desktop-only — see AGENTS.md. */
export const AppShell = () => {
    const { pathname } = useLocation();

    return (
        <div className="flex h-dvh flex-col bg-primary lg:flex-row">
            <aside className="hidden w-64 shrink-0 flex-col border-r border-secondary bg-primary lg:flex">
                <div className="flex h-14 shrink-0 items-center border-b border-secondary px-4">
                    <span className="text-base font-semibold text-primary">repos</span>
                </div>

                <nav className="flex-1 overflow-y-auto pb-4">
                    {/* hrefs are hash-prefixed, so the active key must match that shape */}
                    <NavList items={navItems} activeUrl={`#${pathname}`} />
                </nav>
            </aside>

            <main className="flex-1 overflow-y-auto">
                <Outlet />
            </main>
        </div>
    );
};