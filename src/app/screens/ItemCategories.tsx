import { MasterDataScreen } from "./MasterDataScreen";
import type { MasterConfig } from "./MasterDataScreen";
import { createItemCategory, deleteItemCategory, listItemCategories } from "@/app/ipc";

const config: MasterConfig = {
    entity: "category",
    nameLabel: "Name",
    withSort: true,
    load: async (query) => {
        const page = await listItemCategories(query);
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
    create: (input) => createItemCategory({ name: input.name, description: input.description, sortId: input.sortId }),
    remove: deleteItemCategory,
};

export const ItemCategories = () => <MasterDataScreen {...config} />;