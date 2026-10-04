import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import type { StockRow } from "@/app/ipc";
import {
    COUNT_REASONS,
    listStock,
    recordDamage,
    recordGoodsReceipt,
    recordOpeningStock,
    recordStockCount,
} from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

type Op = "receipt" | "opening" | "count" | "damage";

/**
 * The four things that move stock by hand. All of them append to the same ledger as
 * a sale does, so on-hand stays derived rather than stored.
 *
 * The count form is deliberately blind: it asks what the counter saw and never shows
 * the expected quantity, because a shelf figure on screen is an anchor and anchors
 * make counts agree with a number that is already wrong.
 */
export const StockOps = () => {
    const [items, setItems] = useState<SelectItemType[]>([]);
    const [op, setOp] = useState<Op | null>(null);
    const [itemId, setItemId] = useState("");
    const [quantity, setQuantity] = useState("");
    const [reason, setReason] = useState<string>(COUNT_REASONS[0]);
    const [note, setNote] = useState("");
    const [error, setError] = useState<string | null>(null);
    const [saved, setSaved] = useState<string | null>(null);
    const [busy, setBusy] = useState(false);

    const loadItems = useCallback(async () => {
        try {
            const page = await listStock(null, { perPage: 500 });
            setItems(page.rows.map((row: StockRow) => ({ id: row.itemId, label: row.itemName })));
        } catch (cause) {
            setError(messageOf(cause));
        }
    }, []);

    useEffect(() => {
        void loadItems();
    }, [loadItems]);

    const close = () => {
        setOp(null);
        setItemId("");
        setQuantity("");
        setNote("");
        setError(null);
    };

    const submit = async (e: React.FormEvent) => {
        e.preventDefault();
        if (!op || itemId === "") return;
        setBusy(true);
        setError(null);
        try {
            if (op === "count") {
                await recordStockCount({ itemId: Number(itemId), counted: quantity, reason, note: note || null });
            } else if (op === "damage") {
                await recordDamage({ itemId: Number(itemId), quantity, note: note || null });
            } else if (op === "receipt") {
                await recordGoodsReceipt({ itemId: Number(itemId), quantity, reference: note || null });
            } else {
                await recordOpeningStock({ itemId: Number(itemId), quantity, reference: note || null });
            }
            close();
            setSaved("Posted to the ledger.");
            await loadItems();
        } catch (cause) {
            setError(messageOf(cause));
        } finally {
            setBusy(false);
        }
    };

    const buttons: { id: Op; label: string; hint: string }[] = [
        { id: "receipt", label: "Receive goods", hint: "Stock arriving from a supplier" },
        { id: "opening", label: "Opening stock", hint: "First count for an item, once only" },
        { id: "count", label: "Stock count", hint: "Blind: enter what is on the shelf" },
        { id: "damage", label: "Record damage", hint: "Spoiled or written off" },
    ];

    return (
        <div className="flex flex-col gap-5 p-5">
            <div className="flex flex-col gap-1">
                <h1 className="text-display-xs font-semibold text-primary">Stock operations</h1>
                <p className="text-md text-tertiary">Every action here writes one ledger row.</p>
            </div>

            {saved && <p className="rounded-lg bg-success-secondary px-3 py-2 text-sm text-success-primary">{saved}</p>}

            <div className="flex flex-wrap gap-3">
                {buttons.map((button) => (
                    <Button key={button.id} color="secondary" onPress={() => setOp(button.id)}>
                        {button.label}
                    </Button>
                ))}
            </div>

            <ul className="text-sm text-tertiary">
                {buttons.map((button) => (
                    <li key={button.id}>
                        <span className="font-medium text-secondary">{button.label}</span> — {button.hint}
                    </li>
                ))}
            </ul>

            {op !== null && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && close()}>
                    <Modal className="max-w-lg">
                        <Dialog className="p-6">
                            <form onSubmit={submit} className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">
                                    {buttons.find((b) => b.id === op)?.label}
                                </h2>
                                {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}
                                <Select
                                    label="Item"
                                    items={items}
                                    selectedKey={itemId}
                                    onSelectionChange={(key) => setItemId(String(key ?? ""))}
                                    isRequired
                                >
                                    {(row) => (
                                        <Select.Item id={row.id} textValue={row.label}>
                                            {row.label}
                                        </Select.Item>
                                    )}
                                </Select>
                                <Input
                                    label={op === "count" ? "Counted quantity" : "Quantity"}
                                    type="number"
                                    value={quantity}
                                    onChange={setQuantity}
                                    isRequired
                                    hint={
                                        op === "count"
                                            ? "What is physically on the shelf. The expected figure is not shown on purpose."
                                            : undefined
                                    }
                                />
                                {op === "count" && (
                                    <Select
                                        label="Reason"
                                        items={COUNT_REASONS.map((r) => ({ id: r, label: r }))}
                                        selectedKey={reason}
                                        onSelectionChange={(key) => setReason(String(key ?? ""))}
                                        isRequired
                                    >
                                        {(row) => (
                                            <Select.Item id={row.id} textValue={row.label}>
                                                {row.label}
                                            </Select.Item>
                                        )}
                                    </Select>
                                )}
                                <Input
                                    label="Note"
                                    value={note}
                                    onChange={setNote}
                                    hint={op === "count" ? "Appended to the reason on the ledger row" : undefined}
                                />
                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={close}>
                                        Cancel
                                    </Button>
                                    <Button type="submit" isLoading={busy} isDisabled={itemId === "" || quantity.trim() === ""}>
                                        Post
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