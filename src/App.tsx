import { HashRouter, useRoutes } from "react-router-dom";
import { routes } from "./app/routes";

/** useRoutes must run inside the router, hence the wrapper */
const RoutedApp = () => useRoutes(routes);

/** HashRouter: Tauri's custom protocol has no server to answer a history route on reload — see AGENTS.md */
const App = () => (
    <HashRouter>
        <RoutedApp />
    </HashRouter>
);

export default App;