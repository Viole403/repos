import { useCallback, useEffect, useMemo, useState } from "react";
import { Play, Plus, SearchLg, Trash01 } from "@untitledui/icons";
import { Table, TableCard } from "@/components/application/table/table";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { formatMoney, toDecimal } from "@/app/format";
import {
    createRecurringExpense,
    updateRecurringExpense,
    deleteRecurringExpense,
    listExpenseCategories,
    listPaymentMethods,
    listRecurringExpenses,
    postDueRecurringExpenses,
} from "@/app/ipc";
import type { AccountingCategory, PaymentMethod, RecurringExpense, Rotation } from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

const today = () => new Date().toISOString().slice(0, 10);

const ROTATIONS: { id: Rotation; label: string }[] = [
    { id: "Daily", label: "Daily" },
    { id: "Weekly", label: "Weekly" },
    { id: "BiWeekly", label: "Every two weeks" },
    { id: "Monthly", label: "Monthly" },
    { id: "Quarterly", label: "Quarterly" },
    { id: "Yearly", label: "Yearly" },
];

export const RecurringExpenses = () => {
    const [rows, setRows] = useState<RecurringExpense[]>([]);
    const [total, setTotal] = useState(0);
    const [page, setPage] = useState(1);
    const [term, setTerm] = useState("");
    const [error, setError] = useState<string | null>(null);
    const [notice, setNotice] = useState<string | null>(null);
    const [busy, setBusy] = useState(false);
    const [reload, setReload] = useState(0);

    const [accounts, setAccounts] = useState<PaymentMethod[]>([]);
    const [categories, setCategories] = useState<AccountingCategory[]>([]);

    const [editing, setEditing] = useState(false);
    const [form, setForm] = useState({
        id: 0,
        name: "",
        amount: "",
        categoryId: "",
        accountId: "",
        rotation: "Monthly" as Rotation,
        startsOn: today(),
        endsOn: "",
        note: "",
    });
    const [pendingStop, setPendingStop] = useState<RecurringExpense | null>(null);

    const perPage = 20;

    const refresh = useCallback(async () => {
        try {
            const result = await listRecurringExpenses({ page, perPage, search: term.trim() || undefined });
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
        void Promise.all([listPaymentMethods(), listExpenseCategories({ page: 1, perPage: 200 })])
            .then(([methods, cats]) => {
                if (!live) return;
                setAccounts(methods);
                setCategories(cats.rows);
            })
            .catch((cause) => {
                if (live) setError(messageOf(cause));
            });
        return () => {
            live = false;
        };
    }, [reload]);

    const accountChoices = useMemo<SelectItemType[]>(
        () => accounts.map((a) => ({ id: String(a.id), label: a.name })),
        [accounts],
    );
    const categoryChoices = useMemo<SelectItemType[]>(
        () => categories.map((c) => ({ id: String(c.id), label: c.name })),
        [categories],
    );

    const openNew = () => {
        setForm({
            id: 0,
            name: "",
            amount: "",
            categoryId: String(categoryChoices[0]?.id ?? ""),
            accountId: String(accountChoices[0]?.id ?? ""),
            rotation: "Monthly",
            startsOn: today(),
            endsOn: "",
            note: "",
        });
        setEditing(true);
    };

    const openEdit = (row: RecurringExpense) => {
        setForm({
            id: row.id,
            name: row.name,
            amount: row.amount,
            categoryId: String(row.expenseCategoryId),
            accountId: String(row.paymentMethodId),
            rotation: row.rotation,
            startsOn: row.startsOn,
            endsOn: row.endsOn ?? "",
            note: row.note ?? "",
        });
        setEditing(true);
    };

    const submit = async () => {
        setBusy(true);
        setError(null);
        const input = {
            expenseCategoryId: Number(form.categoryId),
            name: form.name.trim(),
            amount: toDecimal(form.amount),
            paymentMethodId: Number(form.accountId),
            rotation: form.rotation,
            startsOn: form.startsOn,
            endsOn: form.endsOn === "" ? null : form.endsOn,
            note: form.note.trim() === "" ? null : form.note.trim(),
            postNow: form.id === 0 ? false : undefined,
        };
        try {
            if (form.id === 0) {
                await createRecurringExpense(input);
            } else {
                await updateRecurringExpense(form.id, { ...input, postNow: undefined });
            }
            setEditing(false);
            setReload((n) => n + 1);
        } catch (cause) {
            setError(messageOf(cause));
        } finally {
            setBusy(false);
        }
    };

    const runSchedules = async () => {
        setBusy(true);
        setError(null);
        setNotice(null);
        try {
            const run = await postDueRecurringExpenses();
            setNotice(
                run.failed.length === 0
                    ? `Posted ${run.posted} ${run.posted === 1 ? "entry" : "entries"}.`
                    : `Posted ${run.posted}, but these could not be posted: ${run.failed.join(", ")}.`,
            );
            setReload((n) => n + 1);
        } catch (cause) {
            setError(messageOf(cause));
        } finally {
            setBusy(false);
        }
    };

    const stop = async () => {
        if (!pendingStop) return;
        const id = pendingStop.id;
        setPendingStop(null);
        try {
            await deleteRecurringExpense(id);
            setReload((n) => n + 1);
        } catch (cause) {
            setError(messageOf(cause));
        }
    };

    const pages = Math.max(1, Math.ceil(total / perPage));

    return (
        <div className="flex flex-col gap-5 p-5">
            <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
                <div className="flex flex-col gap-1">
                    <h1 className="text-display-xs font-semibold text-primary">Recurring expenses</h1>
                    <p className="text-md text-tertiary">
                        A schedule proposes; the expense posts. What it has already written stays in the
                        books when the schedule stops.
                    </p>
                </div>
                <div className="flex w-full flex-col gap-3 md:w-auto md:flex-row">
                    <Input
                        aria-label="Search recurring expenses"
                        icon={SearchLg}
                        placeholder="Search name or rotation"
                        value={term}
                        onChange={(value) => {
                            setPage(1);
                            setTerm(String(value));
                        }}
                        className="w-full md:w-64"
                    />
                    <Button size="md" color="secondary" iconLeading={Play} isLoading={busy} onPress={() => void runSchedules()}>
                        Post what's due
                    </Button>
                    <Button size="md" iconLeading={Plus} onPress={openNew}>
                        New schedule
                    </Button>
                </div>
            </div>

            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}
            {notice && <p className="rounded-lg bg-success-secondary px-3 py-2 text-sm text-success-primary">{notice}</p>}

            {rows.length === 0 ? (
                <p className="py-10 text-center text-md text-tertiary">
                    {term ? "Nothing matches that search." : "No schedules yet."}
                </p>
            ) : (
                <TableCard.Root>
                    <TableCard.Header title="Schedules" description={`${total} total`} />
                    <Table aria-label="Recurring expenses">
                        <Table.Header>
                            <Table.Head label="Name" />
                            <Table.Head label="Category" />
                            <Table.Head label="Repeats" />
                            <Table.Head label="Account" />
                            <Table.Head label="Next due" />
                            <Table.Head label="Posted" className="text-right" />
                            <Table.Head label="Amount" className="text-right" />
                            <Table.Head label="" className="w-28" />
                        </Table.Header>
                        <Table.Body>
                            {rows.map((row) => (
                                <Table.Row key={row.id} id={row.id}>
                                    <Table.Cell className="font-medium text-primary">{row.name}</Table.Cell>
                                    <Table.Cell>{row.expenseCategoryName}</Table.Cell>
                                    <Table.Cell>{ROTATIONS.find((r) => r.id === row.rotation)?.label ?? row.rotation}</Table.Cell>
                                    <Table.Cell>{row.paymentMethodName}</Table.Cell>
                                    <Table.Cell className="text-tertiary">
                                        {row.nextDueOn ?? (row.endsOn ? "ended" : "—")}
                                    </Table.Cell>
                                    <Table.Cell className="text-right">{row.postedCount}</Table.Cell>
                                    <Table.Cell className="text-right">{formatMoney(row.amount)}</Table.Cell>
                                    <Table.Cell>
                                        <div className="flex items-center gap-2">
                                            <Button size="sm" color="secondary" onPress={() => openEdit(row)}>
                                                Edit
                                            </Button>
                                            <Button
                                                size="sm"
                                                color="tertiary"
                                                iconLeading={Trash01}
                                                aria-label={`Stop ${row.name}`}
                                                onPress={() => setPendingStop(row)}
                                            />
                                        </div>
                                    </Table.Cell>
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

            {editing && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setEditing(false)}>
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
                                    {form.id === 0 ? "New schedule" : "Edit schedule"}
                                </h2>
                                <Input label="What it is for" value={form.name} onChange={(name) => setForm({ ...form, name })} />
                                <Select
                                    label="Category"
                                    aria-label="Category"
                                    items={categoryChoices}
                                    selectedKey={form.categoryId}
                                    onSelectionChange={(key) => {
                                        if (key !== null && key !== undefined) setForm({ ...form, categoryId: String(key) });
                                    }}
                                >
                                    {(choice) => (
                                        <Select.Item id={choice.id} textValue={choice.label}>
                                            {choice.label}
                                        </Select.Item>
                                    )}
                                </Select>
                                <Select
                                    label="Account the money will leave"
                                    aria-label="Account"
                                    items={accountChoices}
                                    selectedKey={form.accountId}
                                    onSelectionChange={(key) => {
                                        if (key !== null && key !== undefined) setForm({ ...form, accountId: String(key) });
                                    }}
                                >
                                    {(choice) => (
                                        <Select.Item id={choice.id} textValue={choice.label}>
                                            {choice.label}
                                        </Select.Item>
                                    )}
                                </Select>
                                <Input label="Amount" value={form.amount} onChange={(amount) => setForm({ ...form, amount })} />
                                <Select
                                    label="Repeats"
                                    aria-label="Repeats"
                                    items={ROTATIONS}
                                    selectedKey={form.rotation}
                                    onSelectionChange={(key) => {
                                        if (key !== null && key !== undefined) setForm({ ...form, rotation: key as Rotation });
                                    }}
                                >
                                    {(choice) => (
                                        <Select.Item id={choice.id} textValue={choice.label}>
                                            {choice.label}
                                        </Select.Item>
                                    )}
                                </Select>
                                <Input
                                    label="Starts on"
                                    type="date"
                                    value={form.startsOn}
                                    onChange={(value) => setForm({ ...form, startsOn: String(value) })}
                                />
                                <Input
                                    label="Ends on (blank means it never does)"
                                    type="date"
                                    value={form.endsOn}
                                    onChange={(value) => setForm({ ...form, endsOn: String(value) })}
                                />
                                <Input label="Note" value={form.note} onChange={(note) => setForm({ ...form, note })} />
                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setEditing(false)}>
                                        Cancel
                                    </Button>
                                    <Button
                                        type="submit"
                                        isLoading={busy}
                                        isDisabled={form.name.trim() === "" || form.amount.trim() === ""}
                                    >
                                        Save
                                    </Button>
                                </div>
                            </form>
                        </Dialog>
                    </Modal>
                </ModalOverlay>
            )}

            {pendingStop && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setPendingStop(null)}>
                    <Modal className="max-w-md">
                        <Dialog className="p-6">
                            <div className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">
                                    Stop {pendingStop.name}?
                                </h2>
                                <p className="text-md text-secondary">
                                    It stops posting. What it has already posted stays in the books —{" "}
                                    {pendingStop.postedCount} {pendingStop.postedCount === 1 ? "entry" : "entries"} so far.
                                </p>
                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setPendingStop(null)}>
                                        Cancel
                                    </Button>
                                    <Button color="tertiary" onPress={() => void stop()}>
                                        Stop it
                                    </Button>
                                </div>
                            </div>
                        </Dialog>
                    </Modal>
                </ModalOverlay>
            )}
        </div>
    );
};
