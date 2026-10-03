import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { HOME_PATH, useAuth } from "@/app/auth";
import { createUser } from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

/**
 * Shown instead of the login form when no account exists, because that form cannot
 * succeed on an install nobody has claimed yet. Creates the owner and signs in.
 */
export const Setup = () => {
    const { login } = useAuth();
    const navigate = useNavigate();

    const [name, setName] = useState("");
    const [email, setEmail] = useState("");
    const [password, setPassword] = useState("");
    const [confirm, setConfirm] = useState("");
    const [error, setError] = useState<string | null>(null);
    const [submitting, setSubmitting] = useState(false);

    const submit = async () => {
        setError(null);
        if (password !== confirm) {
            setError("the two passwords do not match");
            return;
        }

        setSubmitting(true);
        try {
            await createUser({ name: name.trim(), email: email.trim(), password });
            await login(email.trim(), password);
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
                        <h1 className="text-display-xs font-semibold text-primary">Set up this till</h1>
                        <p className="text-md text-tertiary">
                            The first account owns the install: it can add operators and change every setting.
                        </p>
                    </div>

                    {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

                    <Input
                        label="Name"
                        autoComplete="name"
                        value={name}
                        onChange={setName}
                        isRequired
                        isDisabled={submitting}
                    />
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
                        autoComplete="new-password"
                        value={password}
                        onChange={setPassword}
                        isRequired
                        isDisabled={submitting}
                    />
                    <Input
                        label="Confirm password"
                        type="password"
                        autoComplete="new-password"
                        value={confirm}
                        onChange={setConfirm}
                        isRequired
                        isDisabled={submitting}
                    />

                    <Button type="submit" size="lg" isLoading={submitting} showTextWhileLoading>
                        Create account
                    </Button>
                </form>
            </div>
        </div>
    );
};