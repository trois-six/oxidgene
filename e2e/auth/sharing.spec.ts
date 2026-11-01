// The sharing journey of docs/authorization.md §6 and §11, between two of
// the development stack's fictitious users: the owner invites the guest's
// address, the guest accepts the invitation from the homepage, then a share
// link lets an anonymous browser visit the tree until the owner revokes it.
//
// Like the sign-in journeys, every test skips itself while the server
// answers `/auth/session` without OpenID Connect. Each run names its tree
// afresh, so it can run again on the same stack.

import { expect, test, type APIRequestContext, type Browser, type Page } from "@playwright/test";

const baseURL = process.env.E2E_AUTH_BASE_URL ?? "http://localhost:8081";
const issuer = process.env.E2E_AUTH_ISSUER ?? "http://localhost:8180/auth/v1/";

type Role = "owner" | "guest";

test.beforeAll(async ({ playwright }) => {
    const request = await playwright.request.newContext({ baseURL });
    const response = await request.get("/auth/session").catch(() => undefined);
    const body = response?.ok() ? ((await response.json()) as { mode?: string }) : undefined;
    await request.dispose();
    test.skip(body?.mode !== "oidc", "The server has no OpenID Connect sign-in.");
});

/// Signs `role` in through the provider's page, back at the application.
async function signIn(page: Page, role: Role): Promise<void> {
    await page.goto("/auth/login?return_to=/");
    await expect(page).toHaveURL((url) => url.href.startsWith(issuer));
    const email = page.locator('input[name="email"]');
    await email.fill(`${role}@example.invalid`);
    await email.press("Enter");
    const password = page.locator('input[type="password"]:visible');
    await password.fill(role);
    await password.press("Enter");
    await page.waitForURL((url) => url.origin === new URL(baseURL).origin && !url.pathname.startsWith("/auth/"));
}

async function signedIn(browser: Browser, role: Role): Promise<Page> {
    const context = await browser.newContext({ baseURL });
    const page = await context.newPage();
    await signIn(page, role);
    return page;
}

/// The headers of a write from `request`'s session: its CSRF token and the
/// application's origin.
async function writeHeaders(request: APIRequestContext): Promise<Record<string, string>> {
    const session = (await (await request.get("/auth/session")).json()) as { csrf_token?: string };
    expect(session.csrf_token).toBeTruthy();
    return { "x-oxidgene-csrf": session.csrf_token!, origin: new URL(baseURL).origin };
}

test("an owner shares a tree by invitation and by link", async ({ browser }) => {
    const owner = await signedIn(browser, "owner");
    const headers = await writeHeaders(owner.request);
    const name = `Shared fixture ${Date.now()}`;
    const created = await owner.request.post("/api/v1/trees", { headers, data: { name } });
    expect(created.status(), await created.text()).toBe(201);
    const tree = ((await created.json()) as { id: string }).id;

    // The guest signs in once, so the address has an account; the share
    // makes them a member, or — on an instance whose signup the invitation
    // opens — invites them.
    const guest = await signedIn(browser, "guest");
    const shared = await owner.request.post(`/api/v1/trees/${tree}/members`, {
        headers,
        data: { email: "guest@example.invalid", role: "guest" },
    });
    expect([201, 202], await shared.text()).toContain(shared.status());
    if (shared.status() === 202) {
        await guest.goto("/invitations");
        const invitation = guest.locator(".home-invitation", { hasText: name });
        await expect(invitation).toBeVisible();
        await invitation.getByRole("button", { name: "Accept" }).click();
    }
    await guest.goto("/");
    const card = guest.locator(".tree-card", { hasText: name });
    await expect(card).toBeVisible();
    await expect(card.locator(".tree-card-role")).toHaveText(/guest/i);

    // A share link, created in the Sharing section and shown once.
    await owner.goto(`/trees/${tree}/settings`);
    await owner.getByRole("button", { name: "Sharing", exact: true }).click();
    await owner.getByRole("button", { name: "Create the link" }).click();
    const address = owner.locator(".sharing-invited input, .sharing-invited textarea").first();
    await expect(address).toHaveValue(/\/auth\/share\/ogs_/);
    const url = await address.inputValue();

    // An anonymous browser opening it visits the tree as a Visitor.
    const context = await browser.newContext({ baseURL });
    const visitor = await context.newPage();
    await visitor.goto(new URL(url).pathname);
    await expect(visitor).toHaveURL(new RegExp(`/trees/${tree}$`));
    await expect(visitor.locator(".visitor-banner")).toContainText("You are viewing this tree as a visitor");
    const access = await visitor.request.get(`/api/v1/trees/${tree}`);
    expect(((await access.json()) as { viewer_access: { role: string } }).viewer_access.role).toBe("visitor");

    // Revoked, the link opens nothing more, at once.
    const links = (await (await owner.request.get(`/api/v1/trees/${tree}/share-links`)).json()) as {
        edges: { node: { id: string } }[];
    };
    const revoked = await owner.request.delete(`/api/v1/trees/${tree}/share-links/${links.edges[0].node.id}`, {
        headers,
    });
    expect(revoked.status()).toBe(204);
    expect((await visitor.request.get(`/api/v1/trees/${tree}`)).status()).toBe(401);

    // The owner deletes the fixture.
    expect((await owner.request.delete(`/api/v1/trees/${tree}`, { headers })).status()).toBe(204);
});
