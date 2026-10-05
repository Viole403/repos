import { useCallback, useEffect, useState } from "react";
import { SearchLg } from "@untitledui/icons";
import { Table, TableCard } from "@/components/application/table/table";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { formatMoney, formatQuantity } from "@/app/format";
import { getPurchaseReturn, listPurchaseReturns } from "@/app/ipc";
import type { PurchaseReturnView } from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

/** Read-only. A return is raised from a purchase, which is where the quantities to
    send back are known; this lists what has already gone back. */
export const PurchaseReturns = ({ onClose }: { onClose: () => void }) => {
    const [rows, setRows] = useState<PurchaseReturnView[]>([]);
    const [total, setTotal] = useState(0);
    const [term, setTerm] = useState("");
    const [error, setError] = useState<string | null>(null);
    const [openId, setOpenId] = useState<number | null>(null);

    const perPage = 20;

    const refresh = useCallback(async () => {
        try {
            const result = await listPurchaseReturns({
                page: 1,
                perPage,
                search: term.trim() || undefined,
            });
            setRows(result.rows);
            setTotal(result.total);
            setError(null);
        } catch (cause) {
            setError(messageOf(cause));
        }
    }, [term]);

    useEffect(() => {
        void refresh();
    }, [refresh]);

    return (
        <ModalOverlay isOpen onOpenChange={(open) => !open && onClose()}>
            <Modal className="max-w-3xl">
                <Dialog className="max-h-[85dvh] overflow-y-auto p-6">
                    <div className="flex flex-col gap-4">
                        <h2 className="text-display-xs font-semibold text-primary">Purchase returns</h2>

                        <Input
                            aria-label="Search purchase returns"
                            icon={SearchLg}
                            placeholder="Search reference or supplier"
                            value={term}
                            onChange={setTerm}
                        />

                        {error && (
                            <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">
                                {error}
                            </p>
                        )}

                        {rows.length === 0 ? (
                            <p className="py-6 text-center text-md text-tertiary">
                                {term ? "No returns match that search." : "Nothing sent back yet."}
                            </p>
                        ) : (
                            <TableCard.Root>
                                <TableCard.Header title="Returns" description={`${total} total`} />
                                <Table aria-label="Purchase returns">
                                    <Table.Header>
                                        <Table.Head label="Reference" />
                                        <Table.Head label="Purchase" />
                                        <Table.Head label="Supplier" />
                                        <Table.Head label="Date" />
                                        <Table.Head label="Amount" className="text-right" />
                                        <Table.Head label="" className="w-24" />
                                    </Table.Header>
                                    <Table.Body>
                                        {rows.map((row) => (
                                            <Table.Row key={row.id} id={row.id}>
                                                <Table.Cell className="font-medium text-primary">
                                                    {row.referenceNo}
                                                </Table.Cell>
                                                <Table.Cell>{row.purchaseReferenceNo}</Table.Cell>
                                                <Table.Cell>{row.supplierName}</Table.Cell>
                                                <Table.Cell className="text-tertiary">{row.returnedAt}</Table.Cell>
                                                <Table.Cell className="text-right">
                                                    {formatMoney(row.totalAmount)}
                                                </Table.Cell>
                                                <Table.Cell>
                                                    <Button size="sm" color="secondary" onPress={() => setOpenId(row.id)}>
                                                        Open
                                                    </Button>
                                                </Table.Cell>
                                            </Table.Row>
                                        ))}
                                    </Table.Body>
                                </Table>
                            </TableCard.Root>
                        )}

                        <div className="flex justify-end">
                            <Button color="secondary" onPress={onClose}>
                                Close
                            </Button>
                        </div>
                    </div>

                    {openId !== null && <ReturnDetail returnId={openId} onClose={() => setOpenId(null)} />}
                </Dialog>
            </Modal>
        </ModalOverlay>
    );
};

const ReturnDetail = ({ returnId, onClose }: { returnId: number; onClose: () => void }) => {
    const [detail, setDetail] = useState<PurchaseReturnView | null>(null);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        const load = async () => {
            try {
                setDetail(await getPurchaseReturn(returnId));
                setError(null);
            } catch (cause) {
                setError(messageOf(cause));
            }
        };
        void load();
    }, [returnId]);

    return (
        <ModalOverlay isOpen onOpenChange={(open) => !open && onClose()}>
            <Modal className="max-w-2xl">
                <Dialog className="p-6">
                    <div className="flex flex-col gap-4">
                        <h2 className="text-display-xs font-semibold text-primary">
                            {detail ? detail.referenceNo : "Return"}
                        </h2>

                        {error && (
                            <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">
                                {error}
                            </p>
                        )}

                        {detail && (
                            <>
                                <div className="grid grid-cols-2 gap-2 text-sm">
                                    <span className="text-tertiary">Purchase</span>
                                    <span>{detail.purchaseReferenceNo}</span>
                                    <span className="text-tertiary">Supplier</span>
                                    <span>{detail.supplierName}</span>
                                    <span className="text-tertiary">Date</span>
                                    <span>{detail.returnedAt}</span>
                                    <span className="text-tertiary">Total</span>
                                    <span>{formatMoney(detail.totalAmount)}</span>
                                </div>

                                <Table aria-label="Returned lines">
                                    <Table.Header>
                                        <Table.Head label="Item" />
                                        <Table.Head label="Quantity" className="text-right" />
                                        <Table.Head label="Unit price" className="text-right" />
                                        <Table.Head label="Total" className="text-right" />
                                    </Table.Header>
                                    <Table.Body>
                                        {detail.lines.map((line) => (
                                            <Table.Row key={line.id} id={line.id}>
                                                <Table.Cell>
                                                    {line.itemName} ({line.itemCode})
                                                </Table.Cell>
                                                <Table.Cell className="text-right">
                                                    {formatQuantity(line.quantity)}
                                                </Table.Cell>
                                                <Table.Cell className="text-right">
                                                    {formatMoney(line.unitPrice)}
                                                </Table.Cell>
                                                <Table.Cell className="text-right">{formatMoney(line.total)}</Table.Cell>
                                            </Table.Row>
                                        ))}
                                    </Table.Body>
                                </Table>
                            </>
                        )}

                        <div className="flex justify-end">
                            <Button color="secondary" onPress={onClose}>
                                Close
                            </Button>
                        </div>
                    </div>
                </Dialog>
            </Modal>
        </ModalOverlay>
    );
};