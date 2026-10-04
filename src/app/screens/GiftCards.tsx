import { useCallback, useEffect, useState } from "react";
import { ArrowRight, Plus } from "@untitledui/icons";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { Table, TableCard } from "@/components/application/table/table";
import { formatMoney, formatTimestamp, toDecimal } from "@/app/format";
import type { GiftCardSummary, GiftCardView } from "@/app/ipc";
import { getGiftCard, listGiftCards, reloadGiftCard, sellGiftCard } from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

const normalizeMoney = (raw: string): string => raw.replace(/\./g, "").replace(",", ".");

const PAYMENT_METHODS: SelectItemType[] = [
    { id: "Cash", label: "Cash" },
    { id: "Card", label: "Card" },
    { id: "Qris", label: "QRIS" },
];

/**
 * Stored value: sell a card number, reload it, spend it at the till.
 * The balance is derived server-side — this screen displays, never computes.
 */
export const GiftCards = () => {
    const [rows, setRows] = useState<GiftCardSummary[]>([]);
    const [total, setTotal] = useState(0);
    const [page, setPage] = useState(1);
    const [error, setError] = useState<string | null>(null);

    const [formOpen, setFormOpen] = useState(false);
    const [formError, setFormError] = useState<string | null>(null);
    const [cardNo, setCardNo] = useState("");
    const [amount, setAmount] = useState("");
    const [method, setMethod] = useState("Cash");
    const [pin, setPin] = useState("");

    const [detail, setDetail] = useState<GiftCardView | null>(null);
    const [detailError, setDetailError] = useState<string | null>(null);
    const [reloadAmount, setReloadAmount] = useState("");
    const [reloadMethod, setReloadMethod] = useState("Cash");

    const perPage = 20;

    const refresh = useCallback(async () => {
        try {
            const found = await listGiftCards({ page, perPage });
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

    const openCreate = useCallback(() => {
        setCardNo("");
        setAmount("");
        setMethod("Cash");
        setPin("");
        setFormError(null);
        setFormOpen(true);
    }, []);

    const submit = useCallback(async () => {
        setFormError(null);
        try {
            const view = await sellGiftCard(
                cardNo.trim(),
                toDecimal(normalizeMoney(amount.trim() === "" ? "0" : amount)),
                method,
                pin.trim() === "" ? undefined : pin.trim(),
            );
            setFormOpen(false);
            setDetail(view);
            setDetailError(null);
            setReloadAmount("");
            void refresh();
        } catch (cause) {
            setFormError(messageOf(cause));
        }
    }, [cardNo, amount, method, pin, refresh]);

    const openDetail = useCallback(async (no: string) => {
        setDetailError(null);
        try {
            setDetail(await getGiftCard(no));
            setReloadAmount("");
        } catch (cause) {
            setDetailError(messageOf(cause));
        }
    }, []);

    const reload = useCallback(async () => {
        if (detail === null) return;
        setDetailError(null);
        try {
            const raw = reloadAmount.trim();
            setDetail(
                await reloadGiftCard(
                    detail.card.cardNo,
                    toDecimal(normalizeMoney(raw === "" ? "0" : raw)),
                    reloadMethod,
                ),
            );
            setReloadAmount("");
            void refresh();
        } catch (cause) {
            setDetailError(messageOf(cause));
        }
    }, [detail, reloadAmount, reloadMethod, refresh]);

    const pages = Math.max(1, Math.ceil(total / perPage));

    return (
        <div className="flex flex-col gap-5 p-5">
            <div className="flex flex-col gap-1">
                <h1 className="text-display-xs font-semibold text-primary">Gift cards</h1>
                <p className="text-md text-tertiary">Stored value, sold and spent at the till.</p>
            </div>

            <div>
                <Button color="primary" iconLeading={Plus} onPress={openCreate}>
                    Sell gift card
                </Button>
            </div>

            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

            {rows.length === 0 ? (
                <p className="py-10 text-center text-md text-tertiary">No gift cards yet.</p>
            ) : (
                <TableCard.Root>
                    <TableCard.Header title="Gift cards" description={`${total} total`} />
                    <Table aria-label="Gift cards">
                        <Table.Header>
                            <Table.Head label="Card" />
                            <Table.Head label="Balance" />
                            <Table.Head label="Sold" />
                            <Table.Head label="" className="w-12" />
                        </Table.Header>
                        <Table.Body>
                            {rows.map((row) => (
                                <Table.Row key={row.id} id={row.id}>
                                    <Table.Cell className="font-medium tabular-nums text-primary">
                                        {row.cardNo}
                                    </Table.Cell>
                                    <Table.Cell className="tabular-nums">{formatMoney(row.balance)}</Table.Cell>
                                    <Table.Cell className="text-tertiary">
                                        {formatTimestamp(row.createdAt)}
                                    </Table.Cell>
                                    <Table.Cell>
                                        <Button
                                            size="sm"
                                            color="tertiary"
                                            iconLeading={ArrowRight}
                                            aria-label={`Open ${row.cardNo}`}
                                            onPress={() => void openDetail(row.cardNo)}
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
                                <h2 className="text-display-xs font-semibold text-primary">Sell gift card</h2>

                                {formError !== null && (
                                    <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">
                                        {formError}
                                    </p>
                                )}

                                <div className="grid grid-cols-2 gap-3">
                                    <Input
                                        label="Card number"
                                        hint="Printed on the physical card"
                                        value={cardNo}
                                        onChange={setCardNo}
                                        isRequired
                                    />
                                    <Input label="Amount" value={amount} onChange={setAmount} isRequired />
                                    <Select
                                        label="Payment method"
                                        items={PAYMENT_METHODS}
                                        selectedKey={method}
                                        onSelectionChange={(key) => setMethod(String(key ?? "Cash"))}
                                    >
                                        {(row) => (
                                            <Select.Item id={row.id} textValue={row.label}>
                                                {row.label}
                                            </Select.Item>
                                        )}
                                    </Select>
                                    <Input
                                        label="PIN"
                                        hint="Optional; checked at redeem when set"
                                        value={pin}
                                        onChange={setPin}
                                    />
                                </div>

                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setFormOpen(false)}>
                                        Cancel
                                    </Button>
                                    <Button
                                        color="primary"
                                        isDisabled={cardNo.trim() === ""}
                                        onPress={() => void submit()}
                                    >
                                        Sell card
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
                                        {detail.card.cardNo}
                                    </h2>
                                    <p className="text-md tabular-nums text-tertiary">
                                        Balance {formatMoney(detail.balance)}
                                    </p>
                                </div>

                                {detailError !== null && (
                                    <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">
                                        {detailError}
                                    </p>
                                )}

                                <TableCard.Root>
                                    <TableCard.Header title="History" description={`${detail.transactions.length}`} />
                                    <Table aria-label="Transactions">
                                        <Table.Header>
                                            <Table.Head label="When" />
                                            <Table.Head label="Kind" />
                                            <Table.Head label="Amount" />
                                            <Table.Head label="Balance" />
                                        </Table.Header>
                                        <Table.Body>
                                            {detail.transactions.map((t) => (
                                                <Table.Row key={t.id} id={t.id}>
                                                    <Table.Cell className="text-tertiary">
                                                        {formatTimestamp(t.createdAt)}
                                                    </Table.Cell>
                                                    <Table.Cell>{t.kind}</Table.Cell>
                                                    <Table.Cell className="tabular-nums">
                                                        {formatMoney(t.amount)}
                                                    </Table.Cell>
                                                    <Table.Cell className="tabular-nums">
                                                        {formatMoney(t.balanceAfter)}
                                                    </Table.Cell>
                                                </Table.Row>
                                            ))}
                                        </Table.Body>
                                    </Table>
                                </TableCard.Root>

                                <div className="flex items-end gap-2">
                                    <Input
                                        label="Reload amount"
                                        value={reloadAmount}
                                        onChange={setReloadAmount}
                                        className="w-36"
                                    />
                                    <Select
                                        label="Method"
                                        aria-label="Reload method"
                                        items={PAYMENT_METHODS}
                                        selectedKey={reloadMethod}
                                        onSelectionChange={(key) => setReloadMethod(String(key ?? "Cash"))}
                                        className="w-32"
                                    >
                                        {(row) => (
                                            <Select.Item id={row.id} textValue={row.label}>
                                                {row.label}
                                            </Select.Item>
                                        )}
                                    </Select>
                                    <Button color="primary" onPress={() => void reload()}>
                                        Reload
                                    </Button>
                                </div>

                                <p className="text-sm text-tertiary">
                                    To spend: split the tender at the register and pick the gift card method.
                                </p>

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
