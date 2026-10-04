import { Outlet, useLocation } from "react-router-dom";
import { NavList } from "@/components/application/app-navigation/base-components/nav-list";
import { AvatarLabelGroup } from "@/components/base/avatar/avatar-label-group";
import { Button } from "@/components/base/buttons/button";
import { LogOut01 } from "@untitledui/icons";
import { visibleNavItems } from "./nav-config";
import { useAuth } from "./auth";

/** First letters of a name, for the avatar when no photo is set. */
const initialsOf = (name: string): string =>
    name
        .split(/\s+/)
        .filter(Boolean)
        .slice(0, 2)
        .map((part) => part[0]?.toUpperCase() ?? "")
        .join("");

/**
 * Persistent chrome around every screen: fixed sidebar plus the routed outlet.
 * Desktop-only — see AGENTS.md.
 */
export const AppShell = () => {
    const { pathname } = useLocation();
    const { user, logout, permissions } = useAuth();

    return (
        <div className="flex h-dvh flex-col bg-primary lg:flex-row">
            <aside className="hidden w-64 shrink-0 flex-col border-r border-secondary bg-primary lg:flex">
                <div className="flex h-14 shrink-0 items-center border-b border-secondary px-4">
                    <span className="text-base font-semibold text-primary">repos</span>
                </div>

                <nav className="flex-1 overflow-y-auto pb-4">
                    {/* hrefs are hash-prefixed, so the active key must match that shape */}
                    <NavList items={visibleNavItems(permissions)} activeUrl={`#${pathname}`} />
                </nav>

                {/* Only rendered once auth exists, and only ever from `currentUser` — see AGENTS.md. */}
                {user && (
                    <div className="flex flex-col gap-2 border-t border-secondary p-3">
                        <AvatarLabelGroup
                            size="md"
                            src={user.photo}
                            initials={initialsOf(user.name)}
                            title={user.name}
                            subtitle={user.email}
                        />
                        <Button color="secondary" size="sm" iconLeading={LogOut01} onPress={() => void logout()}>
                            Sign out
                        </Button>
                    </div>
                )}
            </aside>

            <main className="flex-1 overflow-y-auto">
                <Outlet />
            </main>
        </div>
    );
};