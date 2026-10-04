import { MasterDataScreen } from "./MasterDataScreen";
import type { MasterConfig } from "./MasterDataScreen";
import { createItemSubCategory, deleteItemSubCategory, listItemCategories, listItemSubCategories } from "@/app/ipc";

// Module level so `load` and `parent.load` are stable effect dependencies — an
// inline arrow here would re-fetch on every render.
const config: MasterConfig = {
    entity: "sub-category",
    nameLabel: "Name",
    load: async (query) => {
        const page = await listItemSubCategories(null, query);
        return {
            ...page,
            rows: page.rows.map((row) => ({
                id: row.id,
                name: row.name,
                description: row.description,
                createdAt: row.createdAt,
            })),
        };
    },
    create: (input) =>
        createItemSubCategory({
            categoryId: input.parentId!,
            name: input.name,
            description: input.description,
            sortId: input.sortId,
        }),
    remove: deleteItemSubCategory,
    parent: {
        label: "Category",
        load: async () => (await listItemCategories({ perPage: 500 })).rows.map((row) => ({ id: row.id, label: row.name })),
    },
};

export const ItemSubCategories = () => <MasterDataScreen {...config} />;