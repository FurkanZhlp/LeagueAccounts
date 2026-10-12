import { failStartup } from "./startup";
import { hextechLoader } from "./loader";

const loader = document.querySelector<HTMLElement>("#startup-animation");
if (loader) hextechLoader(loader);

// Keep the HTML loading screen usable even when an app module fails to load.
document.querySelector("#startup-retry")?.addEventListener("click", () => location.reload());
// Dev-only: `?scene=loading` holds the loading screen so it can be inspected.
const holdLoader = import.meta.env.DEV && new URLSearchParams(location.search).get("scene") === "loading";
if (!holdLoader) void import("./main").catch(failStartup);
