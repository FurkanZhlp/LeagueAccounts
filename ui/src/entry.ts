import { failStartup } from "./startup";

// Keep the HTML loading screen usable even when an app module fails to load.
document.querySelector("#startup-retry")?.addEventListener("click", () => location.reload());
void import("./main").catch(failStartup);
