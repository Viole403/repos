import { MasterDataScreen } from "./MasterDataScreen";
import type { MasterConfig } from "./MasterDataScreen";
import { createIncomeCategory, deleteIncomeCategory, listIncomeCategories } from "@/app/ipc";

// Module level so `load` is a stable effect dependency.
const config: MasterConfig = {
    entity: "income category",
    nameLabel: "Income category name",
    load: async (query) => {
        const page = await listIncomeCategories(query);
        return { ...page, rows: page.rows.map((row) => ({ id: row.id, name: row.name, description: row.description, createdAt: row.createdAt })) };
    },
    create: (input) => createIncomeCategory({ name: input.name, description: input.description }),
    remove: deleteIncomeCategory,
};

export const IncomeCategories = () => <MasterDataScreen {...config} />;
