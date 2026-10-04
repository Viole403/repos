import { useCallback, useEffect, useState } from "react";
import { Plus, Trash01 } from "@untitledui/icons";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { Table, TableCard } from "@/components/application/table/table";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { formatMoney, formatQuantity, formatTimestamp } from "@/app/format";
import type { AssetMovement, FixedAssetRow } from "@/app/ipc";
import {
    createFixedAsset,
    deleteFixedAsset,
    listAssetMovements,
    listFixedAssets,
    recordAssetMovement,
} from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

/**
 * Fixed assets — the fridge, the forklift, the till. On hand and cost basis are both
 * derived from the movement ledger, so neither can drift from the history that
 * produced them. A disposal reduces holdings but not cost basis: money spent is
 * money spent.
 */
export const FixedAssets = () => {
    const [rows, setRows] = useState<FixedAssetRow[]>([]);
    const [error, setError] = useState<string | null>(null);
    const [reload, setReload] = useState(0);

    const [creating, setCreating] = useState(false);
    const [name, setName] = useState("");
    const [code, setCode] = useState("");
    const [purchasePrice, setPurchasePrice] = useState("0");
    const [saveError, setSaveError] = useState<string | null>(null);
    const [saving, setSaving] = useState(false);

    const [detail, setDetail] = useState<FixedAssetRow | null>(null);
    const [history, setHistory] = useState<AssetMovement[]>([]);
    const [pendingDelete, setPendingDelete] = useState<FixedAssetRow | null>(null);

    const load = useCallback(async () => {
        try {
            const page = await listFixedAssets({ perPage: 500 });
            setRows(page.rows);
            setError(null);
        } catch (cause) {
            setError(messageOf(cause));
        }
    }, []);

    useEffect(() => {
        void load();
    }, [load, reload]);

    const openDetail = useCallback(async (row: FixedAssetRow) => {
        setDetail(row);
        try {
            const page = await listAssetMovements(row.assetId, { perPage: 50 });
            setHistory(page.rows);
        } catch (cause) {
            setError(messageOf(cause));
        }
    }, []);

    const submit = async (e: React.FormEvent) => {
        e.preventDefault();
        if (!name.trim() || !code.trim()) {
            setSaveError("Name and code are required");
            return;
        }
        setSaving(true);
        setSaveError(null);
        try {
            await createFixedAsset({
                name: name.trim(),
                code: code.trim(),
                purchasePrice,
                salePrice: "0",
            });
            setCreating(false);
            setName("");
            setCode("");
            setPurchasePrice("0");
            setReload((n) => n + 1);
        } catch (cause) {
            setSaveError(messageOf(cause));
        } finally {
            setSaving(false);
        }
    };

    const confirmDelete = async () => {
        if (!pendingDelete) return;
        setPendingDelete(null);
        try {
            await deleteFixedAsset(pendingDelete.assetId);
            setReload((n) => n + 1);
        } catch (cause) {
            setError(messageOf(cause));
        }
    };

    return (
        <div className="flex flex-col gap-5 p-5">
            <div className="flex flex-col items-start justify-between gap-2 md:flex-row md:items-end">
                <div className="flex flex-col gap-1">
                    <h1 className="text-display-xs font-semibold text-primary">Fixed assets</h1>
                    <p className="text-md text-tertiary">Tracked, never sold off the shelf.</p>
                </div>
                <Button color="primary" iconLeading={Plus} onPress={() => setCreating(true)}>
                    New asset
                </Button>
            </div>

            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

            {rows.length === 0 ? (
                <p className="py-10 text-center text-md text-tertiary">No assets yet.</p>
            ) : (
                <TableCard.Root>
                    <TableCard.Header title="Assets" description={`${rows.length} total`} />
                    <Table aria-label="Fixed assets">
                        <Table.Header>
                            <Table.Head label="Asset" />
                            <Table.Head label="Code" />
                            <Table.Head label="On hand" />
                            <Table.Head label="Cost basis" />
                            <Table.Head label="" className="w-12" />
                        </Table.Header>
                        <Table.Body>
                            {rows.map((row) => (
                                <Table.Row key={row.assetId} id={row.assetId}>
                                    <Table.Cell className="font-medium text-primary">{row.name}</Table.Cell>
                                    <Table.Cell className="text-tertiary">{row.code}</Table.Cell>
                                    <Table.Cell className="tabular-nums">{formatQuantity(row.onHand)}</Table.Cell>
                                    <Table.Cell className="tabular-nums">{formatMoney(row.costBasis)}</Table.Cell>
                                    <Table.Cell>
                                        <Button
                                            size="sm"
                                            color="tertiary"
                                            iconLeading={Trash01}
                                            aria-label={`Delete ${row.name}`}
                                            onPress={() => setPendingDelete(row)}
                                        />
                                    </Table.Cell>
                                </Table.Row>
                            ))}
                        </Table.Body>
                    </Table>
                </TableCard.Root>
            )}

            {creating && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setCreating(false)}>
                    <Modal className="max-w-lg">
                        <Dialog className="p-6">
                            <form onSubmit={submit} className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">New asset</h2>
                                {saveError && (
                                    <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{saveError}</p>
                                )}
                                <Input label="Name" value={name} onChange={setName} isRequired />
                                <Input label="Code" value={code} onChange={setCode} isRequired />
                                <Input label="Purchase price" type="number" value={purchasePrice} onChange={setPurchasePrice} />
                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setCreating(false)}>
                                        Cancel
                                    </Button>
                                    <Button type="submit" isLoading={saving}>
                                        Create
                                    </Button>
                                </div>
                            </form>
                        </Dialog>
                    </Modal>
                </ModalOverlay>
            )}

            {detail !== null && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setDetail(null)}>
                    <Modal className="max-w-2xl">
                        <Dialog className="max-h-[85dvh] overflow-y-auto p-6">
                            <div className="flex flex-col gap-4">
                                <div className="flex items-start justify-between gap-3">
                                    <div>
                                        <h2 className="text-display-xs font-semibold text-primary">{detail.name}</h2>
                                        <p className="text-md tabular-nums text-tertiary">
                                            {formatQuantity(detail.onHand)} on hand · cost basis {formatMoney(detail.costBasis)}
                                        </p>
                                    </div>
                                    <Button color="secondary" onPress={() => setDetail(null)}>
                                        Close
                                    </Button>
                                </div>

                                <AssetMoveForm
                                    assetId={detail.assetId}
                                    onPosted={() => {
                                        void load();
                                        void openDetail(detail);
                                    }}
                                />

                                <TableCard.Root>
                                    <TableCard.Header title="History" description="newest first" />
                                    <Table aria-label="Asset history">
                                        <Table.Header>
                                            <Table.Head label="When" />
                                            <Table.Head label="Direction" />
                                            <Table.Head label="Qty" />
                                            <Table.Head label="Unit price" />
                                            <Table.Head label="Amount" />
                                        </Table.Header>
                                        <Table.Body>
                                            {history.map((movement) => (
                                                <Table.Row key={movement.id} id={movement.id}>
                                                    <Table.Cell className="text-tertiary">
                                                        {formatTimestamp(movement.createdAt)}
                                                    </Table.Cell>
                                                    <Table.Cell>{movement.movementKind}</Table.Cell>
                                                    <Table.Cell className="tabular-nums">
                                                        {formatQuantity(movement.quantity)}
                                                    </Table.Cell>
                                                    <Table.Cell className="tabular-nums">
                                                        {formatMoney(movement.unitPrice)}
                                                    </Table.Cell>
                                                    <Table.Cell className="tabular-nums">
                                                        {formatMoney(movement.amount)}
                                                    </Table.Cell>
                                                </Table.Row>
                                            ))}
                                        </Table.Body>
                                    </Table>
                                </TableCard.Root>
                            </div>
                        </Dialog>
                    </Modal>
                </ModalOverlay>
            )}

            {pendingDelete !== null && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setPendingDelete(null)}>
                    <Modal className="max-w-md">
                        <Dialog className="p-6">
                            <div className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">Delete {pendingDelete.name}?</h2>
                                <p className="text-md text-tertiary">Its movement history goes with it.</p>
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

const directions: SelectItemType[] = [
    { id: "In", label: "In — bought, donated, found" },
    { id: "Out", label: "Out — sold, scrapped, written off" },
];

const AssetMoveForm = ({ assetId, onPosted }: { assetId: number; onPosted: () => void }) => {
    const [kind, setKind] = useState("In");
    const [quantity, setQuantity] = useState("");
    const [unitPrice, setUnitPrice] = useState("0");
    const [referenceNo, setReferenceNo] = useState("");
    const [error, setError] = useState<string | null>(null);
    const [busy, setBusy] = useState(false);

    const submit = async (e: React.FormEvent) => {
        e.preventDefault();
        setBusy(true);
        setError(null);
        try {
            await recordAssetMovement({
                assetId,
                quantity,
                unitPrice,
                kind: kind as "In" | "Out",
                referenceNo: referenceNo || null,
            });
            setQuantity("");
            setReferenceNo("");
            onPosted();
        } catch (cause) {
            setError(messageOf(cause));
        } finally {
            setBusy(false);
        }
    };

    return (
        <form onSubmit={submit} className="flex flex-col gap-3 rounded-lg bg-secondary px-4 py-4">
            <h3 className="text-title-sm font-semibold text-primary">Record a movement</h3>
            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}
            <div className="grid gap-3 md:grid-cols-4">
                <Select label="Direction" items={directions} selectedKey={kind} onSelectionChange={(k) => setKind(String(k ?? "In"))}>
                    {(row) => (
                        <Select.Item id={row.id} textValue={row.label}>
                            {row.label}
                        </Select.Item>
                    )}
                </Select>
                <Input label="Quantity" type="number" value={quantity} onChange={setQuantity} isRequired />
                <Input label="Unit price" type="number" value={unitPrice} onChange={setUnitPrice} isRequired />
                <Input label="Reference" value={referenceNo} onChange={setReferenceNo} />
            </div>
            <div className="flex justify-end">
                <Button type="submit" size="sm" isLoading={busy} isDisabled={quantity.trim() === ""}>
                    Post
                </Button>
            </div>
        </form>
    );
};