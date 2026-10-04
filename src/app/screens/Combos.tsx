import { useCallback, useEffect, useState } from "react";
import { Plus, Trash01 } from "@untitledui/icons";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { Table, TableCard } from "@/components/application/table/table";
import { formatQuantity, toDecimal } from "@/app/format";
import type { ComboItem } from "@/app/ipc";
import { createComboItem, deleteComboItem, listComboItems, listItems } from "@/app/ipc";

type Loaded = { kind: "loading" } | { kind: "ready" } | { kind: "failed"; message: string };

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

/**
 * Bundle definitions: which components one sold unit moves. Selling the bundle
 * writes one line at the bundle price and decrements the components — the bundle
 * item itself is virtual and holds no stock.
 */
export const Combos = () => {
    const [state, setState] = useState<Loaded>({ kind: "loading" });
    const [bundleKey, setBundleKey] = useState("");
    const [names, setNames] = useState<Map<number, string>>(new Map());
    const [rows, setRows] = useState<ComboItem[]>([]);
    const [itemTerm, setItemTerm] = useState("");
    const [itemOptions, setItemOptions] = useState<SelectItemType[]>([]);
    const [pickedItem, setPickedItem] = useState("");
    const [qty, setQty] = useState("");
    const [error, setError] = useState<string | null>(null);
    const [saving, setSaving] = useState(false);
    const [reload, setReload] = useState(0);

    const refresh = useCallback(() => setReload((n) => n + 1), []);

    // One page of the catalog covers the name map and both pickers. Catalogs
    // larger than a page need server-side search here instead.
    useEffect(() => {
        let cancelled = false;
        listItems({ page: 1, perPage: 500 })
            .then((page) => {
                if (cancelled) return;
                const options = page.rows.map((row) => ({ id: String(row.id), label: row.name }));
                setItemOptions(options);
                setNames(new Map(page.rows.map((row) => [row.id, row.name])));
                setState({ kind: "ready" });
            })
            .catch((error: unknown) => {
                if (!cancelled) setState({ kind: "failed", message: messageOf(error) });
            });
        return () => {
            cancelled = true;
        };
    }, []);

    useEffect(() => {
        if (bundleKey === "") {
            setRows([]);
            return;
        }
        let cancelled = false;
        listComboItems(Number(bundleKey))
            .then((page) => {
                if (!cancelled) setRows(page.rows);
            })
            .catch((error: unknown) => {
                if (!cancelled) setError(messageOf(error));
            });
        return () => {
            cancelled = true;
        };
    }, [bundleKey, reload]);

    const filtered = itemOptions.filter((o) => (o.label ?? "").toLowerCase().includes(itemTerm.trim().toLowerCase()));

    const add = useCallback(async () => {
        if (bundleKey === "" || pickedItem === "" || qty.trim() === "") return;
        setSaving(true);
        setError(null);
        try {
            await createComboItem({ comboItemId: Number(bundleKey), itemId: Number(pickedItem), quantity: toDecimal(qty.trim()) });
            setPickedItem("");
            setQty("");
            refresh();
        } catch (cause) {
            setError(messageOf(cause));
        } finally {
            setSaving(false);
        }
    }, [bundleKey, pickedItem, qty, refresh]);

    const remove = useCallback(
        async (id: number) => {
            setError(null);
            try {
                await deleteComboItem(id);
                refresh();
            } catch (cause) {
                setError(messageOf(cause));
            }
        },
        [refresh],
    );

    const bundleName = (id: number): string => names.get(id) ?? `#${id}`;

    return (
        <div className="flex flex-col gap-6 p-6 md:p-8">
            <TableCard.Root>
                <TableCard.Header
                    title="Combos"
                    description="A bundle sells as one line at its own price; the components below are what leave the shelf."
                />

                {state.kind === "failed" ? (
                    <p className="px-6 py-12 text-center text-md text-error-primary">{state.message}</p>
                ) : state.kind === "loading" ? (
                    <p className="px-6 py-12 text-center text-md text-tertiary">Loading…</p>
                ) : (
                    <div className="flex flex-col gap-4 p-6">
                        {error !== null && (
                            <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>
                        )}

                        <Select
                            label="Bundle"
                            items={itemOptions}
                            selectedKey={bundleKey}
                            onSelectionChange={(key) => setBundleKey(String(key ?? ""))}
                            className="w-full md:w-96"
                        >
                            {(row) => (
                                <Select.Item id={row.id} textValue={row.label}>
                                    {row.label}
                                </Select.Item>
                            )}
                        </Select>

                        {bundleKey !== "" && (
                            <>
                                {rows.length === 0 ? (
                                    <p className="text-md text-tertiary">
                                        {bundleName(Number(bundleKey))} has no components yet — it sells as a plain item.
                                    </p>
                                ) : (
                                    <Table>
                                        <Table.Header>
                                            <Table.Head label="Component" />
                                            <Table.Head label="Qty per bundle" className="text-right" />
                                            <Table.Head label="" />
                                        </Table.Header>
                                        <Table.Body>
                                            {rows.map((row) => (
                                                <Table.Row key={row.id} id={row.id}>
                                                    <Table.Cell className="font-medium text-primary">
                                                        {bundleName(row.itemId)}
                                                    </Table.Cell>
                                                    <Table.Cell className="text-right tabular-nums">
                                                        {formatQuantity(row.quantity)}
                                                    </Table.Cell>
                                                    <Table.Cell>
                                                        <div className="flex justify-end">
                                                            <Button
                                                                size="sm"
                                                                color="tertiary"
                                                                iconLeading={Trash01}
                                                                aria-label={`Remove ${bundleName(row.itemId)}`}
                                                                onPress={() => void remove(row.id)}
                                                            />
                                                        </div>
                                                    </Table.Cell>
                                                </Table.Row>
                                            ))}
                                        </Table.Body>
                                    </Table>
                                )}

                                <div className="flex flex-col gap-3 md:flex-row md:items-end">
                                    <Input
                                        label="Find item"
                                        placeholder="Search the catalog"
                                        value={itemTerm}
                                        onChange={(value) => setItemTerm(String(value))}
                                        className="w-full md:w-72"
                                    />
                                    <Select
                                        label="Component"
                                        items={filtered}
                                        selectedKey={pickedItem}
                                        onSelectionChange={(key) => setPickedItem(String(key ?? ""))}
                                        className="w-full md:w-72"
                                    >
                                        {(row) => (
                                            <Select.Item id={row.id} textValue={row.label}>
                                                {row.label}
                                            </Select.Item>
                                        )}
                                    </Select>
                                    <Input
                                        label="Quantity"
                                        placeholder="Per bundle"
                                        value={qty}
                                        onChange={(value) => setQty(String(value))}
                                        className="w-full md:w-40"
                                    />
                                    <Button
                                        onPress={() => void add()}
                                        isLoading={saving}
                                        isDisabled={pickedItem === "" || qty.trim() === ""}
                                    >
                                        <span className="flex items-center gap-1">
                                            <Plus size={16} /> Add
                                        </span>
                                    </Button>
                                </div>
                            </>
                        )}
                    </div>
                )}
            </TableCard.Root>
        </div>
    );
};
