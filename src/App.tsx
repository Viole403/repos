import { HashRouter, useRoutes } from "react-router-dom";
import { AuthProvider } from "./app/auth";
import { routes } from "./app/routes";

/** useRoutes must run inside the router, hence the wrapper */
const RoutedApp = () => useRoutes(routes);

/** HashRouter: Tauri's custom protocol has no server to answer a history route on reload — see AGENTS.md */
const App = () => (
    <HashRouter>
        {/* Above the router: the session is restored once per launch, not per route. */}
        <AuthProvider>
            <RoutedApp />
        </AuthProvider>
    </HashRouter>
);

export default App;