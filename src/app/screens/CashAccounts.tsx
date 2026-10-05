import { useCallback, useEffect, useState } from "react";
import { Table, TableCard } from "@/components/application/table/table";
import { Button } from "@/components/base/buttons/button";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { decIsNegative } from "@/app/decimal";
import { formatMoney } from "@/app/format";
import { accountBalances, accountStatement } from "@/app/ipc";
import type { AccountBalance, CashBookLine } from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

export const CashAccounts = () => {
    const [balances, setBalances] = useState<AccountBalance[]>([]);
    const [error, setError] = useState<string | null>(null);
    const [openId, setOpenId] = useState<number | null>(null);
    const [lines, setLines] = useState<CashBookLine[]>([]);

    const refresh = useCallback(async () => {
        try {
            setBalances(await accountBalances());
            setError(null);
        } catch (cause) {
            setError(messageOf(cause));
        }
    }, []);

    useEffect(() => {
        void refresh();
    }, [refresh]);

    const open = useCallback(async (id: number) => {
        setOpenId(id);
        setLines([]);
        try {
            setLines(await accountStatement(id));
        } catch (cause) {
            setError(messageOf(cause));
        }
    }, []);

    const openAccount = balances.find((b) => b.paymentMethodId === openId);

    return (
        <div className="flex flex-col gap-5 p-5">
            <div className="flex flex-col gap-1">
                <h1 className="text-display-xs font-semibold text-primary">Accounts</h1>
                <p className="text-md text-tertiary">
                    What each tender holds. Sales, receipts, supplier payments, income, expense and
                    transfers all count — nothing here is stored, it is the sum over what moved.
                </p>
            </div>

            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

            {balances.length === 0 ? (
                <p className="py-10 text-center text-md text-tertiary">
                    No payment methods yet. Add one under Settings before recording anything.
                </p>
            ) : (
                <TableCard.Root>
                    <TableCard.Header title="Balances" description={`${balances.length} accounts`} />
                    <Table aria-label="Account balances">
                        <Table.Header>
                            <Table.Head label="Account" />
                            <Table.Head label="Kind" />
                            <Table.Head label="In the drawer" />
                            <Table.Head label="Balance" className="text-right" />
                            <Table.Head label="" className="w-28" />
                        </Table.Header>
                        <Table.Body>
                            {balances.map((row) => (
                                <Table.Row key={row.paymentMethodId} id={row.paymentMethodId}>
                                    <Table.Cell className="font-medium text-primary">
                                        {row.paymentMethodName}
                                    </Table.Cell>
                                    <Table.Cell>{row.kind}</Table.Cell>
                                    <Table.Cell>{row.kind === "Cash" ? "yes" : "no"}</Table.Cell>
                                    <Table.Cell
                                        className={`text-right ${decIsNegative(row.balance) ? "text-error-primary" : undefined}`}
                                    >
                                        {formatMoney(row.balance)}
                                    </Table.Cell>
                                    <Table.Cell>
                                        <Button size="sm" color="secondary" onPress={() => void open(row.paymentMethodId)}>
                                            Statement
                                        </Button>
                                    </Table.Cell>
                                </Table.Row>
                            ))}
                        </Table.Body>
                    </Table>
                </TableCard.Root>
            )}

            {openId !== null && (
                <ModalOverlay isOpen onOpenChange={(isOpen) => !isOpen && setOpenId(null)}>
                    <Modal className="max-w-3xl">
                        <Dialog className="max-h-[85dvh] overflow-y-auto p-6">
                            <div className="flex flex-col gap-4">
                                <div className="flex flex-col gap-1">
                                    <h2 className="text-display-xs font-semibold text-primary">
                                        {openAccount?.paymentMethodName ?? "Account"}
                                    </h2>
                                    <p className="text-md text-tertiary">
                                        Every movement, oldest first. A negative amount is money leaving.
                                    </p>
                                </div>
                                {lines.length === 0 ? (
                                    <p className="text-md text-tertiary">Nothing has moved through this account yet.</p>
                                ) : (
                                    <Table aria-label="Account statement">
                                        <Table.Header>
                                            <Table.Head label="Reference" />
                                            <Table.Head label="What" />
                                            <Table.Head label="Category" />
                                            <Table.Head label="Date" />
                                            <Table.Head label="Amount" className="text-right" />
                                        </Table.Header>
                                        <Table.Body>
                                            {lines.map((line, index) => (
                                                <Table.Row key={`${line.referenceNo}-${index}`}>
                                                    <Table.Cell>{line.referenceNo || "—"}</Table.Cell>
                                                    <Table.Cell>{line.kind}</Table.Cell>
                                                    <Table.Cell>{line.category ?? "—"}</Table.Cell>
                                                    <Table.Cell>{line.occurredAt}</Table.Cell>
                                                    <Table.Cell
                                                        className={`text-right ${decIsNegative(line.signed) ? "text-error-primary" : undefined}`}
                                                    >
                                                        {formatMoney(line.signed)}
                                                    </Table.Cell>
                                                </Table.Row>
                                            ))}
                                        </Table.Body>
                                    </Table>
                                )}
                                <div className="flex justify-end">
                                    <Button color="secondary" onPress={() => setOpenId(null)}>
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
