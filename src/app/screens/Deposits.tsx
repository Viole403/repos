import { useCallback, useEffect, useMemo, useState } from "react";
import { Plus, SearchLg } from "@untitledui/icons";
import { Table, TableCard } from "@/components/application/table/table";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { formatMoney, toDecimal } from "@/app/format";
import { createDepositWithdraw, listDepositWithdraws, listPaymentMethods } from "@/app/ipc";
import type { DepositKind, DepositWithdraw, PaymentMethod } from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

const KINDS: { id: DepositKind; label: string }[] = [
    { id: "Deposit", label: "Deposit (into the account)" },
    { id: "Withdraw", label: "Withdraw (out of the account)" },
];

const today = () => new Date().toISOString().slice(0, 10);

export const Deposits = () => {
    const [rows, setRows] = useState<DepositWithdraw[]>([]);
    const [total, setTotal] = useState(0);
    const [page, setPage] = useState(1);
    const [term, setTerm] = useState("");
    const [error, setError] = useState<string | null>(null);
    const [busy, setBusy] = useState(false);
    const [reload, setReload] = useState(0);

    const [accounts, setAccounts] = useState<PaymentMethod[]>([]);
    const [moving, setMoving] = useState(false);
    const [kind, setKind] = useState<DepositKind>("Deposit");
    const [accountId, setAccountId] = useState("");
    const [amount, setAmount] = useState("");
    const [occurredAt, setOccurredAt] = useState(today);
    const [note, setNote] = useState("");

    const perPage = 20;

    const refresh = useCallback(async () => {
        try {
            const result = await listDepositWithdraws({ page, perPage, search: term.trim() || undefined });
            setRows(result.rows);
            setTotal(result.total);
            setError(null);
        } catch (cause) {
            setError(messageOf(cause));
        }
    }, [page, term]);

    useEffect(() => {
        void refresh();
    }, [refresh, reload]);

    useEffect(() => {
        let live = true;
        void listPaymentMethods()
            .then((methods) => {
                if (live) setAccounts(methods);
            })
            .catch((cause) => {
                if (live) setError(messageOf(cause));
            });
        return () => {
            live = false;
        };
    }, []);

    const accountChoices = useMemo<SelectItemType[]>(
        () => accounts.map((account) => ({ id: String(account.id), label: account.name })),
        [accounts],
    );

    const submit = async () => {
        setBusy(true);
        setError(null);
        try {
            await createDepositWithdraw({
                kind,
                paymentMethodId: Number(accountId),
                amount: toDecimal(amount),
                occurredAt,
                note: note.trim() === "" ? null : note.trim(),
            });
            setMoving(false);
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
                    <h1 className="text-display-xs font-semibold text-primary">Deposits &amp; withdrawals</h1>
                    <p className="text-md text-tertiary">
                        The owner moving money into or out of a tender. A float top-up, cash taken to the
                        bank — it changes what the drawer holds without changing what the shop earned.
                    </p>
                </div>
                <div className="flex w-full flex-col gap-3 md:w-auto md:flex-row">
                    <Input
                        aria-label="Search deposits and withdrawals"
                        icon={SearchLg}
                        placeholder="Search reference"
                        value={term}
                        onChange={(value) => {
                            setPage(1);
                            setTerm(String(value));
                        }}
                        className="w-full md:w-72"
                    />
                    <Button size="md" iconLeading={Plus} onPress={() => setMoving(true)}>
                        Move money
                    </Button>
                </div>
            </div>

            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

            {rows.length === 0 ? (
                <p className="py-10 text-center text-md text-tertiary">
                    {term ? "Nothing matches that search." : "Nothing moved yet."}
                </p>
            ) : (
                <TableCard.Root>
                    <TableCard.Header title="Deposits & withdrawals" description={`${total} total`} />
                    <Table aria-label="Deposits and withdrawals">
                        <Table.Header>
                            <Table.Head label="Reference" />
                            <Table.Head label="Direction" />
                            <Table.Head label="Account" />
                            <Table.Head label="Date" />
                            <Table.Head label="Amount" className="text-right" />
                            <Table.Head label="Note" />
                        </Table.Header>
                        <Table.Body>
                            {rows.map((row) => (
                                <Table.Row key={row.id} id={row.id}>
                                    <Table.Cell className="font-medium text-primary">{row.referenceNo}</Table.Cell>
                                    <Table.Cell>{row.kind === "Deposit" ? "Deposit" : "Withdraw"}</Table.Cell>
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

            {moving && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setMoving(false)}>
                    <Modal className="max-w-lg">
                        <Dialog className="p-6">
                            <form
                                className="flex flex-col gap-4"
                                onSubmit={(event) => {
                                    event.preventDefault();
                                    void submit();
                                }}
                            >
                                <h2 className="text-display-xs font-semibold text-primary">Move money</h2>
                                <Select
                                    label="Direction"
                                    aria-label="Direction"
                                    items={KINDS}
                                    selectedKey={kind}
                                    onSelectionChange={(key) => {
                                        if (key !== null && key !== undefined) setKind(key as DepositKind);
                                    }}
                                >
                                    {(choice) => (
                                        <Select.Item id={choice.id} textValue={choice.label}>
                                            {choice.label}
                                        </Select.Item>
                                    )}
                                </Select>
                                <Select
                                    label="Account"
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
                                    <Button color="secondary" onPress={() => setMoving(false)}>
                                        Cancel
                                    </Button>
                                    <Button type="submit" isLoading={busy} isDisabled={!accountId || amount.trim() === ""}>
                                        Move money
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
