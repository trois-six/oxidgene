// OxidGene's mail through the development stack's Mailpit
// (docs/messaging.md §3, §14): the owner invites an address, the invitation
// arrives in Mailpit in the instance's language with the invitation's link,
// the invited user signs in and accepts it, and the owner's bell — and
// their mailbox — say so.
//
// Like the other journeys, every test skips itself while the server answers
// `/auth/session` without OpenID Connect, and also while Mailpit's API
// (E2E_MAILPIT_URL, http://localhost:8025 by default) does not answer. Each
// run names its tree afresh, so it can run again on the same stack.

import { expect, test, type APIRequestContext, type Browser, type Page } from "@playwright/test";

const baseURL = process.env.E2E_AUTH_BASE_URL ?? "http://localhost:8081";
const issuer = process.env.E2E_AUTH_ISSUER ?? "http://localhost:8180/auth/v1/";
const mailpit = process.env.E2E_MAILPIT_URL ?? "http://localhost:8025";

type Role = "owner" | "visitor";

test.beforeAll(async ({ playwright }) => {
    const request = await playwright.request.newContext();
    const session = await request.get(`${baseURL}/auth/session`).catch(() => undefined);
    const body = session?.ok() ? ((await session.json()) as { mode?: string }) : undefined;
    const mail = await request.get(`${mailpit}/api/v1/info`).catch(() => undefined);
    const mailOk = !!mail?.ok();
    await request.dispose();
    test.skip(body?.mode !== "oidc", "The server has no OpenID Connect sign-in.");
    test.skip(!mailOk, "Mailpit's API does not answer.");
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

async function writeHeaders(request: APIRequestContext): Promise<Record<string, string>> {
    const session = (await (await request.get("/auth/session")).json()) as { csrf_token?: string };
    expect(session.csrf_token).toBeTruthy();
    return { "x-oxidgene-csrf": session.csrf_token!, origin: new URL(baseURL).origin };
}

type Summary = { ID: string; Subject: string; To: { Address: string }[] };

/// The mail to `address` whose subject holds `fragment`, once Mailpit has
/// it: its plain text and HTML.
async function mailTo(
    request: APIRequestContext,
    address: string,
    fragment: string,
): Promise<{ subject: string; text: string; html: string }> {
    let found: Summary | undefined;
    await expect
        .poll(
            async () => {
                const list = (await (await request.get(`${mailpit}/api/v1/messages?limit=100`)).json()) as {
                    messages: Summary[];
                };
                found = list.messages.find(
                    (m) => m.Subject.includes(fragment) && m.To.some((to) => to.Address === address),
                );
                return !!found;
            },
            { timeout: 30_000, message: `a mail to ${address} about ${fragment}` },
        )
        .toBe(true);
    const message = (await (await request.get(`${mailpit}/api/v1/message/${found!.ID}`)).json()) as {
        Text: string;
        HTML: string;
    };
    return { subject: found!.Subject, text: message.Text, html: message.HTML };
}

test("an invitation arrives in Mailpit and is accepted", async ({ browser, playwright }) => {
    const mail = await playwright.request.newContext();
    const owner = await signedIn(browser, "owner");
    const headers = await writeHeaders(owner.request);
    const name = `Mailed fixture ${Date.now()}`;
    const created = await owner.request.post("/api/v1/trees", { headers, data: { name } });
    expect(created.status(), await created.text()).toBe(201);
    const tree = ((await created.json()) as { id: string }).id;

    // An address OxidGene has no account for yet becomes an invitation,
    // mailed rather than handed to the owner.
    const shared = await owner.request.post(`/api/v1/trees/${tree}/members`, {
        headers,
        data: { email: "visitor@example.invalid", role: "editor" },
    });
    expect([201, 202], await shared.text()).toContain(shared.status());
    const invited = shared.status() === 202;
    if (invited) {
        const invitation = ((await shared.json()) as { invitation: { delivery: string } }).invitation;
        expect(invitation.delivery).toBe("email");
        const message = await mailTo(mail, "visitor@example.invalid", name);
        expect(message.subject).toContain("invites you to the family tree");
        expect(message.text).toContain("as an editor");
        expect(message.text).toContain(`${baseURL}/invitations`);
        expect(message.html).toContain(name);
    } else {
        // The address signed in on this stack before: it is shared at once,
        // and the notification is mailed.
        const message = await mailTo(mail, "visitor@example.invalid", name);
        expect(message.text).toContain(`${baseURL}/trees/${tree}`);
    }

    // The invited user follows the mail's link, signs in, and accepts.
    const visitor = await signedIn(browser, "visitor");
    if (invited) {
        await visitor.goto("/invitations");
        const invitation = visitor.locator(".home-invitation", { hasText: name });
        await expect(invitation).toBeVisible();
        await invitation.getByRole("button", { name: "Accept" }).click();
    }
    await visitor.goto("/");
    await expect(visitor.locator(".tree-card", { hasText: name })).toBeVisible();

    if (invited) {
        // The owner's bell counts the acceptance, and their mailbox has it.
        await owner.goto("/");
        const bell = owner.locator(".notification-bell");
        await expect(bell.locator(".notification-badge")).toBeVisible();
        await bell.click();
        const item = owner.locator(".notification-item", { hasText: "accepted your invitation" }).first();
        await expect(item).toContainText(name);
        await owner.getByRole("menuitem", { name: "Mark all as read" }).click();
        await expect(bell.locator(".notification-badge")).toHaveCount(0);
        const accepted = await mailTo(mail, "owner@example.invalid", "accepted your invitation");
        expect(accepted.text).toContain(name);
        expect(accepted.text).toContain("/unsubscribe?token=");
    }

    // The owner deletes the fixture.
    expect((await owner.request.delete(`/api/v1/trees/${tree}`, { headers })).status()).toBe(204);
    await mail.dispose();
});
