import { defineConfig, devices } from "@playwright/test";

// Ports away from the developer's 8080/8081 (`just dev-web`), so the suite can
// run next to a development server. The web bundle bakes the API URL in at
// build time: `just e2e` builds it for the same API port.
const apiPort = process.env.E2E_API_PORT ?? "18080";
const webPort = process.env.E2E_WEB_PORT ?? "18081";
const ci = !!process.env.CI;

export default defineConfig({
    testDir: "./tests",
    timeout: 60_000,
    expect: { timeout: 15_000 },
    fullyParallel: true,
    forbidOnly: ci,
    retries: ci ? 1 : 0,
    // SQLite serializes the writes; more workers only queue behind it.
    workers: ci ? 2 : 4,
    reporter: ci ? [["list"], ["html", { open: "never" }], ["github"]] : [["list"], ["html", { open: "never" }]],
    use: {
        baseURL: `http://127.0.0.1:${webPort}`,
        locale: "en-US",
        timezoneId: "UTC",
        trace: "retain-on-failure",
        screenshot: "only-on-failure",
    },
    projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } } }],
    webServer: [
        {
            command: "node scripts/backend.mjs",
            url: `http://127.0.0.1:${apiPort}/healthz`,
            env: { E2E_API_PORT: apiPort, E2E_WEB_PORT: webPort },
            // The first run compiles the server.
            timeout: 600_000,
            // Never attach to a server already on the port: it could hold
            // someone's data.
            reuseExistingServer: false,
            gracefulShutdown: { signal: "SIGTERM", timeout: 10_000 },
            stdout: "ignore",
            stderr: "pipe",
        },
        {
            command: "node scripts/static-server.mjs",
            url: `http://127.0.0.1:${webPort}/`,
            env: { E2E_WEB_PORT: webPort },
            timeout: 30_000,
            reuseExistingServer: false,
        },
    ],
});
