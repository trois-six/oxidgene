import { defineConfig, devices } from "@playwright/test";

// `just archives-live`: the live checks of the archive portals
// (docs/archives.md §9.1), never part of `just e2e`. It needs no server of
// ours: each test opens a real portal, one archive at a time, with
// sequential requests and no retry.
const ci = !!process.env.CI;
const chrome = devices["Desktop Chrome"];

export default defineConfig({
    testDir: "./archives",
    outputDir: "./test-results/archives",
    // One archive per test: its collections, each with a few requests, a
    // page load per portal and a viewer to open.
    timeout: 300_000,
    fullyParallel: false,
    workers: 1,
    retries: 0,
    forbidOnly: ci,
    reporter: ci ? [["list"], ["github"]] : [["list"]],
    use: {
        ...chrome,
        // The browser's own, plus who is asking (docs/archives.md §8).
        userAgent: `${chrome.userAgent} OxidGene-live-check (+https://github.com/trois-six/oxidgene)`,
        locale: "fr-FR",
        timezoneId: "Europe/Paris",
        // Portal pages only; the workflow keeps them a few days.
        trace: "on",
        screenshot: "off",
        video: "off",
    },
    projects: [{ name: "archives" }],
});
