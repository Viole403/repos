import { useCallback, useEffect, useState } from "react";
import { ArrowRight, Plus } from "@untitledui/icons";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { Table, TableCard } from "@/components/application/table/table";
import { formatTimestamp } from "@/app/format";
import type { WarrantySummary, WarrantyView } from "@/app/ipc";
import {
    WARRANTY_STATUSES,
    createWarranty,
    getWarranty,
    listCustomers,
    listWarranties,
    setWarrantyStatus,
} from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

const statusLabel = (status: string): string =>
    WARRANTY_STATUSES.find((s) => s.id === status)?.label ?? status;

const today = () => new Date().toISOString().slice(0, 10);

/**
 * Repair tickets: a sold product travelling customer → vendor → customer.
 * The pipeline moves in any order — a unit can come straight back.
 */
export const Warranties = () => {
    const [rows, setRows] = useState<WarrantySummary[]>([]);
    const [total, setTotal] = useState(0);
    const [page, setPage] = useState(1);
    const [error, setError] = useState<string | null>(null);

    const [formOpen, setFormOpen] = useState(false);
    const [formError, setFormError] = useState<string | null>(null);
    const [customers, setCustomers] = useState<SelectItemType[]>([]);
    const [customerKey, setCustomerKey] = useState("");
    const [productName, setProductName] = useState("");
    const [serialNo, setSerialNo] = useState("");
    const [description, setDescription] = useState("");
    const [receivingDate, setReceivingDate] = useState(today());
    const [location, setLocation] = useState("");

    const [detail, setDetail] = useState<WarrantyView | null>(null);
    const [detailError, setDetailError] = useState<string | null>(null);

    const perPage = 20;

    const refresh = useCallback(async () => {
        try {
            const found = await listWarranties(null, { page, perPage });
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
        setSerialNo("");
        setDescription("");
        setReceivingDate(today());
        setLocation("");
        setFormError(null);
        setFormOpen(true);
    }, []);

    const submit = useCallback(async () => {
        setFormError(null);
        try {
            const view = await createWarranty({
                customerId: Number(customerKey),
                productName: productName.trim(),
                productSerialNo: serialNo.trim() === "" ? null : serialNo.trim(),
                description: description.trim() === "" ? null : description.trim(),
                receivingDate,
                presentLocation: location.trim() === "" ? null : location.trim(),
            });
            setFormOpen(false);
            setDetail(view);
            setDetailError(null);
            void refresh();
        } catch (cause) {
            setFormError(messageOf(cause));
        }
    }, [customerKey, productName, serialNo, description, receivingDate, location, refresh]);

    const openDetail = useCallback(async (id: number) => {
        setDetailError(null);
        try {
            setDetail(await getWarranty(id));
        } catch (cause) {
            setDetailError(messageOf(cause));
        }
    }, []);

    const advance = useCallback(
        async (id: number, status: string) => {
            setDetailError(null);
            try {
                setDetail(await setWarrantyStatus(id, status));
                void refresh();
            } catch (cause) {
                setDetailError(messageOf(cause));
            }
        },
        [refresh],
    );

    const pages = Math.max(1, Math.ceil(total / perPage));

    return (
        <div className="flex flex-col gap-5 p-5">
            <div className="flex flex-col gap-1">
                <h1 className="text-display-xs font-semibold text-primary">Warranties</h1>
                <p className="text-md text-tertiary">Repair tickets travelling customer → vendor → customer.</p>
            </div>

            <div>
                <Button color="primary" iconLeading={Plus} onPress={openCreate}>
                    New warranty ticket
                </Button>
            </div>

            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

            {rows.length === 0 ? (
                <p className="py-10 text-center text-md text-tertiary">No warranty tickets yet.</p>
            ) : (
                <TableCard.Root>
                    <TableCard.Header title="Warranties" description={`${total} total`} />
                    <Table aria-label="Warranties">
                        <Table.Header>
                            <Table.Head label="Product" />
                            <Table.Head label="Customer" />
                            <Table.Head label="Received" />
                            <Table.Head label="Status" />
                            <Table.Head label="" className="w-12" />
                        </Table.Header>
                        <Table.Body>
                            {rows.map((row) => (
                                <Table.Row key={row.id} id={row.id}>
                                    <Table.Cell className="font-medium text-primary">
                                        {row.productName}
                                        {row.productSerialNo && (
                                            <span className="text-tertiary"> · {row.productSerialNo}</span>
                                        )}
                                    </Table.Cell>
                                    <Table.Cell>{row.customerName ?? "—"}</Table.Cell>
                                    <Table.Cell className="text-tertiary">
                                        {formatTimestamp(row.receivingDate)}
                                    </Table.Cell>
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
                                <h2 className="text-display-xs font-semibold text-primary">New warranty ticket</h2>

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
                                    <Input label="Serial no" value={serialNo} onChange={setSerialNo} />
                                </div>

                                <Input label="Problem" value={description} onChange={setDescription} />

                                <div className="grid grid-cols-2 gap-3">
                                    <Input
                                        label="Received"
                                        type="date"
                                        value={receivingDate}
                                        onChange={(value) => setReceivingDate(String(value))}
                                    />
                                    <Input label="Present location" value={location} onChange={setLocation} />
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
                                        Open ticket
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
                                        {detail.warranty.productName}
                                    </h2>
                                    <p className="text-md text-tertiary">
                                        {detail.customerName ?? "—"} · received{" "}
                                        {formatTimestamp(detail.warranty.receivingDate)}
                                    </p>
                                    {detail.warranty.description && (
                                        <p className="text-md text-secondary">{detail.warranty.description}</p>
                                    )}
                                </div>

                                {detailError !== null && (
                                    <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">
                                        {detailError}
                                    </p>
                                )}

                                <div className="flex flex-col gap-2">
                                    <span className="text-sm font-medium text-primary">Pipeline</span>
                                    <div className="flex flex-wrap gap-2">
                                        {WARRANTY_STATUSES.map((stage) => (
                                            <Button
                                                key={stage.id}
                                                size="sm"
                                                color={detail.warranty.currentStatus === stage.id ? "primary" : "secondary"}
                                                onPress={() => void advance(detail.warranty.id, stage.id)}
                                            >
                                                {stage.label}
                                            </Button>
                                        ))}
                                    </div>
                                </div>

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
