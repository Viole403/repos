import { useCallback, useEffect, useState } from "react";
import { ArrowRight, Plus } from "@untitledui/icons";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { Table, TableCard } from "@/components/application/table/table";
import { formatMoney, formatTimestamp, toDecimal } from "@/app/format";
import type { ServicingSummary, ServicingView } from "@/app/ipc";
import {
    collectServicingPayment,
    createServicing,
    getServicing,
    listCustomers,
    listServicings,
} from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

const normalizeMoney = (raw: string): string => raw.replace(/\./g, "").replace(",", ".");

const SERVICING_STATUSES: SelectItemType[] = [
    { id: "Received", label: "Received" },
    { id: "InRepair", label: "In repair" },
    { id: "Ready", label: "Ready" },
    { id: "Delivered", label: "Delivered" },
];

const statusLabel = (status: string): string =>
    SERVICING_STATUSES.find((s) => s.id === status)?.label ?? status;

const today = () => new Date().toISOString().slice(0, 10);

/**
 * Paid repair jobs: the charge is agreed up front, the due shrinks as money
 * arrives. Overpayments are refused, like tenders above the total at the till.
 */
export const Servicings = () => {
    const [rows, setRows] = useState<ServicingSummary[]>([]);
    const [total, setTotal] = useState(0);
    const [page, setPage] = useState(1);
    const [error, setError] = useState<string | null>(null);

    const [formOpen, setFormOpen] = useState(false);
    const [formError, setFormError] = useState<string | null>(null);
    const [customers, setCustomers] = useState<SelectItemType[]>([]);
    const [customerKey, setCustomerKey] = useState("");
    const [productName, setProductName] = useState("");
    const [productModel, setProductModel] = useState("");
    const [problem, setProblem] = useState("");
    const [receivingDate, setReceivingDate] = useState(today());
    const [charge, setCharge] = useState("");

    const [detail, setDetail] = useState<ServicingView | null>(null);
    const [detailError, setDetailError] = useState<string | null>(null);
    const [collectAmount, setCollectAmount] = useState("");

    const perPage = 20;

    const refresh = useCallback(async () => {
        try {
            const found = await listServicings(null, { page, perPage });
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
        setProductName("");
        setProductModel("");
        setProblem("");
        setReceivingDate(today());
        setCharge("");
        setFormError(null);
        setFormOpen(true);
    }, []);

    const submit = useCallback(async () => {
        setFormError(null);
        try {
            const view = await createServicing({
                customerId: Number(customerKey),
                productName: productName.trim(),
                productModel: productModel.trim() === "" ? null : productModel.trim(),
                problemDescription: problem.trim() === "" ? null : problem.trim(),
                receivingDate,
                servicingCharge: toDecimal(normalizeMoney(charge.trim() === "" ? "0" : charge)),
            });
            setFormOpen(false);
            setDetail(view);
            setDetailError(null);
            setCollectAmount("");
            void refresh();
        } catch (cause) {
            setFormError(messageOf(cause));
        }
    }, [customerKey, productName, productModel, problem, receivingDate, charge, refresh]);

    const openDetail = useCallback(async (id: number) => {
        setDetailError(null);
        try {
            setDetail(await getServicing(id));
            setCollectAmount("");
        } catch (cause) {
            setDetailError(messageOf(cause));
        }
    }, []);

    const collect = useCallback(async () => {
        if (detail === null) return;
        setDetailError(null);
        try {
            const raw = collectAmount.trim();
            setDetail(
                await collectServicingPayment(
                    detail.servicing.id,
                    toDecimal(normalizeMoney(raw === "" ? detail.dueAmount : raw)),
                ),
            );
            setCollectAmount("");
            void refresh();
        } catch (cause) {
            setDetailError(messageOf(cause));
        }
    }, [detail, collectAmount, refresh]);

    const pages = Math.max(1, Math.ceil(total / perPage));

    return (
        <div className="flex flex-col gap-5 p-5">
            <div className="flex flex-col gap-1">
                <h1 className="text-display-xs font-semibold text-primary">Servicing</h1>
                <p className="text-md text-tertiary">Paid repair jobs and what each still owes.</p>
            </div>

            <div>
                <Button color="primary" iconLeading={Plus} onPress={openCreate}>
                    New servicing job
                </Button>
            </div>

            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

            {rows.length === 0 ? (
                <p className="py-10 text-center text-md text-tertiary">No servicing jobs yet.</p>
            ) : (
                <TableCard.Root>
                    <TableCard.Header title="Servicing" description={`${total} total`} />
                    <Table aria-label="Servicing">
                        <Table.Header>
                            <Table.Head label="Product" />
                            <Table.Head label="Customer" />
                            <Table.Head label="Charge" />
                            <Table.Head label="Paid" />
                            <Table.Head label="Due" />
                            <Table.Head label="Status" />
                            <Table.Head label="" className="w-12" />
                        </Table.Header>
                        <Table.Body>
                            {rows.map((row) => (
                                <Table.Row key={row.id} id={row.id}>
                                    <Table.Cell className="font-medium text-primary">{row.productName}</Table.Cell>
                                    <Table.Cell>{row.customerName ?? "—"}</Table.Cell>
                                    <Table.Cell className="tabular-nums">
                                        {formatMoney(row.servicingCharge)}
                                    </Table.Cell>
                                    <Table.Cell className="tabular-nums">{formatMoney(row.paidAmount)}</Table.Cell>
                                    <Table.Cell className="tabular-nums">{formatMoney(row.dueAmount)}</Table.Cell>
                                    <Table.Cell className="text-tertiary">{statusLabel(row.currentStatus)}</Table.Cell>
                                    <Table.Cell>
                                        <Button
                                            size="sm"
                                            color="tertiary"
                                            iconLeading={ArrowRight}
                                            aria-label={`Open ${row.productName}`}
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
                                <h2 className="text-display-xs font-semibold text-primary">New servicing job</h2>

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
                                    <Input label="Product" value={productName} onChange={setProductName} isRequired />
                                    <Input label="Model" value={productModel} onChange={setProductModel} />
                                </div>

                                <Input label="Problem" value={problem} onChange={setProblem} />

                                <div className="grid grid-cols-2 gap-3">
                                    <Input
                                        label="Received"
                                        type="date"
                                        value={receivingDate}
                                        onChange={(value) => setReceivingDate(String(value))}
                                    />
                                    <Input label="Charge" value={charge} onChange={setCharge} isRequired />
                                </div>

                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setFormOpen(false)}>
                                        Cancel
                                    </Button>
                                    <Button
                                        color="primary"
                                        isDisabled={customerKey === "" || productName.trim() === ""}
                                        onPress={() => void submit()}
                                    >
                                        Open job
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
                                    <h2 className="text-display-xs font-semibold text-primary">
                                        {detail.servicing.productName}
                                    </h2>
                                    <p className="text-md text-tertiary">
                                        {detail.customerName ?? "—"} · {statusLabel(detail.servicing.currentStatus)} ·
                                        received {formatTimestamp(detail.servicing.receivingDate)}
                                    </p>
                                    <p className="text-md tabular-nums text-tertiary">
                                        Charge {formatMoney(detail.servicing.servicingCharge)} · Paid{" "}
                                        {formatMoney(detail.servicing.paidAmount)} · Due{" "}
                                        {formatMoney(detail.dueAmount)}
                                    </p>
                                    {detail.servicing.problemDescription && (
                                        <p className="text-md text-secondary">
                                            {detail.servicing.problemDescription}
                                        </p>
                                    )}
                                </div>

                                {detailError !== null && (
                                    <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">
                                        {detailError}
                                    </p>
                                )}

                                {detail.dueAmount !== "0.000" && detail.dueAmount !== "0" ? (
                                    <div className="flex items-end gap-2">
                                        <Input
                                            label="Collect"
                                            aria-label="Collect amount"
                                            placeholder={detail.dueAmount}
                                            value={collectAmount}
                                            onChange={setCollectAmount}
                                            className="w-36"
                                        />
                                        <Button color="primary" onPress={() => void collect()}>
                                            Collect
                                        </Button>
                                    </div>
                                ) : (
                                    <p className="text-sm text-tertiary">Paid in full.</p>
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
