import { useCallback, useEffect, useState } from "react";
import { SearchLg } from "@untitledui/icons";
import { Table, TableCard } from "@/components/application/table/table";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { formatMoney, formatTimestamp } from "@/app/format";
import { listSales } from "@/app/ipc";
import type { SaleSummary } from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

const STATUSES: SelectItemType[] = [
    { id: "", label: "Any status" },
    { id: "Completed", label: "Completed" },
    { id: "Draft", label: "Draft" },
];

const today = () => new Date().toISOString().slice(0, 10);

export const Sales = () => {
    const [rows, setRows] = useState<SaleSummary[]>([]);
    const [total, setTotal] = useState(0);
    const [page, setPage] = useState(1);
    const [status, setStatus] = useState("");
    const [from, setFrom] = useState(today());
    const [to, setTo] = useState(today());
    const [error, setError] = useState<string | null>(null);

    const perPage = 20;

    const refresh = useCallback(async () => {
        try {
            const found = await listSales(
                { status: status || null, from: from || null, to: to || null },
                { page, perPage },
            );
            setRows(found.rows);
            setTotal(found.total);
            setError(null);
        } catch (cause) {
            setError(messageOf(cause));
        }
    }, [page, status, from, to]);

    useEffect(() => {
        void refresh();
    }, [refresh]);

    const pages = Math.max(1, Math.ceil(total / perPage));

    return (
        <div className="flex flex-col gap-5 p-5">
            <div className="flex flex-col gap-1">
                <h1 className="text-display-xs font-semibold text-primary">All sales</h1>
                <p className="text-md text-tertiary">Every sale the till has written, newest first.</p>
            </div>

            <div className="flex flex-col gap-3 md:flex-row md:items-end">
                <Select
                    label="Status"
                    items={STATUSES}
                    selectedKey={status}
                    onSelectionChange={(key) => {
                        setPage(1);
                        setStatus(String(key ?? ""));
                    }}
                    className="w-full md:w-44"
                >
                    {(row) => (
                        <Select.Item id={row.id} textValue={row.label}>
                            {row.label}
                        </Select.Item>
                    )}
                </Select>
                <Input
                    label="From"
                    type="date"
                    value={from}
                    onChange={(value) => {
                        setPage(1);
                        setFrom(String(value));
                    }}
                    className="w-full md:w-44"
                />
                <Input
                    label="To"
                    type="date"
                    value={to}
                    onChange={(value) => {
                        setPage(1);
                        setTo(String(value));
                    }}
                    className="w-full md:w-44"
                />
                {/* Defaults to today, so a busy shop opens on the day's takings rather
                    than on its whole history. */}
                <Button
                    color="secondary"
                    iconLeading={SearchLg}
                    onPress={() => {
                        setFrom("");
                        setTo("");
                        setPage(1);
                    }}
                >
                    All time
                </Button>
            </div>

            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

            {rows.length === 0 ? (
                <p className="py-10 text-center text-md text-tertiary">No sales in that range.</p>
            ) : (
                <TableCard.Root>
                    <TableCard.Header title="Sales" description={`${total} total`} />
                    <Table aria-label="Sales">
                        <Table.Header>
                            <Table.Head label="Invoice" />
                            <Table.Head label="When" />
                            <Table.Head label="Customer" />
                            <Table.Head label="Method" />
                            <Table.Head label="Total" />
                            <Table.Head label="Status" />
                        </Table.Header>
                        <Table.Body>
                            {rows.map((row) => (
                                <Table.Row key={row.id} id={row.id}>
                                    <Table.Cell className="font-medium tabular-nums text-primary">
                                        {row.invoiceNo}
                                    </Table.Cell>
                                    <Table.Cell className="text-tertiary">{formatTimestamp(row.createdAt)}</Table.Cell>
                                    <Table.Cell>{row.customerName ?? "Walk-in"}</Table.Cell>
                                    <Table.Cell>{row.paymentMethod}</Table.Cell>
                                    <Table.Cell className="tabular-nums">{formatMoney(row.grandTotal)}</Table.Cell>
                                    <Table.Cell className="text-tertiary">{row.status}</Table.Cell>
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
        </div>
    );
};