import { MasterDataScreen } from "./MasterDataScreen";
import type { MasterConfig } from "./MasterDataScreen";
import { createExpenseCategory, deleteExpenseCategory, listExpenseCategories } from "@/app/ipc";

const config: MasterConfig = {
    entity: "expense category",
    nameLabel: "Expense category name",
    load: async (query) => {
        const page = await listExpenseCategories(query);
        return { ...page, rows: page.rows.map((row) => ({ id: row.id, name: row.name, description: row.description, createdAt: row.createdAt })) };
    },
    create: (input) => createExpenseCategory({ name: input.name, description: input.description }),
    remove: deleteExpenseCategory,
};

export const ExpenseCategories = () => <MasterDataScreen {...config} />;
