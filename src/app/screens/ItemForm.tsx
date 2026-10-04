import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { toDecimal } from "@/app/format";
import type { ItemInput, ItemSubCategory, ItemView } from "@/app/ipc";
import { createItem, listBrands, listItemCategories, listItemSubCategories, listUnits, SYMBOLOGIES, updateItem } from "@/app/ipc";

/** Decimal fields stay as text so the exact digits the user typed reach Rust. */
interface FormState {
    name: string;
    code: string;
    alternativeName: string;
    genericName: string;
    description: string;
    categoryId: string;
    subCategoryId: string;
    brandId: string;
    purchaseUnitId: string;
    saleUnitId: string;
    conversionRate: string;
    purchasePrice: string;
    salePrice: string;
    wholeSalePrice: string;
    alertQuantity: string;
    loyaltyPoint: string;
    photo: string;
    symbology: string;
    weighed: string;
}

const empty: FormState = {
    name: "",
    code: "",
    alternativeName: "",
    genericName: "",
    description: "",
    categoryId: "",
    subCategoryId: "",
    brandId: "",
    purchaseUnitId: "",
    saleUnitId: "",
    conversionRate: "1",
    purchasePrice: "0",
    salePrice: "0",
    wholeSalePrice: "",
    alertQuantity: "",
    loyaltyPoint: "0",
    photo: "",
    symbology: "",
    weighed: "no",
};

const fromItem = (item: ItemView): FormState => ({
    name: item.name,
    code: item.code,
    alternativeName: item.alternativeName ?? "",
    genericName: item.genericName ?? "",
    description: item.description ?? "",
    categoryId: item.categoryId === null ? "" : String(item.categoryId),
    subCategoryId: item.subCategoryId === null ? "" : String(item.subCategoryId),
    brandId: item.brandId === null ? "" : String(item.brandId),
    purchaseUnitId: item.purchaseUnitId === null ? "" : String(item.purchaseUnitId),
    saleUnitId: item.saleUnitId === null ? "" : String(item.saleUnitId),
    conversionRate: item.conversionRate,
    purchasePrice: item.purchasePrice,
    salePrice: item.salePrice,
    wholeSalePrice: item.wholeSalePrice ?? "",
    alertQuantity: item.alertQuantity ?? "",
    loyaltyPoint: item.loyaltyPoint,
    photo: item.photo ?? "",
    symbology: item.symbology ?? "",
    weighed: item.weighed ? "yes" : "no",
});

type Errors = Partial<Record<keyof FormState, string>>;

/**
 * Mirrors `validate()` in commands.rs. Duplicated deliberately: the server stays the
 * authority, and a rule expressed in only one place means either the form accepts
 * what the command rejects, or it blocks what the command would accept.
 */
const validate = (form: FormState): Errors => {
    const errors: Errors = {};
    if (!form.name.trim()) errors.name = "Item name is required";
    if (!form.code.trim()) errors.code = "Item code is required";

    // Sale below cost is legal (clearance); below zero never is.
    for (const [field, label] of [
        ["purchasePrice", "Purchase price"],
        ["salePrice", "Sale price"],
    ] as const) {
        const raw = form[field].trim();
        if (raw === "") continue;
        if (!Number.isFinite(Number(raw))) errors[field] = `${label} must be a number`;
        else if (Number(raw) < 0) errors[field] = `${label.toLowerCase()} cannot be negative`;
    }

    const rate = Number(form.conversionRate);
    if (!form.conversionRate.trim() || !Number.isFinite(rate) || rate <= 0) {
        errors.conversionRate = "Conversion rate must be greater than zero";
    }
    return errors;
};

/** Blank text becomes null rather than an empty string the server has to re-trim. */
const optionalText = (raw: string): string | null => (raw.trim() === "" ? null : raw.trim());
/** Blank decimal becomes null; anything else keeps the exact digits typed. */
const optionalDecimal = (raw: string): string | null => (raw.trim() === "" ? null : toDecimal(raw));
const optionalId = (raw: string): number | null => (raw.trim() === "" ? null : Number(raw));

interface ItemFormProps {
    /** Present when editing; omit to create. */
    item?: ItemView;
    onClose: () => void;
    onSaved: () => void;
}

export const ItemForm = ({ item, onClose, onSaved }: ItemFormProps) => {
    const [form, setForm] = useState<FormState>(() => (item ? fromItem(item) : empty));
    const [errors, setErrors] = useState<Errors>({});
    const [submitError, setSubmitError] = useState<string | null>(null);
    const [saving, setSaving] = useState(false);

    const [categories, setCategories] = useState<SelectItemType[]>([]);
    const [subCategories, setSubCategories] = useState<ItemSubCategory[]>([]);
    const [brands, setBrands] = useState<SelectItemType[]>([]);
    const [units, setUnits] = useState<SelectItemType[]>([]);

    const set = (field: keyof FormState) => (value: string) => {
        setForm((f) => ({ ...f, [field]: value }));
        setErrors((e) => ({ ...e, [field]: undefined }));
    };

    useEffect(() => {
        // The foreign keys need options; an empty catalog is fine, the selects just
        // offer nothing and the field stays unset.
        Promise.all([
            listItemCategories({ perPage: 500 }),
            listItemSubCategories(null, { perPage: 500 }),
            listBrands({ perPage: 500 }),
            listUnits({ perPage: 500 }),
        ])
            .then(([c, s, b, u]) => {
                setCategories(c.rows.map((row) => ({ id: row.id, label: row.name })));
                setSubCategories(s.rows);
                setBrands(b.rows.map((row) => ({ id: row.id, label: row.name })));
                setUnits(u.rows.map((row) => ({ id: row.id, label: row.unitName })));
            })
            .catch(() => {
                setSubmitError("Could not load categories, brands and units.");
            });
    }, []);

    // A sub-category belongs to one category, so the picker narrows to the chosen
    // parent's children and clears itself when the parent changes.
    const subCategoryOptions = useMemo(() => {
        const options = subCategories.map((row) => ({ id: row.id, label: row.name }));
        if (form.categoryId === "") return options;
        const parent = Number(form.categoryId);
        return options.filter((option) => {
            const match = subCategories.find((row) => row.id === option.id);
            return match === undefined || match.categoryId === parent;
        });
    }, [form.categoryId, subCategories]);

    const submit = async (e: React.FormEvent) => {
        e.preventDefault();
        const found = validate(form);
        setErrors(found);
        if (Object.keys(found).length > 0) return;

        const payload: ItemInput = {
            name: form.name.trim(),
            code: form.code.trim(),
            alternativeName: optionalText(form.alternativeName),
            genericName: optionalText(form.genericName),
            description: optionalText(form.description),
            categoryId: optionalId(form.categoryId),
            subCategoryId: optionalId(form.subCategoryId),
            brandId: optionalId(form.brandId),
            purchaseUnitId: optionalId(form.purchaseUnitId),
            saleUnitId: optionalId(form.saleUnitId),
            conversionRate: toDecimal(form.conversionRate),
            purchasePrice: toDecimal(form.purchasePrice),
            salePrice: toDecimal(form.salePrice),
            wholeSalePrice: optionalDecimal(form.wholeSalePrice),
            alertQuantity: optionalDecimal(form.alertQuantity),
            loyaltyPoint: toDecimal(form.loyaltyPoint),
            photo: optionalText(form.photo),
            symbology: optionalText(form.symbology),
            weighed: form.weighed === "yes",
        };

        setSaving(true);
        setSubmitError(null);
        try {
            if (item) await updateItem(item.id, payload);
            else await createItem(payload);
            onSaved();
        } catch (error) {
            setSubmitError(error instanceof Error ? error.message : String(error));
        } finally {
            setSaving(false);
        }
    };

    const optional = [{ id: "", label: "None" }, ...categories];
    const optionalBrands = [{ id: "", label: "None" }, ...brands];
    const optionalUnits = [{ id: "", label: "None" }, ...units];
    const symbologyOptions = [{ id: "", label: "Internal code" }, ...SYMBOLOGIES.map((s) => ({ id: s, label: s }))];
    const yesNoOptions = [
        { id: "no", label: "No" },
        { id: "yes", label: "Yes" },
    ];

    return (
        <ModalOverlay isOpen onOpenChange={(open) => !open && onClose()}>
            <Modal className="max-w-2xl">
                <Dialog className="p-6 md:p-8">
                    <form onSubmit={submit} className="flex flex-col gap-6">
                        <div className="flex flex-col gap-0.5">
                            <h2 className="text-display-xs font-semibold text-primary">{item ? "Edit item" : "New item"}</h2>
                            <p className="text-sm text-tertiary">Name and code are required; prices cannot be negative.</p>
                        </div>

                        {submitError && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{submitError}</p>}

                        <div className="grid gap-4 md:grid-cols-2">
                            <Input label="Name" value={form.name} onChange={set("name")} isInvalid={!!errors.name} hint={errors.name} isRequired />
                            <Input label="Code" value={form.code} onChange={set("code")} isInvalid={!!errors.code} hint={errors.code} isRequired />
                            <Input label="Alternative name" value={form.alternativeName} onChange={set("alternativeName")} />
                            <Input label="Generic name" value={form.genericName} onChange={set("genericName")} />
                            <Select label="Category" items={optional} selectedKey={form.categoryId} onSelectionChange={(k) => set("categoryId")(String(k ?? ""))}>
                                {(row) => <Select.Item id={row.id} textValue={row.label}>{row.label}</Select.Item>}
                            </Select>
                            <Select
                                label="Sub-category"
                                items={subCategoryOptions}
                                selectedKey={form.subCategoryId}
                                onSelectionChange={(k) => set("subCategoryId")(String(k ?? ""))}
                                hint="Belongs to the chosen category"
                            >
                                {(row) => <Select.Item id={row.id} textValue={row.label}>{row.label}</Select.Item>}
                            </Select>
                            <Select label="Brand" items={optionalBrands} selectedKey={form.brandId} onSelectionChange={(k) => set("brandId")(String(k ?? ""))}>
                                {(row) => <Select.Item id={row.id} textValue={row.label}>{row.label}</Select.Item>}
                            </Select>
                            <Select label="Purchase unit" items={optionalUnits} selectedKey={form.purchaseUnitId} onSelectionChange={(k) => set("purchaseUnitId")(String(k ?? ""))}>
                                {(row) => <Select.Item id={row.id} textValue={row.label}>{row.label}</Select.Item>}
                            </Select>
                            <Select label="Sale unit" items={optionalUnits} selectedKey={form.saleUnitId} onSelectionChange={(k) => set("saleUnitId")(String(k ?? ""))}>
                                {(row) => <Select.Item id={row.id} textValue={row.label}>{row.label}</Select.Item>}
                            </Select>
                            <Input
                                label="Purchase price"
                                value={form.purchasePrice}
                                onChange={set("purchasePrice")}
                                isInvalid={!!errors.purchasePrice}
                                hint={errors.purchasePrice}
                            />
                            <Input label="Sale price" value={form.salePrice} onChange={set("salePrice")} isInvalid={!!errors.salePrice} hint={errors.salePrice} />
                            <Input
                                label="Conversion rate"
                                hint={errors.conversionRate ?? "How many purchase units make one sale unit"}
                                value={form.conversionRate}
                                onChange={set("conversionRate")}
                                isInvalid={!!errors.conversionRate}
                                isRequired
                            />
                            <Input
                                label="Wholesale price"
                                value={form.wholeSalePrice}
                                onChange={set("wholeSalePrice")}
                            />
                            <Input label="Alert quantity" value={form.alertQuantity} onChange={set("alertQuantity")} />
                            <Input label="Loyalty point" value={form.loyaltyPoint} onChange={set("loyaltyPoint")} />
                            <Select
                                label="Barcode symbology"
                                items={symbologyOptions}
                                selectedKey={form.symbology}
                                onSelectionChange={(k) => set("symbology")(String(k ?? ""))}
                                hint="Blank means an internal code"
                            >
                                {(row) => <Select.Item id={row.id} textValue={row.label}>{row.label}</Select.Item>}
                            </Select>
                            <Select
                                label="Sold by weight"
                                items={yesNoOptions}
                                selectedKey={form.weighed}
                                onSelectionChange={(k) => set("weighed")(String(k ?? ""))}
                                hint="Price per kg — quantity comes from a scale"
                            >
                                {(row) => <Select.Item id={row.id} textValue={row.label}>{row.label}</Select.Item>}
                            </Select>
                            <Input
                                label="Photo"
                                value={form.photo}
                                onChange={set("photo")}
                                hint="Data URI image, under 1 MB"
                            />
                        </div>

                        <Input label="Description" value={form.description} onChange={set("description")} />

                        <div className="flex justify-end gap-2">
                            <Button color="secondary" onPress={onClose}>
                                Cancel
                            </Button>
                            <Button type="submit" isLoading={saving}>
                                {item ? "Save changes" : "Create item"}
                            </Button>
                        </div>
                    </form>
                </Dialog>
            </Modal>
        </ModalOverlay>
    );
};