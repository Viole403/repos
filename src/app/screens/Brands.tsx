import { MasterDataScreen } from "./MasterDataScreen";
import type { MasterConfig } from "./MasterDataScreen";
import { createBrand, deleteBrand, listBrands } from "@/app/ipc";

const config: MasterConfig = {
    entity: "brand",
    nameLabel: "Name",
    load: async (query) => {
        const page = await listBrands(query);
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
    create: (input) => createBrand({ name: input.name, description: input.description }),
    remove: deleteBrand,
};

export const Brands = () => <MasterDataScreen {...config} />;