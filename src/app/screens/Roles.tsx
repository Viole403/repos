import { useCallback, useEffect, useState } from "react";
import { Plus, Trash01 } from "@untitledui/icons";
import { Table, TableCard } from "@/components/application/table/table";
import { Button } from "@/components/base/buttons/button";
import { Checkbox } from "@/components/base/checkbox/checkbox";
import { Input } from "@/components/base/input/input";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { createRole, deleteRole, listRoles, setRolePermissions } from "@/app/ipc";
import type { RoleView } from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

const isOwner = (role: RoleView) => role.roleType === "Master";

/** Groups the flat `group-action` names the catalog seeds, for the picker layout. */
const byGroup = (names: string[]) => {
    const groups = new Map<string, string[]>();
    for (const name of names) {
        const group = name.includes("-") ? name.slice(0, name.indexOf("-")) : "other";
        groups.set(group, [...(groups.get(group) ?? []), name]);
    }
    return [...groups.entries()].sort(([a], [b]) => a.localeCompare(b));
};

export const Roles = () => {
    const [roles, setRoles] = useState<RoleView[]>([]);
    const [available, setAvailable] = useState<string[]>([]);
    const [error, setError] = useState<string | null>(null);
    const [reload, setReload] = useState(0);

    const [name, setName] = useState("");
    const [creating, setCreating] = useState(false);
    const [editing, setEditing] = useState<RoleView | null>(null);
    const [picked, setPicked] = useState<string[]>([]);
    const [pendingDelete, setPendingDelete] = useState<RoleView | null>(null);
    const [busy, setBusy] = useState(false);

    const refresh = useCallback(async () => {
        try {
            const list = await listRoles();
            setRoles(list);
            setError(null);
        } catch (cause) {
            setError(messageOf(cause));
        }
    }, []);

    useEffect(() => {
        void refresh();
    }, [refresh, reload]);

    // Every seeded permission, read off the owner role: it bypasses the pivot, so it
    // is the one role whose list is the complete catalog.
    useEffect(() => {
        void listRoles()
            .then((list) => {
                const owner = list.find(isOwner);
                setAvailable(owner?.permissions ?? []);
            })
            .catch((cause) => setError(messageOf(cause)));
    }, []);

    const create = async () => {
        setBusy(true);
        setError(null);
        try {
            await createRole(name.trim());
            setName("");
            setCreating(false);
            setReload((n) => n + 1);
        } catch (cause) {
            setError(messageOf(cause));
        } finally {
            setBusy(false);
        }
    };

    const savePermissions = async () => {
        if (!editing) return;
        setBusy(true);
        setError(null);
        try {
            await setRolePermissions(editing.id, picked);
            setEditing(null);
            setReload((n) => n + 1);
        } catch (cause) {
            setError(messageOf(cause));
        } finally {
            setBusy(false);
        }
    };

    const confirmDelete = async () => {
        if (!pendingDelete) return;
        setBusy(true);
        setError(null);
        try {
            await deleteRole(pendingDelete.id);
            setPendingDelete(null);
            setReload((n) => n + 1);
        } catch (cause) {
            setError(messageOf(cause));
            setPendingDelete(null);
        } finally {
            setBusy(false);
        }
    };

    return (
        <div className="flex flex-col gap-5 p-5">
            <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
                <div className="flex flex-col gap-1">
                    <h1 className="text-display-xs font-semibold text-primary">Roles</h1>
                    <p className="text-md text-tertiary">
                        What each account may do. A permission named here is enforced by the backend, not the screen.
                    </p>
                </div>
                <Button size="md" iconLeading={Plus} onPress={() => setCreating(true)} className="w-full md:w-auto">
                    New role
                </Button>
            </div>

            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

            {roles.length === 0 ? (
                <p className="py-10 text-center text-md text-tertiary">No roles.</p>
            ) : (
                <TableCard.Root>
                    <TableCard.Header title="Roles" description={`${roles.length}`} />
                    <Table aria-label="Roles">
                        <Table.Header>
                            <Table.Head label="Name" />
                            <Table.Head label="Type" />
                            <Table.Head label="Permissions" />
                            <Table.Head label="" className="w-16" />
                        </Table.Header>
                        <Table.Body>
                            {roles.map((role) => (
                                <Table.Row key={role.id} id={role.id}>
                                    <Table.Cell className="font-medium text-primary">{role.name}</Table.Cell>
                                    <Table.Cell>
                                        {isOwner(role) ? (
                                            <span className="text-tertiary">Owner — all permissions</span>
                                        ) : (
                                            <span className="text-tertiary">{role.permissions.length}</span>
                                        )}
                                    </Table.Cell>
                                    <Table.Cell>
                                        {isOwner(role) ? (
                                            <span className="text-tertiary">—</span>
                                        ) : (
                                            <Button
                                                size="sm"
                                                color="secondary"
                                                onPress={() => {
                                                    setEditing(role);
                                                    setPicked(role.permissions);
                                                }}
                                            >
                                                Edit permissions
                                            </Button>
                                        )}
                                    </Table.Cell>
                                    <Table.Cell>
                                        {!isOwner(role) && (
                                            <Button
                                                size="sm"
                                                color="tertiary"
                                                iconLeading={Trash01}
                                                aria-label={`Delete ${role.name}`}
                                                onPress={() => setPendingDelete(role)}
                                            />
                                        )}
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
                                    void create();
                                }}
                            >
                                <h2 className="text-display-xs font-semibold text-primary">New role</h2>
                                <p className="text-md text-secondary">
                                    It grants nothing until you give it permissions.
                                </p>
                                <Input
                                    label="Name"
                                    value={name}
                                    onChange={setName}
                                    isRequired
                                    isDisabled={busy}
                                />
                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setCreating(false)}>
                                        Cancel
                                    </Button>
                                    <Button type="submit" isLoading={busy}>
                                        Create role
                                    </Button>
                                </div>
                            </form>
                        </Dialog>
                    </Modal>
                </ModalOverlay>
            )}

            {editing && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setEditing(null)}>
                    <Modal className="max-w-xl">
                        <Dialog className="p-6">
                            <div className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">
                                    {editing.name} permissions
                                </h2>
                                <div className="flex max-h-96 flex-col gap-4 overflow-y-auto">
                                    {byGroup(available).map(([group, names]) => (
                                        <fieldset key={group} className="flex flex-col gap-2">
                                            <legend className="text-sm font-semibold text-primary">{group}</legend>
                                            {names.map((permission) => (
                                                <Checkbox
                                                    key={permission}
                                                    label={permission}
                                                    isSelected={picked.includes(permission)}
                                                    onChange={(selected) =>
                                                        setPicked((current) =>
                                                            selected
                                                                ? [...current, permission]
                                                                : current.filter((p) => p !== permission),
                                                        )
                                                    }
                                                />
                                            ))}
                                        </fieldset>
                                    ))}
                                </div>
                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setEditing(null)}>
                                        Cancel
                                    </Button>
                                    <Button onPress={() => void savePermissions()} isLoading={busy}>
                                        Save
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
                                    Accounts still holding it must be moved first.
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