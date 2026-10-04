import { useCallback, useEffect, useState } from "react";
import { Plus, SearchLg, Trash01 } from "@untitledui/icons";
import { Table, TableCard } from "@/components/application/table/table";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { decIsNegative } from "@/app/decimal";
import { formatMoney, formatTimestamp, toDecimal } from "@/app/format";
import { createCustomer, deleteCustomer, listCustomers, recordCustomerReceipt } from "@/app/ipc";
import type { CustomerInput, CustomerView } from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

const blank: CustomerInput = {
    name: "",
    code: null,
    email: null,
    phone: null,
    address: null,
    city: null,
    country: null,
    zip: null,
    taxNumber: null,
    creditLimit: "0",
    note: null,
};

/** A customer form is long enough that every field is worth a line, not a grid cell. */
const Field = ({
    label,
    value,
    onChange,
    type,
}: {
    label: string;
    value: string;
    onChange: (next: string) => void;
    type?: string;
}) => (
    <Input label={label} type={type} value={value} onChange={onChange} />
);

export const Customers = () => {
    const [rows, setRows] = useState<CustomerView[]>([]);
    const [total, setTotal] = useState(0);
    const [page, setPage] = useState(1);
    const [term, setTerm] = useState("");
    const [error, setError] = useState<string | null>(null);
    const [busy, setBusy] = useState(false);
    const [reload, setReload] = useState(0);

    const [creating, setCreating] = useState(false);
    const [form, setForm] = useState<CustomerInput>(blank);
    const [pendingDelete, setPendingDelete] = useState<CustomerView | null>(null);
    const [receiving, setReceiving] = useState<CustomerView | null>(null);
    const [received, setReceived] = useState("");

    const perPage = 20;

    const refresh = useCallback(async () => {
        try {
            const page_ = await listCustomers({
                page,
                perPage,
                search: term.trim() || undefined,
            });
            setRows(page_.rows);
            setTotal(page_.total);
            setError(null);
        } catch (cause) {
            setError(messageOf(cause));
        }
    }, [page, term]);

    useEffect(() => {
        void refresh();
    }, [refresh, reload]);

    const set = <K extends keyof CustomerInput>(key: K) => (value: CustomerInput[K]) =>
        setForm((current) => ({ ...current, [key]: value }));

    const save = async () => {
        setBusy(true);
        setError(null);
        try {
            await createCustomer({
                ...form,
                name: form.name.trim(),
                code: form.code?.trim() || null,
                email: form.email?.trim() || null,
                phone: form.phone?.trim() || null,
                creditLimit: toDecimal(form.creditLimit),
            });
            setCreating(false);
            setForm(blank);
            setPage(1);
            setReload((n) => n + 1);
        } catch (cause) {
            setError(messageOf(cause));
        } finally {
            setBusy(false);
        }
    };

    const confirmDelete = async () => {
        if (!pendingDelete) return;
        const id = pendingDelete.id;
        setPendingDelete(null);
        setError(null);
        try {
            await deleteCustomer(id);
            setReload((n) => n + 1);
        } catch (cause) {
            setError(messageOf(cause));
        }
    };

    const recordReceipt = async () => {
        if (!receiving) return;
        setBusy(true);
        setError(null);
        try {
            await recordCustomerReceipt(receiving.id, { amount: toDecimal(received) });
            setReceiving(null);
            setReceived("");
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
                    <h1 className="text-display-xs font-semibold text-primary">Customers</h1>
                    <p className="text-md text-tertiary">
                        Balance is completed sales less payments. Nothing stores it.
                    </p>
                </div>
                <div className="flex w-full flex-col gap-3 md:w-auto md:flex-row">
                    <Input
                        aria-label="Search customers"
                        icon={SearchLg}
                        placeholder="Search name, code or phone"
                        value={term}
                        onChange={(value) => {
                            setPage(1);
                            setTerm(String(value));
                        }}
                        className="w-full md:w-72"
                    />
                    <Button size="md" iconLeading={Plus} onPress={() => setCreating(true)}>
                        New customer
                    </Button>
                </div>
            </div>

            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

            {rows.length === 0 ? (
                <p className="py-10 text-center text-md text-tertiary">
                    {term ? "No customers match that search." : "No customers yet."}
                </p>
            ) : (
                <TableCard.Root>
                    <TableCard.Header title="Customers" description={`${total} total`} />
                    <Table aria-label="Customers">
                        <Table.Header>
                            <Table.Head label="Name" />
                            <Table.Head label="Code" />
                            <Table.Head label="Phone" />
                            <Table.Head label="Balance" />
                            <Table.Head label="Credit left" />
                            <Table.Head label="Points" />
                            <Table.Head label="Added" />
                            <Table.Head label="" className="w-28" />
                        </Table.Header>
                        <Table.Body>
                            {rows.map((row) => (
                                <Table.Row key={row.id} id={row.id}>
                                    <Table.Cell className="font-medium text-primary">{row.name}</Table.Cell>
                                    <Table.Cell>{row.code ?? "—"}</Table.Cell>
                                    <Table.Cell>{row.phone ?? "—"}</Table.Cell>
                                    <Table.Cell>{formatMoney(row.balance)}</Table.Cell>
                                    {/* Over the limit reads as a negative allowance, which is
                                        what it is — clamping at zero would hide the breach. */}
                                    <Table.Cell
                                        className={decIsNegative(row.creditAvailable) ? "text-error-primary" : undefined}
                                    >
                                        {formatMoney(row.creditAvailable)}
                                    </Table.Cell>
                                    {/* Derived from the points ledger, so it moves with every
                                        earn and redeem rather than whatever was typed last. */}
                                    <Table.Cell className="tabular-nums">{row.loyaltyPoints}</Table.Cell>
                                    <Table.Cell className="text-tertiary">{formatTimestamp(row.createdAt)}</Table.Cell>
                                    <Table.Cell>
                                        <div className="flex items-center gap-2">
                                            <Button size="sm" color="secondary" onPress={() => setReceiving(row)}>
                                                Receive
                                            </Button>
                                            <Button
                                                size="sm"
                                                color="tertiary"
                                                iconLeading={Trash01}
                                                aria-label={`Delete ${row.name}`}
                                                onPress={() => setPendingDelete(row)}
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
                    <Button
                        color="secondary"
                        size="sm"
                        isDisabled={page <= 1}
                        onPress={() => setPage((n) => n - 1)}
                    >
                        Previous
                    </Button>
                    <span className="text-sm text-tertiary">
                        {page} / {pages}
                    </span>
                    <Button
                        color="secondary"
                        size="sm"
                        isDisabled={page >= pages}
                        onPress={() => setPage((n) => n + 1)}
                    >
                        Next
                    </Button>
                </div>
            )}

            {creating && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setCreating(false)}>
                    <Modal className="max-w-2xl">
                        <Dialog className="max-h-[85dvh] overflow-y-auto p-6">
                            <form
                                className="flex flex-col gap-4"
                                onSubmit={(event) => {
                                    event.preventDefault();
                                    void save();
                                }}
                            >
                                <h2 className="text-display-xs font-semibold text-primary">New customer</h2>
                                <Field label="Name" value={form.name ?? ""} onChange={set("name")} />
                                <Field label="Member code" value={form.code ?? ""} onChange={set("code")} />
                                <Field label="Phone" value={form.phone ?? ""} onChange={set("phone")} />
                                <Field label="Email" type="email" value={form.email ?? ""} onChange={set("email")} />
                                <Field label="Address" value={form.address ?? ""} onChange={set("address")} />
                                <div className="grid grid-cols-2 gap-3">
                                    <Field label="City" value={form.city ?? ""} onChange={set("city")} />
                                    <Field label="Postal code" value={form.zip ?? ""} onChange={set("zip")} />
                                </div>
                                <div className="grid grid-cols-2 gap-3">
                                    <Field
                                        label="Credit limit"
                                        value={form.creditLimit}
                                        onChange={set("creditLimit")}
                                    />
                                </div>
                                <Field label="Tax number" value={form.taxNumber ?? ""} onChange={set("taxNumber")} />
                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setCreating(false)}>
                                        Cancel
                                    </Button>
                                    <Button type="submit" isLoading={busy} isDisabled={!form.name.trim()}>
                                        Create customer
                                    </Button>
                                </div>
                            </form>
                        </Dialog>
                    </Modal>
                </ModalOverlay>
            )}

            {receiving && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setReceiving(null)}>
                    <Modal className="max-w-md">
                        <Dialog className="p-6">
                            <form
                                className="flex flex-col gap-4"
                                onSubmit={(event) => {
                                    event.preventDefault();
                                    void recordReceipt();
                                }}
                            >
                                <h2 className="text-display-xs font-semibold text-primary">
                                    Receive from {receiving.name}
                                </h2>
                                <p className="text-md text-secondary">
                                    Currently owes {formatMoney(receiving.balance)}.
                                </p>
                                <Field label="Amount" value={received} onChange={setReceived} />
                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setReceiving(null)}>
                                        Cancel
                                    </Button>
                                    <Button type="submit" isLoading={busy} isDisabled={!received.trim()}>
                                        Record payment
                                    </Button>
                                </div>
                            </form>
                        </Dialog>
                    </Modal>
                </ModalOverlay>
            )}

            {pendingDelete && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setPendingDelete(null)}>
                    <Modal className="max-w-md">
                        <Dialog className="p-6">
                            <div className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">
                                    Delete {pendingDelete.name}?
                                </h2>
                                <p className="text-md text-secondary">
                                    Sales keep the name. A customer with recorded payments cannot be deleted at all.
                                </p>
                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setPendingDelete(null)}>
                                        Cancel
                                    </Button>
                                    <Button color="primary-destructive" onPress={() => void confirmDelete()}>
                                        Delete
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