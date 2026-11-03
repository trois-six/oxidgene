// The web build in `none` mode, the mode this suite's server runs in
// (scripts/backend.mjs): an interface equivalent to the desktop's, with no
// sign-in, no account menu, no banner and no profile, and no request to an
// account route (docs/authentication.md §2, §10). The sign-in itself is
// `just e2e-auth`'s (e2e/auth/).

import { apiUrl, expect, test } from "./fixtures";

/// The account routes the interface must never call in `none` mode.
const accountRoute = /\/(auth\/(login|logout|logout-everywhere)|api\/v1\/(me|users|admin)\b)/;

test("without authentication the interface shows no account", async ({ page, tree }) => {
    const sessions: string[] = [];
    const accountCalls: string[] = [];
    page.on("request", (request) => {
        const url = request.url();
        if (url === `${apiUrl}/auth/session`) sessions.push(url);
        if (accountRoute.test(new URL(url).pathname)) accountCalls.push(url);
    });

    await page.goto("/");
    await expect(page.locator(".tree-card-name", { hasText: tree.name })).toBeVisible();
    // The server was asked once whether it signs anyone in, and said no.
    expect(sessions).toHaveLength(1);

    const navbar = page.locator(".app-nav");
    await expect(navbar.getByRole("link", { name: "OxidGene" })).toBeVisible();
    await expect(navbar.locator(".nav-end")).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Sign in" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Account menu" })).toHaveCount(0);
    await expect(page.locator(".user-avatar")).toHaveCount(0);

    // The application settings have no Instance section.
    await page.goto("/settings");
    await expect(page.getByRole("button", { name: "Appearance", exact: true })).toBeVisible();
    await expect(page.getByText("Instance", { exact: true })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Users", exact: true })).toHaveCount(0);

    // There is no account to open: its section gives way to the first one.
    await page.goto("/settings/profile");
    await expect(page.getByRole("heading", { name: "Appearance" })).toBeVisible();
    await expect(page.getByText("My account", { exact: true })).toHaveCount(0);

    // A tree page opens as it always did.
    await page.goto(`/trees/${tree.treeId}`);
    await expect(page.locator("g.ped-card").first()).toBeVisible();
    await expect(page.getByText("Sign in to continue")).toHaveCount(0);

    expect(accountCalls, "account routes called in none mode").toEqual([]);
});

test("a sign-in error in the address is ignored without authentication", async ({ page, tree }) => {
    await page.goto("/?auth_error=signup_closed");
    await expect(page.locator(".tree-card-name", { hasText: tree.name })).toBeVisible();
    await expect(page.getByRole("dialog")).toHaveCount(0);
});
