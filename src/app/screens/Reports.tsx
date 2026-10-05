import { useCallback, useEffect, useState } from "react";
import { Table, TableCard } from "@/components/application/table/table";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { decIsNegative } from "@/app/decimal";
import { formatMoney } from "@/app/format";
import { balanceSheet, trialBalance } from "@/app/ipc";
import type { BalanceSheet, TrialBalance, TrialLine } from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

const GROUPS: Record<string, string> = {
    Cash: "Cash and tenders",
    Receivables: "Owed by customers",
    Payables: "Owed to suppliers",
    OwnerEquity: "Owner in and out",
    Revenue: "Taken",
    Expenses: "Spent",
};

const Section = ({ title, rows }: { title: string; rows: TrialLine[] }) => (
    <div className="flex flex-col gap-2">
        <span className="text-md font-semibold text-primary">{title}</span>
        {rows.length === 0 ? (
            <p className="text-sm text-tertiary">Nothing in this section.</p>
        ) : (
            <Table aria-label={title}>
                <Table.Header>
                    <Table.Head label="Group" />
                    <Table.Head label="Amount" className="text-right" />
                </Table.Header>
                <Table.Body>
                    {rows.map((row) => (
                        <Table.Row key={row.group}>
                            <Table.Cell>{GROUPS[row.group] ?? row.group}</Table.Cell>
                            <Table.Cell
                                className={`text-right ${decIsNegative(row.net) ? "text-error-primary" : undefined}`}
                            >
                                {formatMoney(row.net)}
                            </Table.Cell>
                        </Table.Row>
                    ))}
                </Table.Body>
            </Table>
        )}
    </div>
);

/** A zero difference is the only correct answer, so it is shown rather than assumed —
 *  a figure that should not balance says so instead of looking like a number. */
const Difference = ({ value, what }: { value: string; what: string }) =>
    value === "0" ? null : (
        <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">
            {what} does not balance: {value}. A figure that should be zero is not, so something in
            the books moved without a counterpart.
        </p>
    );

export const Reports = () => {
    const [trial, setTrial] = useState<TrialBalance | null>(null);
    const [sheet, setSheet] = useState<BalanceSheet | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [busy, setBusy] = useState(false);
    const [from, setFrom] = useState("");
    const [to, setTo] = useState("");
    const [view, setView] = useState<"trial" | "sheet">("trial");

    const load = useCallback(async () => {
        setBusy(true);
        try {
            const window_ = {
                from: from === "" ? null : from,
                to: to === "" ? null : to,
            };
            const [t, s] = await Promise.all([trialBalance(window_.from, window_.to), balanceSheet(window_.from, window_.to)]);
            setTrial(t);
            setSheet(s);
            setError(null);
        } catch (cause) {
            setError(messageOf(cause));
        } finally {
            setBusy(false);
        }
    }, [from, to]);

    useEffect(() => {
        void load();
    }, [load]);

    return (
        <div className="flex flex-col gap-5 p-5">
            <div className="flex flex-col gap-1">
                <h1 className="text-display-xs font-semibold text-primary">Reports</h1>
                <p className="text-md text-tertiary">
                    Derived from the same rows the cash book reads. Stock is not valued here — that
                    needs a cost per unit sold, which is profit and loss work.
                </p>
            </div>

            <div className="flex flex-wrap items-end gap-3">
                <Input
                    label="From"
                    type="date"
                    value={from}
                    onChange={(value) => setFrom(String(value))}
                    className="w-44"
                />
                <Input label="To" type="date" value={to} onChange={(value) => setTo(String(value))} className="w-44" />
                <Button color="secondary" isLoading={busy} onPress={() => void load()}>
                    Refresh
                </Button>
            </div>

            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

            <div className="flex gap-2">
                <Button
                    size="sm"
                    color={view === "trial" ? "secondary" : "tertiary"}
                    onPress={() => setView("trial")}
                >
                    Trial balance
                </Button>
                <Button
                    size="sm"
                    color={view === "sheet" ? "secondary" : "tertiary"}
                    onPress={() => setView("sheet")}
                >
                    Balance sheet
                </Button>
            </div>

            {trial && view === "trial" && (
                <div className="flex flex-col gap-4">
                            <Difference value={trial.difference} what="The trial balance" />
                            <TableCard.Root>
                                <TableCard.Header
                                    title="Trial balance"
                                    description={`Dr ${formatMoney(trial.totalDebit)} · Cr ${formatMoney(trial.totalCredit)}`}
                                />
                                <Table aria-label="Trial balance">
                                    <Table.Header>
                                        <Table.Head label="Group" />
                                        <Table.Head label="Debit" className="text-right" />
                                        <Table.Head label="Credit" className="text-right" />
                                        <Table.Head label="Net" className="text-right" />
                                    </Table.Header>
                                    <Table.Body>
                                        {trial.lines.map((row) => (
                                            <Table.Row key={row.group}>
                                                <Table.Cell>{GROUPS[row.group] ?? row.group}</Table.Cell>
                                                <Table.Cell className="text-right">{formatMoney(row.debit)}</Table.Cell>
                                                <Table.Cell className="text-right">{formatMoney(row.credit)}</Table.Cell>
                                                <Table.Cell
                                                    className={`text-right ${decIsNegative(row.net) ? "text-error-primary" : undefined}`}
                                                >
                                                    {formatMoney(row.net)}
                                                </Table.Cell>
                                            </Table.Row>
                                        ))}
                                    </Table.Body>
                                </Table>
                            </TableCard.Root>
                </div>
            )}

            {sheet && view === "sheet" && (
                <div className="flex flex-col gap-5">
                    <Difference value={sheet.difference} what="The balance sheet" />
                    <Section title={`Assets · ${formatMoney(sheet.totalAssets)}`} rows={sheet.assets} />
                    <Section title={`Owed · ${formatMoney(sheet.totalLiabilities)}`} rows={sheet.liabilities} />
                    <Section title={`Equity and result · ${formatMoney(sheet.totalEquity)}`} rows={sheet.equity} />
                </div>
            )}
        </div>
    );
};
