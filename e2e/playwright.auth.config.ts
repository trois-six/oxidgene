import { defineConfig, devices } from "@playwright/test";

// `just e2e-auth`: the sign-in journeys of docs/authentication.md §12, never
// part of `just e2e`. They need the development Compose stack, which the
// recipe starts: the web container on http://localhost:8081 — `localhost`,
// where the browser keeps `Secure` cookies over plain HTTP — proxying the
// server, which signs in against the Rauthy on http://localhost:8180 with
// the fictitious users of docker/rauthy/bootstrap/users.json. Playwright
// starts no server. E2E_AUTH_BASE_URL and E2E_AUTH_ISSUER move them.
const ci = !!process.env.CI;

export default defineConfig({
    testDir: "./auth",
    outputDir: "./test-results/auth",
    timeout: 60_000,
    expect: { timeout: 15_000 },
    // One provider and one store shared by every test: the journeys end
    // sessions, so they run one after the other.
    fullyParallel: false,
    workers: 1,
    retries: 0,
    forbidOnly: ci,
    reporter: ci ? [["list"], ["github"]] : [["list"]],
    use: {
        baseURL: process.env.E2E_AUTH_BASE_URL ?? "http://localhost:8081",
        locale: "en-US",
        timezoneId: "UTC",
        trace: "retain-on-failure",
        screenshot: "only-on-failure",
    },
    projects: [{ name: "auth", use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } } }],
});
