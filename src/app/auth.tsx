import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import type { UserView } from "@/app/ipc";
import { currentUser, login as loginRequest, logout as logoutRequest } from "@/app/ipc";

/**
 * Session state for the whole app. The backend keeps the signed-in id in a
 * process-wide cell, so "am I logged in" is a single round trip, not a token
 * this side has to store — which also means there is nothing in localStorage to
 * leak and nothing to expire.
 */

type AuthStatus = "restoring" | "authenticated" | "anonymous";

interface AuthState {
    status: AuthStatus;
    user: UserView | null;
    /** Signs in; throws `IpcError` with the backend's message on bad credentials. */
    login: (email: string, password: string) => Promise<void>;
    logout: () => Promise<void>;
}

const AuthContext = createContext<AuthState | null>(null);

export const AuthProvider = ({ children }: { children: ReactNode }) => {
    const [status, setStatus] = useState<AuthStatus>("restoring");
    const [user, setUser] = useState<UserView | null>(null);

    useEffect(() => {
        // A rejection here means "no session", which is the expected state on a
        // cold launch — the command rejects rather than returning null, and there
        // is no way to tell that apart from a real failure over the wire. Either
        // way the operator lands on the login screen, which is where an anonymous
        // launch belongs.
        void currentUser()
            .then((found) => {
                setUser(found);
                setStatus("authenticated");
            })
            .catch(() => setStatus("anonymous"));
    }, []);

    const login = useCallback(async (email: string, password: string) => {
        const found = await loginRequest({ email: email.trim(), password });
        setUser(found);
        setStatus("authenticated");
    }, []);

    const logout = useCallback(async () => {
        // Cleared locally whatever the call does: a sign-out the cashier cannot
        // see is worse than a backend cell that is briefly still set.
        try {
            await logoutRequest();
        } finally {
            setUser(null);
            setStatus("anonymous");
        }
    }, []);

    const value = useMemo<AuthState>(() => ({ status, user, login, logout }), [status, user, login, logout]);

    return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
};

export const useAuth = (): AuthState => {
    const state = useContext(AuthContext);
    if (!state) throw new Error("useAuth must be used inside <AuthProvider>");
    return state;
};

/** The route anonymous users are sent to. Full-screen: nothing works before login. */
export const LOGIN_PATH = "/login";

/** Where authenticated users land. */
export const HOME_PATH = "/";