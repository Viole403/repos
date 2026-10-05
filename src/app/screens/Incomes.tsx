import { LedgerScreen } from "./LedgerScreen";
import type { LedgerConfig } from "./LedgerScreen";
import { createIncome, listIncomeCategories, listIncomes } from "@/app/ipc";

// Module level so the screen's effect dependencies are stable references.
const config: LedgerConfig = {
    title: "Incomes",
    blurb: "Money the shop takes that is not a sale — a refund, a loan, a grant.",
    accountVerb: "arrived in",
    submitLabel: "Record income",
    load: (query) => listIncomes(query),
    create: (input) => createIncome(input),
    loadCategories: (query) => listIncomeCategories(query),
};

export const Incomes = () => <LedgerScreen config={config} />;