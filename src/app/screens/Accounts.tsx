import { useCallback, useEffect, useState } from "react";
import { Plus, SearchLg, Trash01 } from "@untitledui/icons";
import { Table, TableCard } from "@/components/application/table/table";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { createUser, deleteUser, listRoles, listUsers, setUserPin, setUserRole } from "@/app/ipc";
import type { RoleView, UserView } from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

const blank = { name: "", email: "", password: "", phone: "", role: "" };

/**
 * Accounts and their roles. The role is picked from the roles that exist rather
 * than typed, so a typo cannot become a role nobody holds any permissions through.
 */
export const Accounts = () => {
    const [rows, setRows] = useState<UserView[]>([]);
    const [roles, setRoles] = useState<RoleView[]>([]);
    const [loadError, setLoadError] = useState<string | null>(null);
    const [actionError, setActionError] = useState<string | null>(null);
    const [term, setTerm] = useState("");
    const [reload, setReload] = useState(0);

    const [creating, setCreating] = useState(false);
    const [form, setForm] = useState(blank);
    const [saving, setSaving] = useState(false);
    const [pendingDelete, setPendingDelete] = useState<UserView | null>(null);
    const [pinFor, setPinFor] = useState<UserView | null>(null);
    const [pin, setPin] = useState("");
    const [pinError, setPinError] = useState<string | null>(null);

    const refresh = useCallback(async () => {
        try {
            const [accounts, available] = await Promise.all([listUsers(), listRoles()]);
            setRows(accounts);
            setRoles(available);
            setLoadError(null);
        } catch (cause) {
            setLoadError(messageOf(cause));
        }
    }, []);

    useEffect(() => {
        void refresh();
    }, [refresh, reload]);

    // `list_users` is unpaginated and takes no search parameter, so filtering happens
    // here rather than paying a round trip that returns the same rows.
    const visible = term.trim()
        ? rows.filter((row) => `${row.name} ${row.email}`.toLowerCase().includes(term.trim().toLowerCase()))
        : rows;

    const roleOptions: SelectItemType[] = [
        { id: "", label: "No role" },
        ...roles.map((role) => ({ id: String(role.id), label: role.name })),
    ];

    const roleIdOf = (name: string | null) => roles.find((role) => role.name === name)?.id.toString() ?? "";

    const save = async () => {
        setSaving(true);
        setActionError(null);
        try {
            await createUser({
                name: form.name.trim(),
                email: form.email.trim(),
                password: form.password,
                phone: form.phone.trim() || null,
                role: form.role || null,
            });
            setCreating(false);
            setForm(blank);
            setReload((n) => n + 1);
        } catch (cause) {
            setActionError(messageOf(cause));
        } finally {
            setSaving(false);
        }
    };

    const assign = async (userId: number, key: string) => {
        if (!key) return;
        setActionError(null);
        try {
            await setUserRole(userId, Number(key));
            setReload((n) => n + 1);
        } catch (cause) {
            setActionError(messageOf(cause));
            setReload((n) => n + 1);
        }
    };

    const confirmDelete = async () => {
        if (!pendingDelete) return;
        setPendingDelete(null);
        setActionError(null);
        try {
            await deleteUser(pendingDelete.id);
        } catch (cause) {
            setActionError(messageOf(cause));
        } finally {
            setReload((n) => n + 1);
        }
    };

    const savePin = async () => {
        if (!pinFor) return;
        setPinError(null);
        setActionError(null);
        try {
            await setUserPin(pinFor.id, pin);
            setPinFor(null);
            setPin("");
            setReload((n) => n + 1);
        } catch (cause) {
            setPinError(messageOf(cause));
        }
    };

    return (
        <div className="flex flex-col gap-5 p-5">
            <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
                <div className="flex flex-col gap-1">
                    <h1 className="text-display-xs font-semibold text-primary">Accounts</h1>
                    <p className="text-md text-tertiary">Who can sign in to this till, and what each of them may do.</p>
                </div>
                <div className="flex w-full flex-col gap-3 md:w-auto md:flex-row">
                    <Input
                        aria-label="Search accounts"
                        icon={SearchLg}
                        placeholder="Search name or email"
                        value={term}
                        onChange={setTerm}
                        className="w-full md:w-72"
                    />
                    <Button size="md" iconLeading={Plus} onPress={() => setCreating(true)}>
                        New account
                    </Button>
                </div>
            </div>

            {loadError && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{loadError}</p>}
            {actionError && (
                <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{actionError}</p>
            )}

            {rows.length === 0 ? (
                <p className="py-10 text-center text-md text-tertiary">No accounts.</p>
            ) : (
                <TableCard.Root>
                    <TableCard.Header title="Accounts" description={`${visible.length} of ${rows.length}`} />
                    <Table aria-label="Accounts">
                        <Table.Header>
                            <Table.Head label="Name" />
                            <Table.Head label="Email" />
                            <Table.Head label="Role" />
                            <Table.Head label="" className="w-16" />
                        </Table.Header>
                        <Table.Body>
                            {visible.map((row) => (
                                <Table.Row key={row.id} id={row.id}>
                                    <Table.Cell className="font-medium text-primary">{row.name}</Table.Cell>
                                    <Table.Cell>{row.email}</Table.Cell>
                                    <Table.Cell>
                                        <Select
                                            aria-label={`Role for ${row.name}`}
                                            items={roleOptions}
                                            selectedKey={roleIdOf(row.role)}
                                            onSelectionChange={(key) => void assign(row.id, String(key))}
                                            className="min-w-44"
                                        >
                                            {(option) => (
                                                <Select.Item id={option.id} textValue={option.label}>
                                                    {option.label}
                                                </Select.Item>
                                            )}
                                        </Select>
                                    </Table.Cell>
                                    <Table.Cell>
                                        <div className="flex items-center gap-1">
                                            <Button
                                                size="sm"
                                                color="tertiary"
                                                aria-label={`Set approval PIN for ${row.name}`}
                                                onPress={() => {
                                                    setPinFor(row);
                                                    setPin("");
                                                    setPinError(null);
                                                }}
                                            >
                                                PIN
                                            </Button>
                                            <Button
                                                size="sm"
                                                color="tertiary"
                                                iconLeading={Trash01}
                                                aria-label={`Delete ${row.name}`}
                                                onPress={() => setPendingDelete(row)}
                                            />
                                        </div>
                                    </Table.Cell>
                                </Table.Row>
                            ))}
                        </Table.Body>
                    </Table>
                </TableCard.Root>
            )}

            {creating && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setCreating(false)}>
                    <Modal className="max-w-md">
                        <Dialog className="p-6">
                            <form
                                className="flex flex-col gap-4"
                                onSubmit={(event) => {
                                    event.preventDefault();
                                    void save();
                                }}
                            >
                                <h2 className="text-display-xs font-semibold text-primary">New account</h2>
                                <Input
                                    label="Name"
                                    value={form.name}
                                    onChange={(v) => setForm({ ...form, name: String(v) })}
                                    isRequired
                                    isDisabled={saving}
                                />
                                <Input
                                    label="Email"
                                    type="email"
                                    value={form.email}
                                    onChange={(v) => setForm({ ...form, email: String(v) })}
                                    isRequired
                                    isDisabled={saving}
                                />
                                <Input
                                    label="Password"
                                    type="password"
                                    autoComplete="new-password"
                                    value={form.password}
                                    onChange={(v) => setForm({ ...form, password: String(v) })}
                                    isRequired
                                    isDisabled={saving}
                                />
                                <Input
                                    label="Phone"
                                    value={form.phone}
                                    onChange={(v) => setForm({ ...form, phone: String(v) })}
                                    isDisabled={saving}
                                />
                                <Select
                                    label="Role"
                                    items={roleOptions}
                                    selectedKey={roleOptions.find((o) => o.label === form.role)?.id ?? ""}
                                    onSelectionChange={(key) =>
                                        setForm({
                                            ...form,
                                            role: roleOptions.find((o) => o.id === String(key))?.label ?? "",
                                        })
                                    }
                                    isDisabled={saving}
                                >
                                    {(option) => (
                                        <Select.Item id={option.id} textValue={option.label}>
                                            {option.label}
                                        </Select.Item>
                                    )}
                                </Select>
                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setCreating(false)}>
                                        Cancel
                                    </Button>
                                    <Button type="submit" isLoading={saving}>
                                        Create account
                                    </Button>
                                </div>
                            </form>
                        </Dialog>
                    </Modal>
                </ModalOverlay>
            )}

            {pinFor && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setPinFor(null)}>
                    <Modal className="max-w-md">
                        <Dialog className="p-6">
                            <div className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">
                                    Approval PIN for {pinFor.name}
                                </h2>
                                <p className="text-md text-secondary">
                                    At least 4 digits. This PIN approves discounts and returns — give it
                                    only to someone who may approve sales.
                                </p>

                                {pinError !== null && (
                                    <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">
                                        {pinError}
                                    </p>
                                )}

                                <Input
                                    label="PIN"
                                    type="password"
                                    inputMode="numeric"
                                    value={pin}
                                    onChange={setPin}
                                    autoFocus
                                />

                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setPinFor(null)}>
                                        Cancel
                                    </Button>
                                    <Button color="primary" onPress={() => void savePin()}>
                                        Set PIN
                                    </Button>
                                </div>
                            </div>
                        </Dialog>
                    </Modal>
                </ModalOverlay>
            )}

            {pendingDelete && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setPendingDelete(null)}>
                    <Modal className="max-w-md">
                        <Dialog className="p-6">
                            <div className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">
                                    Delete {pendingDelete.name}?
                                </h2>
                                <p className="text-md text-secondary">
                                    The account will no longer be able to sign in. Records it created keep its name.
                                </p>
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