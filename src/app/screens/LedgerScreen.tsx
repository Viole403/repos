import { useCallback, useEffect, useMemo, useState } from "react";
import { Plus, SearchLg } from "@untitledui/icons";
import { Table, TableCard } from "@/components/application/table/table";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { formatMoney, toDecimal } from "@/app/format";
import { listPaymentMethods } from "@/app/ipc";
import type { AccountingCategory, AccountingEntry, Decimal, PageQuery, PaymentMethod } from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

const today = () => new Date().toISOString().slice(0, 10);

/** One screen serves both ledgers. Income and expense differ in which command they
 *  call and in which way the amount reads, and nothing else — three near-identical
 *  files would drift, which is what this config exists to prevent. */
export interface LedgerConfig {
    title: string;
    /** What the money is: "comes in" reads "received", "goes out" reads "paid". */
    blurb: string;
    /** The account picker says "arrived in" versus "left". */
    accountVerb: string;
    submitLabel: string;
    load: (query: PageQuery) => Promise<{ rows: AccountingEntry[]; total: number }>;
    create: (input: {
        categoryId: number;
        paymentMethodId: number;
        amount: Decimal;
        occurredAt: string;
        note?: string | null;
    }) => Promise<AccountingEntry>;
    loadCategories: (query: PageQuery) => Promise<{ rows: AccountingCategory[]; total: number }>;
}

const accountItems = (accounts: PaymentMethod[]): SelectItemType[] =>
    accounts.map((account) => ({ id: String(account.id), label: account.name }));

export const LedgerScreen = ({ config }: { config: LedgerConfig }) => {
    const [rows, setRows] = useState<AccountingEntry[]>([]);
    const [total, setTotal] = useState(0);
    const [page, setPage] = useState(1);
    const [term, setTerm] = useState("");
    const [error, setError] = useState<string | null>(null);
    const [busy, setBusy] = useState(false);
    const [reload, setReload] = useState(0);

    const [accounts, setAccounts] = useState<PaymentMethod[]>([]);
    const [categories, setCategories] = useState<AccountingCategory[]>([]);

    const [recording, setRecording] = useState(false);
    const [categoryId, setCategoryId] = useState("");
    const [accountId, setAccountId] = useState("");
    const [amount, setAmount] = useState("");
    const [occurredAt, setOccurredAt] = useState(today);
    const [note, setNote] = useState("");

    const perPage = 20;

    const refresh = useCallback(async () => {
        try {
            const result = await config.load({ page, perPage, search: term.trim() || undefined });
            setRows(result.rows);
            setTotal(result.total);
            setError(null);
        } catch (cause) {
            setError(messageOf(cause));
        }
    }, [config, page, term]);

    useEffect(() => {
        void refresh();
    }, [refresh, reload]);

    // Module-level references keep this effect's dependencies honest: a config object
    // built inline would re-fetch on every render.
    const load = config.loadCategories;
    useEffect(() => {
        let live = true;
        void Promise.all([listPaymentMethods(), load({ page: 1, perPage: 200 })])
            .then(([methods, page_]) => {
                if (!live) return;
                setAccounts(methods);
                setCategories(page_.rows);
            })
            .catch((cause) => {
                if (live) setError(messageOf(cause));
            });
        return () => {
            live = false;
        };
    }, [load, reload]);

    const categoryChoices = useMemo(
        () => categories.map((c) => ({ id: String(c.id), label: c.name })),
        [categories],
    );
    const accountChoices = useMemo(() => accountItems(accounts), [accounts]);

    const submit = async () => {
        setBusy(true);
        setError(null);
        try {
            await config.create({
                categoryId: Number(categoryId),
                paymentMethodId: Number(accountId),
                amount: toDecimal(amount),
                occurredAt,
                note: note.trim() === "" ? null : note.trim(),
            });
            setRecording(false);
            setAmount("");
            setNote("");
            setPage(1);
            setReload((n) => n + 1);
        } catch (cause) {
            setError(messageOf(cause));
        } finally {
            setBusy(false);
        }
    };

    const pages = Math.max(1, Math.ceil(total / perPage));

    return (
        <div className="flex flex-col gap-5 p-5">
            <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
                <div className="flex flex-col gap-1">
                    <h1 className="text-display-xs font-semibold text-primary">{config.title}</h1>
                    <p className="text-md text-tertiary">{config.blurb}</p>
                </div>
                <div className="flex w-full flex-col gap-3 md:w-auto md:flex-row">
                    <Input
                        aria-label={`Search ${config.title.toLowerCase()}`}
                        icon={SearchLg}
                        placeholder="Search reference"
                        value={term}
                        onChange={(value) => {
                            setPage(1);
                            setTerm(String(value));
                        }}
                        className="w-full md:w-72"
                    />
                    <Button size="md" iconLeading={Plus} onPress={() => setRecording(true)}>
                        {config.submitLabel}
                    </Button>
                </div>
            </div>

            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

            {categories.length === 0 && (
                <p className="rounded-lg bg-warning-secondary px-3 py-2 text-sm text-warning-primary">
                    No categories yet, so there is nothing to file this under. Add one first.
                </p>
            )}

            {rows.length === 0 ? (
                <p className="py-10 text-center text-md text-tertiary">
                    {term ? `Nothing matches that search.` : `Nothing recorded yet.`}
                </p>
            ) : (
                <TableCard.Root>
                    <TableCard.Header title={config.title} description={`${total} total`} />
                    <Table aria-label={config.title}>
                        <Table.Header>
                            <Table.Head label="Reference" />
                            <Table.Head label="Category" />
                            <Table.Head label="Account" />
                            <Table.Head label="Date" />
                            <Table.Head label="Amount" className="text-right" />
                            <Table.Head label="Note" />
                        </Table.Header>
                        <Table.Body>
                            {rows.map((row) => (
                                <Table.Row key={row.id} id={row.id}>
                                    <Table.Cell className="font-medium text-primary">
                                        {row.referenceNo}
                                    </Table.Cell>
                                    <Table.Cell>{row.categoryName}</Table.Cell>
                                    <Table.Cell>{row.paymentMethodName}</Table.Cell>
                                    <Table.Cell>{row.occurredAt}</Table.Cell>
                                    <Table.Cell className="text-right">{formatMoney(row.amount)}</Table.Cell>
                                    <Table.Cell className="text-tertiary">{row.note ?? "—"}</Table.Cell>
                                </Table.Row>
                            ))}
                        </Table.Body>
                    </Table>
                </TableCard.Root>
            )}

            {pages > 1 && (
                <div className="flex items-center justify-end gap-3">
                    <Button color="secondary" size="sm" isDisabled={page <= 1} onPress={() => setPage((n) => n - 1)}>
                        Previous
                    </Button>
                    <span className="text-sm text-tertiary">
                        {page} / {pages}
                    </span>
                    <Button color="secondary" size="sm" isDisabled={page >= pages} onPress={() => setPage((n) => n + 1)}>
                        Next
                    </Button>
                </div>
            )}

            {recording && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setRecording(false)}>
                    <Modal className="max-w-lg">
                        <Dialog className="p-6">
                            <form
                                className="flex flex-col gap-4"
                                onSubmit={(event) => {
                                    event.preventDefault();
                                    void submit();
                                }}
                            >
                                <h2 className="text-display-xs font-semibold text-primary">
                                    {config.submitLabel}
                                </h2>
                                <Select
                                    label="Category"
                                    aria-label="Category"
                                    items={categoryChoices}
                                    selectedKey={categoryId}
                                    onSelectionChange={(key) => {
                                        if (key !== null && key !== undefined) setCategoryId(String(key));
                                    }}
                                >
                                    {(choice) => (
                                        <Select.Item id={choice.id} textValue={choice.label}>
                                            {choice.label}
                                        </Select.Item>
                                    )}
                                </Select>
                                <Select
                                    label={`Account the money ${config.accountVerb}`}
                                    aria-label="Account"
                                    items={accountChoices}
                                    selectedKey={accountId}
                                    onSelectionChange={(key) => {
                                        if (key !== null && key !== undefined) setAccountId(String(key));
                                    }}
                                >
                                    {(choice) => (
                                        <Select.Item id={choice.id} textValue={choice.label}>
                                            {choice.label}
                                        </Select.Item>
                                    )}
                                </Select>
                                <Input label="Amount" value={amount} onChange={setAmount} />
                                <Input
                                    label="Date"
                                    type="date"
                                    value={occurredAt}
                                    onChange={(value) => setOccurredAt(String(value))}
                                />
                                <Input label="Note" value={note} onChange={setNote} />
                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setRecording(false)}>
                                        Cancel
                                    </Button>
                                    <Button
                                        type="submit"
                                        isLoading={busy}
                                        isDisabled={!categoryId || !accountId || amount.trim() === ""}
                                    >
                                        {config.submitLabel}
                                    </Button>
                                </div>
                            </form>
                        </Dialog>
                    </Modal>
                </ModalOverlay>
            )}
        </div>
    );
};