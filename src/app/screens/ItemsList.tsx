import { useEffect, useState } from "react";
import { Plus, SearchLg } from "@untitledui/icons";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Table, TableCard, TableRowActionsDropdown } from "@/components/application/table/table";
import { ItemForm } from "./ItemForm";
import { formatMoney, formatQuantity } from "@/app/format";
import type { ItemView } from "@/app/ipc";
import { listItems } from "@/app/ipc";

const PER_PAGE = 20;

type State =
    | { status: "loading" }
    | { status: "error"; message: string }
    | { status: "ready"; rows: ItemView[]; total: number };

export const ItemsList = () => {
    const [search, setSearch] = useState("");
    const [term, setTerm] = useState("");
    const [page, setPage] = useState(1);
    const [state, setState] = useState<State>({ status: "loading" });
    const [editing, setEditing] = useState<ItemView | null | undefined>(undefined);
    // Bumped after a write so the fetch effect reruns; state alone would not.
    const [reload, setReload] = useState(0);

    // Typing should not fire a query per keystroke; page resets so results stay in view.
    useEffect(() => {
        const timer = setTimeout(() => {
            setTerm(search.trim());
            setPage(1);
        }, 250);
        return () => clearTimeout(timer);
    }, [search]);

    useEffect(() => {
        let cancelled = false;
        setState({ status: "loading" });

        listItems({ page, perPage: PER_PAGE, search: term || undefined })
            .then((result) => {
                if (cancelled) return;
                setState({ status: "ready", rows: result.rows, total: result.total });
            })
            .catch((error: unknown) => {
                if (cancelled) return;
                setState({ status: "error", message: error instanceof Error ? error.message : String(error) });
            });

        return () => {
            cancelled = true;
        };
    }, [page, term, reload]);

    const pages = state.status === "ready" ? Math.max(1, Math.ceil(state.total / PER_PAGE)) : 1;

    return (
        <div className="flex flex-col gap-6 p-6 md:p-8">
            <TableCard.Root>
                <TableCard.Header
                    title="Items"
                    description={state.status === "ready" ? `${state.total} in catalog` : "Loading catalog…"}
                    contentTrailing={
                        <div className="flex w-full flex-col gap-3 md:flex-row md:w-auto">
                            <Input
                                aria-label="Search items"
                                icon={SearchLg}
                                placeholder="Search name or code"
                                value={search}
                                onChange={setSearch}
                                className="w-full md:w-72"
                            />
                            <Button size="md" iconLeading={Plus} onPress={() => setEditing(null)}>
                                New item
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
                        {term ? `No items match “${term}”.` : "No items yet."}
                    </p>
                ) : (
                    <Table>
                        <Table.Header>
                            <Table.Head label="Code" />
                            <Table.Head label="Name" />
                            <Table.Head label="Category" />
                            <Table.Head label="Brand" />
                            <Table.Head label="Sale unit" />
                            <Table.Head label="Purchase" className="text-right" />
                            <Table.Head label="Sale" className="text-right" />
                            <Table.Head label="" className="w-16" />
                        </Table.Header>
                        <Table.Body>
                            {state.rows.map((item) => (
                                <Table.Row key={item.id} id={item.id}>
                                    <Table.Cell className="font-medium text-primary">{item.code}</Table.Cell>
                                    <Table.Cell>
                                        <span className="block truncate max-w-64">{item.name}</span>
                                        {item.alternativeName && <span className="text-sm text-tertiary">{item.alternativeName}</span>}
                                    </Table.Cell>
                                    <Table.Cell>{item.categoryName ?? "—"}</Table.Cell>
                                    <Table.Cell>{item.brandName ?? "—"}</Table.Cell>
                                    <Table.Cell>
                                        {item.saleUnitName ?? "—"}
                                        {Number(item.conversionRate) !== 1 && (
                                            <span className="block text-sm text-tertiary">
                                                1 = {formatQuantity(item.conversionRate)} {item.purchaseUnitName ?? "purchase units"}
                                            </span>
                                        )}
                                    </Table.Cell>
                                    <Table.Cell className="text-right">{formatMoney(item.purchasePrice)}</Table.Cell>
                                    <Table.Cell className="text-right font-medium text-primary">{formatMoney(item.salePrice)}</Table.Cell>
                                    <Table.Cell>
                                        <TableRowActionsDropdown onEdit={() => setEditing(item)} />
                                    </Table.Cell>
                                </Table.Row>
                            ))}
                        </Table.Body>
                    </Table>
                )}
            </TableCard.Root>

            {editing !== undefined && (
                <ItemForm
                    {...(editing ? { item: editing } : {})}
                    onClose={() => setEditing(undefined)}
                    onSaved={() => {
                        setEditing(undefined);
                        // Stay on page 1 after a create so the new row is visible.
                        setPage(1);
                        setReload((n) => n + 1);
                    }}
                />
            )}

            <div className="flex items-center justify-between">
                <p className="text-sm text-tertiary">
                    {state.status === "ready" ? `Page ${page} of ${pages}` : " "}
                </p>
                <div className="flex gap-2">
                    <Button size="sm" color="secondary" onPress={() => setPage((p) => Math.max(1, p - 1))} isDisabled={page <= 1}>
                        Previous
                    </Button>
                    <Button size="sm" color="secondary" onPress={() => setPage((p) => p + 1)} isDisabled={page >= pages}>
                        Next
                    </Button>
                </div>
            </div>
        </div>
    );
};