import { defineConfig, devices } from "@playwright/test";

import suite from "./playwright.config";

// `just screenshots`: the suite's servers and browser, pointed at the
// screenshot script instead of the tests, one page at a time, at the fixed
// viewport the README images are taken at.
export default defineConfig({
    ...suite,
    testDir: "./screenshots",
    timeout: 120_000,
    fullyParallel: false,
    workers: 1,
    retries: 0,
    reporter: [["list"]],
    projects: [
        {
            name: "screenshots",
            use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 },
        },
    ],
});
