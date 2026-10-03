import { MasterDataScreen } from "./MasterDataScreen";
import type { MasterConfig } from "./MasterDataScreen";
import { createUnit, deleteUnit, listUnits } from "@/app/ipc";

// Module level so `load` is a stable effect dependency.
const config: MasterConfig = {
    entity: "unit",
    nameLabel: "Unit name",
    load: async (query) => {
        const page = await listUnits(query);
        return {
            ...page,
            rows: page.rows.map((row) => ({
                id: row.id,
                name: row.unitName,
                description: row.description,
                createdAt: row.createdAt,
            })),
        };
    },
    create: (input) => createUnit({ unitName: input.name, description: input.description }),
    remove: deleteUnit,
};

export const Units = () => <MasterDataScreen {...config} />;