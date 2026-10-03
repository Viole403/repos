import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { HOME_PATH, useAuth } from "@/app/auth";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

/**
 * Full-screen sign-in: no sidebar, because nothing behind it works yet. A real
 * failure always means "wrong email or password" — the backend says the same
 * thing either way so the form cannot be used to find out which accounts exist.
 */
export const Login = () => {
    const { login } = useAuth();
    const navigate = useNavigate();

    const [email, setEmail] = useState("");
    const [password, setPassword] = useState("");
    const [error, setError] = useState<string | null>(null);
    const [submitting, setSubmitting] = useState(false);

    const submit = async () => {
        setError(null);
        setSubmitting(true);
        try {
            await login(email, password);
            navigate(HOME_PATH, { replace: true });
        } catch (cause) {
            setError(messageOf(cause));
        } finally {
            setSubmitting(false);
        }
    };

    return (
        <div className="flex h-dvh items-center justify-center bg-primary px-6">
            <div className="w-full max-w-sm">
                <form
                    className="flex flex-col gap-5 rounded-2xl bg-primary p-6 shadow-xs ring-1 ring-secondary ring-inset"
                    onSubmit={(event) => {
                        event.preventDefault();
                        void submit();
                    }}
                >
                    <div className="flex flex-col gap-1">
                        <h1 className="text-display-xs font-semibold text-primary">Sign in</h1>
                        <p className="text-md text-tertiary">Use the account this till was set up with.</p>
                    </div>

                    {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

                    <Input
                        label="Email"
                        type="email"
                        autoComplete="username"
                        value={email}
                        onChange={setEmail}
                        isRequired
                        isDisabled={submitting}
                    />
                    <Input
                        label="Password"
                        type="password"
                        autoComplete="current-password"
                        value={password}
                        onChange={setPassword}
                        isRequired
                        isDisabled={submitting}
                    />

                    <Button type="submit" size="lg" isLoading={submitting} showTextWhileLoading>
                        Sign in
                    </Button>
                </form>
            </div>
        </div>
    );
};

/** Shown for the one round trip between mount and a restored session. */
export const SessionLoading = () => (
    <div className="flex h-dvh items-center justify-center bg-primary">
        <p className="text-md text-tertiary">Restoring session…</p>
    </div>
);