import { useCallback, useEffect, useState } from "react";
import { Plus, Trash01 } from "@untitledui/icons";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { Table, TableCard } from "@/components/application/table/table";
import { formatMoney, formatTimestamp, toDecimal } from "@/app/format";
import type { PromotionInput } from "@/app/ipc";
import {
    PROMOTION_KINDS,
    createPromotion,
    deletePromotion,
    getPromotion,
    listItems,
    listPromotions,
    updatePromotion,
} from "@/app/ipc";

type Loaded =
    | { kind: "loading" }
    | { kind: "ready" }
    | { kind: "failed"; message: string };

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

const normalizeMoney = (raw: string): string => raw.replace(/\./g, "").replace(",", ".");

const optDecimal = (raw: string): string | null => (raw.trim() === "" ? null : toDecimal(normalizeMoney(raw.trim())));

/**
 * Discount rules the till applies by itself. The form shows only the fields the
 * chosen kind uses — the server refuses anything else, so offering those fields
 * would only produce rejections.
 */
export const Promotions = () => {
    const [state, setState] = useState<Loaded>({ kind: "loading" });
    const [rows, setRows] = useState<Awaited<ReturnType<typeof listPromotions>>["rows"]>([]);
    const [total, setTotal] = useState(0);
    const [reload, setReload] = useState(0);
    const [formOpen, setFormOpen] = useState(false);
    const [editingId, setEditingId] = useState<number | null>(null);
    const [discarding, setDiscarding] = useState<number | null>(null);
    const [formError, setFormError] = useState<string | null>(null);
    const [saving, setSaving] = useState(false);

    const [title, setTitle] = useState("");
    const [kind, setKind] = useState<string>("ItemPercent");
    const [targetKey, setTargetKey] = useState("");
    const [rewardKey, setRewardKey] = useState("");
    const [itemOptions, setItemOptions] = useState<SelectItemType[]>([]);
    const [percent, setPercent] = useState("");
    const [amount, setAmount] = useState("");
    const [minTotal, setMinTotal] = useState("");
    const [buyQty, setBuyQty] = useState("");
    const [getQty, setGetQty] = useState("");
    const [startAt, setStartAt] = useState("");
    const [endAt, setEndAt] = useState("");

    const refresh = useCallback(() => setReload((n) => n + 1), []);

    useEffect(() => {
        let cancelled = false;
        setState({ kind: "loading" });

        listPromotions({ page: 1, perPage: 50 })
            .then((page) => {
                if (cancelled) return;
                setRows(page.rows);
                setTotal(page.total);
                setState({ kind: "ready" });
            })
            .catch((error: unknown) => {
                if (!cancelled) setState({ kind: "failed", message: messageOf(error) });
            });

        return () => {
            cancelled = true;
        };
    }, [reload]);

    const findItems = useCallback(async (term: string) => {
        try {
            const found = await listItems({ page: 1, perPage: 20, search: term || undefined });
            setItemOptions(found.rows.map((row) => ({ id: String(row.id), label: row.name })));
        } catch {
            setItemOptions([]);
        }
    }, []);

    const resetForm = useCallback(() => {
        setTitle("");
        setKind("ItemPercent");
        setTargetKey("");
        setRewardKey("");
        setPercent("");
        setAmount("");
        setMinTotal("");
        setBuyQty("");
        setGetQty("");
        setStartAt("");
        setEndAt("");
        setFormError(null);
    }, []);

    const openCreate = useCallback(() => {
        resetForm();
        setEditingId(null);
        setFormOpen(true);
    }, [resetForm]);

    const openEdit = useCallback(async (id: number) => {
        setFormError(null);
        try {
            const view = await getPromotion(id);
            setEditingId(id);
            setTitle(view.title);
            setKind(view.kind);
            setTargetKey(view.targetItemId === null ? "" : String(view.targetItemId));
            setRewardKey(view.rewardItemId === null ? "" : String(view.rewardItemId));
            setPercent(view.percent ?? "");
            setAmount(view.amount ?? "");
            setMinTotal(view.minTotal ?? "");
            setBuyQty(view.buyQty ?? "");
            setGetQty(view.getQty ?? "");
            setStartAt(view.startAt.slice(0, 10));
            setEndAt(view.endAt.slice(0, 10));
            setFormOpen(true);
        } catch (error) {
            setFormError(messageOf(error));
            setFormOpen(true);
        }
    }, []);

    const save = useCallback(async () => {
        setSaving(true);
        setFormError(null);
        try {
            const input: PromotionInput = {
                title: title.trim(),
                kind,
                targetItemId: targetKey === "" ? null : Number(targetKey),
                rewardItemId: rewardKey === "" ? null : Number(rewardKey),
                percent: optDecimal(percent),
                amount: optDecimal(amount),
                minTotal: optDecimal(minTotal),
                buyQty: buyQty.trim() === "" ? null : buyQty.trim(),
                getQty: getQty.trim() === "" ? null : getQty.trim(),
                startAt: startAt.trim(),
                endAt: endAt.trim(),
            };
            if (editingId === null) {
                await createPromotion(input);
            } else {
                await updatePromotion(editingId, input);
            }
            setFormOpen(false);
            refresh();
        } catch (error) {
            setFormError(messageOf(error));
        } finally {
            setSaving(false);
        }
    }, [title, kind, targetKey, rewardKey, percent, amount, minTotal, buyQty, getQty, startAt, endAt, editingId, refresh]);

    const confirmDiscard = useCallback(async () => {
        if (discarding === null) return;
        const id = discarding;
        setDiscarding(null);
        try {
            await deletePromotion(id);
            refresh();
        } catch (error) {
            setState({ kind: "failed", message: messageOf(error) });
        }
    }, [discarding, refresh]);

    const needsItem = kind === "ItemPercent" || kind === "ItemFixed";
    const needsBuyGet = kind === "BuyGet";
    const needsPercent = kind === "ItemPercent" || kind === "OrderPercent";
    const needsAmount = kind === "ItemFixed" || kind === "OrderFixed";
    const needsMin = kind === "OrderPercent" || kind === "OrderFixed";

    const kindOptions: SelectItemType[] = PROMOTION_KINDS.map((k) => ({ id: k, label: k }));

    return (
        <div className="flex flex-col gap-6 p-6 md:p-8">
            <TableCard.Root>
                <TableCard.Header
                    title="Promotions"
                    description={`${total} rule${total === 1 ? "" : "s"}`}
                    contentTrailing={
                        <Button size="sm" iconLeading={Plus} onPress={openCreate}>
                            New promotion
                        </Button>
                    }
                />

                {state.kind === "failed" ? (
                    <p className="px-6 py-12 text-center text-md text-error-primary">{state.message}</p>
                ) : state.kind === "loading" ? (
                    <p className="px-6 py-12 text-center text-md text-tertiary">Loading…</p>
                ) : rows.length === 0 ? (
                    <p className="px-6 py-12 text-center text-md text-tertiary">No promotions yet.</p>
                ) : (
                    <Table>
                        <Table.Header>
                            <Table.Head label="Title" />
                            <Table.Head label="Kind" />
                            <Table.Head label="Value" />
                            <Table.Head label="Dates" />
                            <Table.Head label="" />
                        </Table.Header>
                        <Table.Body>
                            {rows.map((row) => (
                                <Table.Row key={row.id} id={row.id}>
                                    <Table.Cell className="font-medium text-primary">{row.title}</Table.Cell>
                                    <Table.Cell className="text-tertiary">{row.kind}</Table.Cell>
                                    <Table.Cell className="tabular-nums">
                                        {row.percent !== null
                                            ? `${row.percent}%`
                                            : row.amount !== null
                                              ? formatMoney(row.amount)
                                              : row.kind === "BuyGet"
                                                ? `buy ${row.buyQty} get ${row.getQty}`
                                                : "—"}
                                    </Table.Cell>
                                    <Table.Cell className="text-tertiary">
                                        {formatTimestamp(row.startAt)} – {formatTimestamp(row.endAt)}
                                    </Table.Cell>
                                    <Table.Cell>
                                        <div className="flex justify-end gap-1">
                                            <Button size="sm" color="secondary" onPress={() => void openEdit(row.id)}>
                                                Edit
                                            </Button>
                                            <Button
                                                size="sm"
                                                color="tertiary"
                                                iconLeading={Trash01}
                                                aria-label={`Delete ${row.title}`}
                                                onPress={() => setDiscarding(row.id)}
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
                                    {editingId === null ? "New promotion" : "Edit promotion"}
                                </h2>

                                {formError !== null && (
                                    <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">
                                        {formError}
                                    </p>
                                )}

                                <Input label="Title" value={title} onChange={setTitle} isRequired />

                                <Select
                                    label="Kind"
                                    items={kindOptions}
                                    selectedKey={kind}
                                    onSelectionChange={(key) => setKind(String(key ?? "ItemPercent"))}
                                >
                                    {(row) => (
                                        <Select.Item id={row.id} textValue={row.label}>
                                            {row.label}
                                        </Select.Item>
                                    )}
                                </Select>

                                {(needsItem || needsBuyGet) && (
                                    <Select
                                        label={needsBuyGet ? "Buy item" : "Item"}
                                        items={itemOptions}
                                        selectedKey={targetKey}
                                        onOpenChange={(open) => open && void findItems("")}
                                        onSelectionChange={(key) => setTargetKey(String(key ?? ""))}
                                        isRequired
                                    >
                                        {(row) => (
                                            <Select.Item id={row.id} textValue={row.label}>
                                                {row.label}
                                            </Select.Item>
                                        )}
                                    </Select>
                                )}

                                {needsBuyGet && (
                                    <>
                                        <Select
                                            label="Free item"
                                            items={itemOptions}
                                            selectedKey={rewardKey}
                                            onOpenChange={(open) => open && void findItems("")}
                                            onSelectionChange={(key) => setRewardKey(String(key ?? ""))}
                                            isRequired
                                        >
                                            {(row) => (
                                                <Select.Item id={row.id} textValue={row.label}>
                                                    {row.label}
                                                </Select.Item>
                                            )}
                                        </Select>
                                        <div className="grid grid-cols-2 gap-3">
                                            <Input label="Buy quantity" value={buyQty} onChange={setBuyQty} isRequired />
                                            <Input label="Free quantity" value={getQty} onChange={setGetQty} isRequired />
                                        </div>
                                    </>
                                )}

                                {needsPercent && <Input label="Percent" placeholder="0–100" value={percent} onChange={setPercent} isRequired />}
                                {needsAmount && <Input label="Amount off" value={amount} onChange={setAmount} isRequired />}
                                {needsMin && <Input label="Minimum subtotal" placeholder="Blank means none" value={minTotal} onChange={setMinTotal} />}

                                <div className="grid grid-cols-2 gap-3">
                                    <Input label="Starts" type="date" value={startAt} onChange={setStartAt} isRequired />
                                    <Input label="Ends" type="date" value={endAt} onChange={setEndAt} isRequired />
                                </div>

                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setFormOpen(false)}>
                                        Cancel
                                    </Button>
                                    <Button onPress={() => void save()} isLoading={saving} isDisabled={title.trim() === ""}>
                                        {editingId === null ? "Create promotion" : "Save changes"}
                                    </Button>
                                </div>
                            </div>
                        </Dialog>
                    </Modal>
                </ModalOverlay>
            )}

            {discarding !== null && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setDiscarding(null)}>
                    <Modal className="max-w-md">
                        <Dialog className="p-6">
                            <div className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">Delete this promotion?</h2>
                                <p className="text-md text-secondary">
                                    Past sales keep the discount amounts on their lines. Future
                                    sales simply stop receiving it.
                                </p>
                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setDiscarding(null)}>
                                        Cancel
                                    </Button>
                                    <Button color="primary-destructive" onPress={confirmDiscard}>
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
