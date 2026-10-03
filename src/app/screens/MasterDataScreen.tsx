import { useEffect, useState } from "react";
import { Plus, SearchLg, Trash01 } from "@untitledui/icons";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Table, TableCard } from "@/components/application/table/table";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { formatTimestamp } from "@/app/format";
import type { Page, PageQuery } from "@/app/ipc";

/**
 * Shared screen for the master-data tables (units, brands, item categories). They
 * differ only in a label and an optional sort order, so the behaviour lives here
 * once instead of three near-identical files that drift apart.
 */
export interface MasterEntity {
    id: number;
    name: string;
    description: string | null;
    createdAt: string;
}

export interface MasterConfig {
    /** Singular noun, used in error text: "unit". */
    entity: string;
    /** Label for the name field: units call it "Unit name", brands just "Name". */
    nameLabel: string;
    /** Must be a stable module-level reference: it is an effect dependency. */
    load: (query: PageQuery) => Promise<Page<MasterEntity>>;
    create: (input: { name: string; description?: string | null; sortId?: number }) => Promise<unknown>;
    remove: (id: number) => Promise<void>;
    /** Categories carry a sort order; the others do not. */
    withSort?: boolean;
}

type State =
    | { status: "loading" }
    | { status: "error"; message: string }
    | { status: "ready"; rows: MasterEntity[]; total: number };

export const MasterDataScreen = ({ entity, nameLabel, load, create, remove, withSort }: MasterConfig) => {
    const [search, setSearch] = useState("");
    const [term, setTerm] = useState("");
    const [state, setState] = useState<State>({ status: "loading" });
    const [reload, setReload] = useState(0);

    const [creating, setCreating] = useState(false);
    const [name, setName] = useState("");
    const [description, setDescription] = useState("");
    const [sortId, setSortId] = useState("0");
    const [saveError, setSaveError] = useState<string | null>(null);
    const [saving, setSaving] = useState(false);

    const [pendingDelete, setPendingDelete] = useState<MasterEntity | null>(null);

    useEffect(() => {
        const timer = setTimeout(() => setTerm(search.trim()), 250);
        return () => clearTimeout(timer);
    }, [search]);

    useEffect(() => {
        let cancelled = false;
        setState({ status: "loading" });

        load({ page: 1, perPage: 500, search: term || undefined })
            .then((result) => {
                if (!cancelled) setState({ status: "ready", rows: result.rows, total: result.total });
            })
            .catch((error: unknown) => {
                if (!cancelled) setState({ status: "error", message: error instanceof Error ? error.message : String(error) });
            });

        return () => {
            cancelled = true;
        };
    }, [term, reload, load]);

    const resetForm = () => {
        setName("");
        setDescription("");
        setSortId("0");
        setSaveError(null);
    };

    const submit = async (e: React.FormEvent) => {
        e.preventDefault();
        if (!name.trim()) {
            setSaveError(`${nameLabel} is required`);
            return;
        }

        setSaving(true);
        setSaveError(null);
        try {
            await create({ name: name.trim(), description: description.trim() || null, sortId: withSort ? Number(sortId) || 0 : undefined });
            setCreating(false);
            resetForm();
            setReload((n) => n + 1);
        } catch (error) {
            setSaveError(error instanceof Error ? error.message : String(error));
        } finally {
            setSaving(false);
        }
    };

    const confirmDelete = async () => {
        if (!pendingDelete) return;
        const name_ = pendingDelete.name;
        setPendingDelete(null);
        try {
            await remove(pendingDelete.id);
            setReload((n) => n + 1);
        } catch (error) {
            setState({ status: "error", message: `${name_}: ${error instanceof Error ? error.message : String(error)}` });
        }
    };

    return (
        <div className="flex flex-col gap-6 p-6 md:p-8">
            <TableCard.Root>
                <TableCard.Header
                    title={`${entity[0].toUpperCase()}${entity.slice(1)}s`}
                    description={state.status === "ready" ? `${state.total} total` : "Loading…"}
                    contentTrailing={
                        <div className="flex w-full flex-col gap-3 md:flex-row md:w-auto">
                            <Input
                                aria-label={`Search ${entity}s`}
                                icon={SearchLg}
                                placeholder={`Search ${entity}s`}
                                value={search}
                                onChange={setSearch}
                                className="w-full md:w-64"
                            />
                            <Button iconLeading={Plus} onPress={() => setCreating(true)}>
                                New
                            </Button>
                        </div>
                    }
                />

                {state.status === "error" ? (
                    <p className="px-6 py-12 text-center text-md text-error-primary">{state.message}</p>
                ) : state.status === "loading" ? (
                    <p className="px-6 py-12 text-center text-md text-tertiary">Loading…</p>
                ) : state.rows.length === 0 ? (
                    <p className="px-6 py-12 text-center text-md text-tertiary">
                        {term ? `No ${entity}s match “${term}”.` : `No ${entity}s yet.`}
                    </p>
                ) : (
                    <Table>
                        <Table.Header>
                            <Table.Head label={nameLabel} />
                            <Table.Head label="Description" />
                            <Table.Head label="Created" />
                            <Table.Head label="" className="w-16" />
                        </Table.Header>
                        <Table.Body>
                            {state.rows.map((row) => (
                                <Table.Row key={row.id} id={row.id}>
                                    <Table.Cell className="font-medium text-primary">{row.name}</Table.Cell>
                                    <Table.Cell>{row.description ?? "—"}</Table.Cell>
                                    <Table.Cell className="text-tertiary">{formatTimestamp(row.createdAt)}</Table.Cell>
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
                )}
            </TableCard.Root>

            {creating && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setCreating(false)}>
                    <Modal className="max-w-md">
                        <Dialog className="p-6">
                            <form onSubmit={submit} className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">New {entity}</h2>
                                {saveError && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{saveError}</p>}
                                <Input label={nameLabel} value={name} onChange={setName} isRequired />
                                <Input label="Description" value={description} onChange={setDescription} />
                                {withSort && <Input label="Sort order" type="number" value={sortId} onChange={setSortId} />}
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

            {pendingDelete && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setPendingDelete(null)}>
                    <Modal className="max-w-md">
                        <Dialog className="p-6">
                            <div className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">Delete {pendingDelete.name}?</h2>
                                <p className="text-md text-secondary">
                                    It will no longer appear in lists. Existing records that reference it keep working.
                                </p>
                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setPendingDelete(null)}>
                                        Cancel
                                    </Button>
                                    <Button color="primary-destructive" onPress={confirmDelete}>
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