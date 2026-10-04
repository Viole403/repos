import { useCallback, useEffect, useState } from "react";
import { ArrowRight, Plus } from "@untitledui/icons";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { Table, TableCard } from "@/components/application/table/table";
import { formatMoney, formatTimestamp, toDecimal } from "@/app/format";
import type { CreditNoteSummary, CreditNoteView } from "@/app/ipc";
import {
    applyCreditNote,
    getCreditNote,
    issueCreditNote,
    listCreditNotes,
    listCustomers,
} from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

const normalizeMoney = (raw: string): string => raw.replace(/\./g, "").replace(",", ".");

/**
 * Store credit: a return becomes a promise against future sales instead of cash
 * back. Issuing moves nothing; spending posts a receipt, so the customer
 * balance moves with it.
 */
export const CreditNotes = () => {
    const [rows, setRows] = useState<CreditNoteSummary[]>([]);
    const [total, setTotal] = useState(0);
    const [page, setPage] = useState(1);
    const [error, setError] = useState<string | null>(null);

    const [formOpen, setFormOpen] = useState(false);
    const [formError, setFormError] = useState<string | null>(null);
    const [customers, setCustomers] = useState<SelectItemType[]>([]);
    const [customerKey, setCustomerKey] = useState("");
    const [amount, setAmount] = useState("");
    const [note, setNote] = useState("");

    const [detail, setDetail] = useState<CreditNoteView | null>(null);
    const [detailError, setDetailError] = useState<string | null>(null);
    const [spendAmount, setSpendAmount] = useState("");

    const perPage = 20;

    const refresh = useCallback(async () => {
        try {
            const found = await listCreditNotes(null, { page, perPage });
            setRows(found.rows);
            setTotal(found.total);
            setError(null);
        } catch (cause) {
            setError(messageOf(cause));
        }
    }, [page]);

    useEffect(() => {
        void refresh();
    }, [refresh]);

    const findCustomers = useCallback(async (term: string) => {
        try {
            const found = await listCustomers({ page: 1, perPage: 20, search: term || undefined });
            setCustomers(found.rows.map((row) => ({ id: String(row.id), label: row.name })));
        } catch {
            setCustomers([]);
        }
    }, []);

    const openCreate = useCallback(() => {
        setCustomerKey("");
        setAmount("");
        setNote("");
        setFormError(null);
        setFormOpen(true);
    }, []);

    const submit = useCallback(async () => {
        setFormError(null);
        try {
            const view = await issueCreditNote(
                Number(customerKey),
                toDecimal(normalizeMoney(amount.trim() === "" ? "0" : amount)),
                null,
                note.trim() === "" ? null : note.trim(),
            );
            setFormOpen(false);
            setDetail(view);
            setDetailError(null);
            setSpendAmount("");
            void refresh();
        } catch (cause) {
            setFormError(messageOf(cause));
        }
    }, [customerKey, amount, note, refresh]);

    const openDetail = useCallback(async (id: number) => {
        setDetailError(null);
        try {
            setDetail(await getCreditNote(id));
            setSpendAmount("");
        } catch (cause) {
            setDetailError(messageOf(cause));
        }
    }, []);

    const spend = useCallback(async () => {
        if (detail === null) return;
        setDetailError(null);
        try {
            const raw = spendAmount.trim();
            setDetail(
                await applyCreditNote(
                    detail.note.id,
                    toDecimal(normalizeMoney(raw === "" ? detail.remaining : raw)),
                ),
            );
            setSpendAmount("");
            void refresh();
        } catch (cause) {
            setDetailError(messageOf(cause));
        }
    }, [detail, spendAmount, refresh]);

    const pages = Math.max(1, Math.ceil(total / perPage));

    return (
        <div className="flex flex-col gap-5 p-5">
            <div className="flex flex-col gap-1">
                <h1 className="text-display-xs font-semibold text-primary">Credit notes</h1>
                <p className="text-md text-tertiary">Store credit, spent against future sales.</p>
            </div>

            <div>
                <Button color="primary" iconLeading={Plus} onPress={openCreate}>
                    Issue credit note
                </Button>
            </div>

            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

            {rows.length === 0 ? (
                <p className="py-10 text-center text-md text-tertiary">No credit notes yet.</p>
            ) : (
                <TableCard.Root>
                    <TableCard.Header title="Credit notes" description={`${total} total`} />
                    <Table aria-label="Credit notes">
                        <Table.Header>
                            <Table.Head label="Note" />
                            <Table.Head label="Customer" />
                            <Table.Head label="Amount" />
                            <Table.Head label="Spent" />
                            <Table.Head label="Left" />
                            <Table.Head label="" className="w-12" />
                        </Table.Header>
                        <Table.Body>
                            {rows.map((row) => (
                                <Table.Row key={row.id} id={row.id}>
                                    <Table.Cell className="font-medium tabular-nums text-primary">
                                        {row.creditNo}
                                    </Table.Cell>
                                    <Table.Cell>{row.customerName ?? "—"}</Table.Cell>
                                    <Table.Cell className="tabular-nums">{formatMoney(row.amount)}</Table.Cell>
                                    <Table.Cell className="tabular-nums">
                                        {formatMoney(row.appliedTotal)}
                                    </Table.Cell>
                                    <Table.Cell className="tabular-nums">{formatMoney(row.remaining)}</Table.Cell>
                                    <Table.Cell>
                                        <Button
                                            size="sm"
                                            color="tertiary"
                                            iconLeading={ArrowRight}
                                            aria-label={`Open ${row.creditNo}`}
                                            onPress={() => void openDetail(row.id)}
                                        />
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

            {formOpen && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setFormOpen(false)}>
                    <Modal className="max-w-2xl">
                        <Dialog className="max-h-[85dvh] overflow-y-auto p-6">
                            <div className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">Issue credit note</h2>

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

                                <div className="grid grid-cols-2 gap-3">
                                    <Input label="Amount" value={amount} onChange={setAmount} isRequired />
                                    <Input label="Note" value={note} onChange={setNote} />
                                </div>

                                <p className="text-sm text-tertiary">
                                    Issuing is a promise, not a payment — the customer balance moves
                                    when the credit is spent, not now.
                                </p>

                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setFormOpen(false)}>
                                        Cancel
                                    </Button>
                                    <Button
                                        color="primary"
                                        isDisabled={customerKey === ""}
                                        onPress={() => void submit()}
                                    >
                                        Issue note
                                    </Button>
                                </div>
                            </div>
                        </Dialog>
                    </Modal>
                </ModalOverlay>
            )}

            {detail !== null && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setDetail(null)}>
                    <Modal className="max-w-2xl">
                        <Dialog className="max-h-[85dvh] overflow-y-auto p-6">
                            <div className="flex flex-col gap-4">
                                <div>
                                    <h2 className="text-display-xs font-semibold tabular-nums text-primary">
                                        {detail.note.creditNo}
                                    </h2>
                                    <p className="text-md text-tertiary">
                                        {detail.customerName ?? "—"} · {formatTimestamp(detail.note.createdAt)}
                                    </p>
                                    <p className="text-md tabular-nums text-tertiary">
                                        Amount {formatMoney(detail.note.amount)} · Spent{" "}
                                        {formatMoney(detail.note.appliedTotal)} · Left{" "}
                                        {formatMoney(detail.remaining)}
                                    </p>
                                    {detail.note.note && (
                                        <p className="text-md text-secondary">{detail.note.note}</p>
                                    )}
                                </div>

                                {detailError !== null && (
                                    <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">
                                        {detailError}
                                    </p>
                                )}

                                {detail.remaining !== "0.000" && detail.remaining !== "0" ? (
                                    <div className="flex items-end gap-2">
                                        <Input
                                            label="Spend"
                                            aria-label="Spend amount"
                                            placeholder={detail.remaining}
                                            value={spendAmount}
                                            onChange={setSpendAmount}
                                            className="w-36"
                                        />
                                        <Button color="primary" onPress={() => void spend()}>
                                            Spend
                                        </Button>
                                    </div>
                                ) : (
                                    <p className="text-sm text-tertiary">Fully spent.</p>
                                )}

                                <div className="flex justify-end">
                                    <Button color="secondary" onPress={() => setDetail(null)}>
                                        Close
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
