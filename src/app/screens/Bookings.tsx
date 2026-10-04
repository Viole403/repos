import { useCallback, useEffect, useState } from "react";
import { Plus, Trash01 } from "@untitledui/icons";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { Table, TableCard } from "@/components/application/table/table";
import { formatTimestamp } from "@/app/format";
import type { BookingInput, BookingView } from "@/app/ipc";
import {
    BOOKING_STATUSES,
    createBooking,
    deleteBooking,
    getBooking,
    listBookings,
    listCustomers,
    listUsers,
    updateBooking,
} from "@/app/ipc";

type Loaded =
    | { kind: "loading" }
    | { kind: "ready"; rows: BookingView[]; total: number }
    | { kind: "failed"; message: string };

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

const statusOptions: SelectItemType[] = [
    { id: "", label: "Any status" },
    ...BOOKING_STATUSES.map((s) => ({ id: s, label: s })),
];

const toLocalInput = (stamp: string): string => stamp.slice(0, 16);

/**
 * Customer appointments. A schedule, not an archive: soonest first, with the past
 * left behind rather than surfaced.
 */
export const Bookings = () => {
    const [state, setState] = useState<Loaded>({ kind: "loading" });
    const [reload, setReload] = useState(0);
    const [statusFilter, setStatusFilter] = useState("");
    const [formOpen, setFormOpen] = useState(false);
    const [editingId, setEditingId] = useState<number | null>(null);
    const [discarding, setDiscarding] = useState<BookingView | null>(null);
    const [formError, setFormError] = useState<string | null>(null);
    const [saving, setSaving] = useState(false);

    const [customerKey, setCustomerKey] = useState("");
    const [customers, setCustomers] = useState<SelectItemType[]>([]);
    const [sellerKey, setSellerKey] = useState("");
    const [sellers, setSellers] = useState<SelectItemType[]>([]);
    const [status, setStatus] = useState<string>("Booked");
    const [startAt, setStartAt] = useState("");
    const [endAt, setEndAt] = useState("");
    const [note, setNote] = useState("");

    const refresh = useCallback(() => setReload((n) => n + 1), []);

    useEffect(() => {
        let cancelled = false;
        setState({ kind: "loading" });

        listBookings(statusFilter === "" ? {} : { status: statusFilter }, { page: 1, perPage: 50 })
            .then((page) => {
                if (!cancelled) setState({ kind: "ready", rows: page.rows, total: page.total });
            })
            .catch((error: unknown) => {
                if (!cancelled) setState({ kind: "failed", message: messageOf(error) });
            });

        return () => {
            cancelled = true;
        };
    }, [reload, statusFilter]);

    const findCustomers = useCallback(async (term: string) => {
        try {
            const found = await listCustomers({ page: 1, perPage: 20, search: term || undefined });
            setCustomers(found.rows.map((row) => ({ id: String(row.id), label: row.name })));
        } catch {
            setCustomers([]);
        }
    }, []);

    const loadSellers = useCallback(async () => {
        try {
            const rows = await listUsers();
            setSellers(rows.map((row) => ({ id: String(row.id), label: row.name })));
        } catch {
            setSellers([]);
        }
    }, []);

    const openCreate = useCallback(() => {
        setEditingId(null);
        setCustomerKey("");
        setSellerKey("");
        setStatus("Booked");
        setStartAt("");
        setEndAt("");
        setNote("");
        setFormError(null);
        setFormOpen(true);
    }, []);

    const openEdit = useCallback(async (id: number) => {
        setFormError(null);
        try {
            const view = await getBooking(id);
            setEditingId(id);
            setCustomerKey(String(view.customerId));
            setCustomers([{ id: String(view.customerId), label: view.customerName ?? String(view.customerId) }]);
            setSellerKey(view.serviceSellerId === null ? "" : String(view.serviceSellerId));
            setStatus(view.status);
            setStartAt(toLocalInput(view.startAt));
            setEndAt(toLocalInput(view.endAt));
            setNote(view.note ?? "");
            setFormOpen(true);
        } catch (error) {
            setFormError(messageOf(error));
            setFormOpen(true);
        }
    }, []);

    const save = useCallback(async () => {
        if (customerKey === "" || startAt === "" || endAt === "") return;
        setSaving(true);
        setFormError(null);
        try {
            const input: BookingInput = {
                customerId: Number(customerKey),
                serviceSellerId: sellerKey === "" ? null : Number(sellerKey),
                status,
                startAt,
                endAt,
                note: note.trim() === "" ? null : note.trim(),
            };
            if (editingId === null) {
                await createBooking(input);
            } else {
                await updateBooking(editingId, input);
            }
            setFormOpen(false);
            refresh();
        } catch (error) {
            setFormError(messageOf(error));
        } finally {
            setSaving(false);
        }
    }, [customerKey, sellerKey, status, startAt, endAt, note, editingId, refresh]);

    const confirmDiscard = useCallback(async () => {
        if (discarding === null) return;
        const id = discarding.id;
        setDiscarding(null);
        try {
            await deleteBooking(id);
            refresh();
        } catch (error) {
            setState({ kind: "failed", message: messageOf(error) });
        }
    }, [discarding, refresh]);

    const sellerOptions: SelectItemType[] = [{ id: "", label: "No one assigned" }, ...sellers];

    return (
        <div className="flex flex-col gap-6 p-6 md:p-8">
            <TableCard.Root>
                <TableCard.Header
                    title="Bookings"
                    description={state.kind === "ready" ? `${state.total} appointment${state.total === 1 ? "" : "s"}` : undefined}
                    contentTrailing={
                        <Button size="sm" iconLeading={Plus} onPress={openCreate}>
                            New booking
                        </Button>
                    }
                />

                <div className="flex flex-col gap-3 px-6 pt-4 md:flex-row md:items-end">
                    <Select
                        label="Status"
                        items={statusOptions}
                        selectedKey={statusFilter}
                        onSelectionChange={(key) => setStatusFilter(String(key ?? ""))}
                        className="w-full md:w-44"
                    >
                        {(row) => (
                            <Select.Item id={row.id} textValue={row.label}>
                                {row.label}
                            </Select.Item>
                        )}
                    </Select>
                </div>

                {state.kind === "failed" ? (
                    <p className="px-6 py-12 text-center text-md text-error-primary">{state.message}</p>
                ) : state.kind === "loading" ? (
                    <p className="px-6 py-12 text-center text-md text-tertiary">Loading…</p>
                ) : state.rows.length === 0 ? (
                    <p className="px-6 py-12 text-center text-md text-tertiary">No bookings.</p>
                ) : (
                    <Table>
                        <Table.Header>
                            <Table.Head label="When" />
                            <Table.Head label="Customer" />
                            <Table.Head label="Staff" />
                            <Table.Head label="Status" />
                            <Table.Head label="" />
                        </Table.Header>
                        <Table.Body>
                            {state.rows.map((row) => (
                                <Table.Row key={row.id} id={row.id}>
                                    <Table.Cell className="font-medium text-primary">
                                        {formatTimestamp(row.startAt)}
                                        <span className="block text-sm font-normal text-tertiary">
                                            until {formatTimestamp(row.endAt)}
                                        </span>
                                    </Table.Cell>
                                    <Table.Cell>{row.customerName ?? "—"}</Table.Cell>
                                    <Table.Cell>{row.serviceSellerName ?? "—"}</Table.Cell>
                                    <Table.Cell className="text-tertiary">{row.status}</Table.Cell>
                                    <Table.Cell>
                                        <div className="flex justify-end gap-1">
                                            <Button size="sm" color="secondary" onPress={() => void openEdit(row.id)}>
                                                Edit
                                            </Button>
                                            <Button
                                                size="sm"
                                                color="tertiary"
                                                iconLeading={Trash01}
                                                aria-label={`Cancel booking for ${row.customerName ?? "customer"}`}
                                                onPress={() => setDiscarding(row)}
                                            />
                                        </div>
                                    </Table.Cell>
                                </Table.Row>
                            ))}
                        </Table.Body>
                    </Table>
                )}
            </TableCard.Root>

            {formOpen && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setFormOpen(false)}>
                    <Modal className="max-w-xl">
                        <Dialog className="max-h-[85dvh] overflow-y-auto p-6">
                            <div className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">
                                    {editingId === null ? "New booking" : "Edit booking"}
                                </h2>

                                {formError !== null && (
                                    <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">
                                        {formError}
                                    </p>
                                )}

                                <Select
                                    label="Customer"
                                    items={customers}
                                    selectedKey={customerKey}
                                    onOpenChange={(open) => open && void findCustomers("")}
                                    onSelectionChange={(key) => setCustomerKey(String(key ?? ""))}
                                    isRequired
                                >
                                    {(row) => (
                                        <Select.Item id={row.id} textValue={row.label}>
                                            {row.label}
                                        </Select.Item>
                                    )}
                                </Select>

                                <Select
                                    label="Staff member"
                                    items={sellerOptions}
                                    selectedKey={sellerKey}
                                    onOpenChange={(open) => open && void loadSellers()}
                                    onSelectionChange={(key) => setSellerKey(String(key ?? ""))}
                                >
                                    {(row) => (
                                        <Select.Item id={row.id} textValue={row.label}>
                                            {row.label}
                                        </Select.Item>
                                    )}
                                </Select>

                                <Select
                                    label="Status"
                                    items={BOOKING_STATUSES.map((s) => ({ id: s, label: s }))}
                                    selectedKey={status}
                                    onSelectionChange={(key) => setStatus(String(key ?? "Booked"))}
                                >
                                    {(row) => (
                                        <Select.Item id={row.id} textValue={row.label}>
                                            {row.label}
                                        </Select.Item>
                                    )}
                                </Select>

                                <div className="grid grid-cols-2 gap-3">
                                    <Input label="Starts" type="datetime-local" value={startAt} onChange={setStartAt} isRequired />
                                    <Input label="Ends" type="datetime-local" value={endAt} onChange={setEndAt} isRequired />
                                </div>

                                <Input label="Note" value={note} onChange={setNote} />

                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setFormOpen(false)}>
                                        Cancel
                                    </Button>
                                    <Button
                                        onPress={() => void save()}
                                        isLoading={saving}
                                        isDisabled={customerKey === "" || startAt === "" || endAt === ""}
                                    >
                                        {editingId === null ? "Create booking" : "Save changes"}
                                    </Button>
                                </div>
                            </div>
                        </Dialog>
                    </Modal>
                </ModalOverlay>
            )}

            {discarding && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setDiscarding(null)}>
                    <Modal className="max-w-md">
                        <Dialog className="p-6">
                            <div className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">Cancel this booking?</h2>
                                <p className="text-md text-secondary">
                                    It leaves this list. The row is kept, not erased — set its
                                    status to Cancelled instead if the slot should stay visible.
                                </p>
                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setDiscarding(null)}>
                                        Keep it
                                    </Button>
                                    <Button color="primary-destructive" onPress={confirmDiscard}>
                                        Cancel booking
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
