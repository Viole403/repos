import { useEffect, useState } from "react";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { RadioButton, RadioGroup } from "@/components/base/radio-buttons/radio-buttons";
import type { DatabaseOption, DatabaseStatus } from "@/app/ipc";
import { configureDatabase, databaseOptions, databaseStatus, testDatabaseConnection } from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

const KNOWN_BACKENDS = ["sqlite", "postgres", "mysql"] as const;
type Backend = (typeof KNOWN_BACKENDS)[number];

const isBackend = (value: string): value is Backend => (KNOWN_BACKENDS as readonly string[]).includes(value);

const PLACEHOLDERS: Record<Backend, string> = {
    sqlite: "",
    postgres: "postgres://user:password@localhost:5432/repos",
    mysql: "mysql://user:password@localhost:3306/repos",
};

/**
 * First step of the first-run wizard: which database this install uses.
 *
 * The app supports three engines and runs on exactly one, chosen here. That is the
 * whole point of the step — the choice is recorded and the app stays on it, rather
 * than a build per database or three live at once.
 *
 * It is also the recovery path. A saved database that will not open (moved server,
 * rotated password) lands here with `state: "error"`, because an operator whose
 * server is down needs to be able to say "use SQLite instead" without reinstalling.
 */
export const DatabaseSetup = ({ onConfigured }: { onConfigured: () => void }) => {
    const [options, setOptions] = useState<DatabaseOption[]>([]);
    const [backend, setBackend] = useState<Backend>("sqlite");
    const [url, setUrl] = useState("");
    const [status, setStatus] = useState<DatabaseStatus | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [busy, setBusy] = useState<"testing" | "saving" | null>(null);

    useEffect(() => {
        void databaseOptions().then(setOptions).catch((cause) => setError(messageOf(cause)));
        void databaseStatus()
            .then((found) => {
                setStatus(found);
                // Pre-select what is already recorded so "retry" does not silently
                // retype the connection someone chose last time.
                if (found.backend && isBackend(found.backend)) setBackend(found.backend);
            })
            .catch(() => undefined);
    }, []);

    const option = options.find((o) => o.backend === backend);
    const needsUrl = option?.needsUrl ?? backend !== "sqlite";
    const candidate = needsUrl ? { backend, url: url.trim() } : { backend };

    const test = async () => {
        setError(null);
        setBusy("testing");
        try {
            await testDatabaseConnection(candidate);
            setError(null);
        } catch (cause) {
            setError(messageOf(cause));
        } finally {
            setBusy(null);
        }
    };

    const save = async () => {
        setError(null);
        setBusy("saving");
        try {
            const saved = await configureDatabase(candidate);
            setStatus(saved);
            onConfigured();
        } catch (cause) {
            setError(messageOf(cause));
        } finally {
            setBusy(null);
        }
    };

    return (
        <div className="flex h-dvh items-center justify-center bg-primary px-6">
            <div className="w-full max-w-lg">
                <div className="flex flex-col gap-6 rounded-2xl bg-primary p-6 shadow-xs ring-1 ring-secondary ring-inset">
                    <div className="flex flex-col gap-1">
                        <h1 className="text-display-xs font-semibold text-primary">Choose a database</h1>
                        <p className="text-md text-tertiary">
                            This till stores everything in one database. Pick one now — you can change it later, and the
                            same data is read whichever you choose.
                        </p>
                    </div>

                    {status?.state === "error" && (
                        <div className="rounded-lg bg-warning-secondary px-3 py-2 text-sm text-warning-primary ring-1 ring-warning-primary/10 ring-inset">
                            <p className="font-medium">The saved database could not be opened.</p>
                            {status.location && <p className="mt-1 font-mono text-xs break-all">{status.location}</p>}
                            {status.message && <p className="mt-1">{status.message}</p>}
                        </div>
                    )}

                    {status?.state === "ready" && (
                        <p className="rounded-lg bg-success-secondary px-3 py-2 text-sm text-success-primary">
                            Already using {status.backend}
                            {status.location ? ` at ${status.location}` : ""}.
                        </p>
                    )}

                    <RadioGroup
                        aria-label="Database type"
                        value={backend}
                        onChange={(value) => setBackend(value as Backend)}
                        className="flex flex-col gap-2"
                    >
                        {options.map((o) => (
                            <label
                                key={o.backend}
                                className="flex cursor-pointer items-start gap-3 rounded-xl p-3 ring-1 ring-secondary ring-inset transition-colors has-checked:bg-secondary has-checked:ring-primary has-focus-visible:ring-2 has-focus-visible:ring-primary"
                            >
                                <RadioButton value={o.backend} />
                                <span className="flex flex-col gap-0.5">
                                    <span className="text-sm font-medium text-primary">{o.label}</span>
                                    <span className="text-sm text-tertiary">{o.detail}</span>
                                </span>
                            </label>
                        ))}
                    </RadioGroup>

                    {needsUrl && (
                        <Input
                            label="Connection URL"
                            hint="Host, port, database, and credentials in one URL."
                            placeholder={PLACEHOLDERS[backend]}
                            value={url}
                            onChange={setUrl}
                            isDisabled={busy !== null}
                        />
                    )}

                    {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

                    <div className="flex items-center justify-end gap-3">
                        {needsUrl && (
                            <Button type="button" onClick={() => void test()} isDisabled={busy !== null || !url.trim()}>
                                Test connection
                            </Button>
                        )}
                        <Button
                            type="button"
                            onClick={() => void save()}
                            isDisabled={busy !== null || (needsUrl && !url.trim())}
                            isLoading={busy === "saving"}
                            showTextWhileLoading
                        >
                            {status?.state === "ready" ? "Switch database" : "Use this database"}
                        </Button>
                    </div>
                </div>
            </div>
        </div>
    );
};