import { LedgerScreen } from "./LedgerScreen";
import type { LedgerConfig } from "./LedgerScreen";
import { createExpense, listExpenseCategories, listExpenses } from "@/app/ipc";

const config: LedgerConfig = {
    title: "Expenses",
    blurb: "Money the shop spends that is not a purchase — rent, utilities, a subscription.",
    accountVerb: "left",
    submitLabel: "Record expense",
    load: (query) => listExpenses(query),
    create: (input) => createExpense(input),
    loadCategories: (query) => listExpenseCategories(query),
};

export const Expenses = () => <LedgerScreen config={config} />;
