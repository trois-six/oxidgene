// The sign-in journeys of docs/authentication.md §12, through the
// Backend-for-Frontend of §4 and the development stack's Rauthy (§8.5):
// signing in, the session the browser holds and cannot read, the instance
// role a groups claim gives, CSRF, sign-out at both ends, sessions ended by
// the provider or everywhere, and the tokens the API refuses; then the web
// interface over them (§10): the navbar's sign-in and account menu, the
// failed sign-in and ended-session dialogs, the profile page and the
// administrator's list of users.
//
// Every user is fictitious, named after a role, with that role as password
// (docker/rauthy/bootstrap/users.json). Every test skips itself while
// the server answers `/auth/session` without OpenID Connect: a backend built
// before authentication, or one started with OXIDGENE_AUTH_MODE=none.

import { expect, test, type APIRequestContext, type Browser, type Page } from "@playwright/test";

import { png } from "../tests/requests";

const baseURL = process.env.E2E_AUTH_BASE_URL ?? "http://localhost:8081";
// Rauthy's issuer ends with a slash.
const issuer = process.env.E2E_AUTH_ISSUER ?? "http://localhost:8180/auth/v1/";
const provider = new URL(issuer).origin;
// The development API key of docker/rauthy/bootstrap/api_keys.json, which
// may read users and end their sessions.
const providerApiKey =
    process.env.E2E_AUTH_RAUTHY_API_KEY ??
    "oxidgene-dev$oxidgeneDevelopmentApiKeyForTheComposeStackOnlyNeverUseItElsewhere";

type Role = "admin" | "owner" | "editor" | "guest" | "visitor" | "outsider";

interface Session {
    mode: string;
    authenticated: boolean;
    client_id?: string;
    csrf_token?: string | null;
    expires_at?: string;
    idle_expires_at?: string;
    account_url?: string;
    user?: null | {
        id: string;
        display_name: string;
        email: string;
        email_verified: boolean;
        instance_role: string;
        can_create_trees: boolean;
    };
}

test.beforeAll(async ({ playwright }) => {
    const request = await playwright.request.newContext({ baseURL });
    let response;
    try {
        response = await request.get("/auth/session");
    } catch (error) {
        throw new Error(`Nothing answers at ${baseURL}: start the Compose stack (\`just e2e-auth\` does). ${error}`);
    }
    // A proxy error (502 while the server restarts) is a broken stack, not
    // a build without sign-in: it fails rather than skips every journey.
    if (!response.ok()) {
        throw new Error(`${baseURL}/auth/session answers ${response.status()}: the stack is not serving the API.`);
    }
    const json = (response.headers()["content-type"] ?? "").includes("json");
    const body = json ? ((await response.json()) as Session) : undefined;
    await request.dispose();
    test.skip(
        body?.mode !== "oidc",
        `The server answers /auth/session with ${response.status()}${body ? `, mode ${body.mode}` : ""}: it has no ` +
            "OpenID Connect sign-in (a build before authentication, or OXIDGENE_AUTH_MODE=none).",
    );
});

/// The session the browser's cookies name, checked to be uncacheable.
async function session(request: APIRequestContext): Promise<Session> {
    const response = await request.get("/auth/session");
    expect(response.status()).toBe(200);
    expect(response.headers()["cache-control"]).toContain("no-store");
    return (await response.json()) as Session;
}

/// Whether the browser is back at the application, past its `/auth` routes.
function backHome(url: URL): boolean {
    return url.origin === new URL(baseURL).origin && !url.pathname.startsWith("/auth/");
}

/// Signs `role` in on Rauthy's page, where a sign-in started — the email,
/// then the password — and waits for the application to take the browser
/// back.
async function signInAtProvider(page: Page, role: Role): Promise<void> {
    await expect(page).toHaveURL((url) => url.href.startsWith(issuer));
    const email = page.locator('input[name="email"]');
    await email.fill(`${role}@example.invalid`);
    await email.press("Enter");
    const password = page.locator('input[type="password"]:visible');
    await password.fill(role);
    await password.press("Enter");
    await page.waitForURL(backHome);
}

/// Starts a sign-in at the application and signs `role` in.
async function signIn(page: Page, role: Role, returnTo = "/"): Promise<void> {
    await page.goto(`/auth/login?return_to=${encodeURIComponent(returnTo)}`);
    await signInAtProvider(page, role);
}

/// A new browser with `role` signed in.
async function signedIn(browser: Browser, role: Role): Promise<Page> {
    const context = await browser.newContext({ baseURL });
    const page = await context.newPage();
    await signIn(page, role);
    return page;
}

/// Ends every session `role` holds at Rauthy, through its admin API: what
/// an administrator or the provider's own expiry does.
async function endProviderSessions(request: APIRequestContext, role: Role): Promise<void> {
    const headers = { Authorization: `API-Key ${providerApiKey}` };
    const user = await request.get(`${issuer}users/email/${role}@example.invalid`, { headers });
    expect(user.ok(), await user.text()).toBeTruthy();
    const { id } = (await user.json()) as { id: string };
    const ended = await request.delete(`${issuer}sessions/${id}`, { headers });
    expect(ended.ok(), await ended.text()).toBeTruthy();
}

test("an anonymous visitor has no session", async ({ request }) => {
    const anonymous = await session(request);
    expect(anonymous.mode).toBe("oidc");
    expect(anonymous.authenticated).toBe(false);
    // Absent or null alike: the server answers `null` (§4.2).
    expect(anonymous.user ?? undefined).toBeUndefined();
    expect(anonymous.csrf_token ?? undefined).toBeUndefined();
});

test("signing in through the provider opens a session the page cannot read", async ({ page, context }) => {
    await signIn(page, "owner");
    expect(new URL(page.url()).pathname).toBe("/");

    const owner = await session(page.request);
    expect(owner.authenticated).toBe(true);
    expect(owner.client_id).toBe("oxidgene-web");
    expect(owner.csrf_token).toBeTruthy();
    expect(owner.user).toMatchObject({
        display_name: "Owner Example",
        email: "owner@example.invalid",
        email_verified: true,
        instance_role: "user",
        can_create_trees: true,
    });
    expect(owner.account_url).toBe(`${issuer}account`);

    // §4.4: the session cookie is first-party, host-only, and out of scripts' reach.
    const cookie = (await context.cookies()).find((c) => c.name === "__Host-oxidgene");
    expect(cookie, "the session cookie").toBeDefined();
    expect(cookie).toMatchObject({ httpOnly: true, secure: true, sameSite: "Strict", path: "/" });
    // §4.1: no token anywhere a script could read it.
    const readable = await page.evaluate(() => [
        document.cookie,
        JSON.stringify({ ...localStorage }),
        JSON.stringify({ ...sessionStorage }),
    ]);
    for (const store of readable) {
        expect(store).not.toContain("__Host-oxidgene");
        expect(store).not.toMatch(/eyJ[\w-]+\.eyJ[\w-]+\./);
    }
});

test("sign-in returns to the page it started from, and only to the application", async ({ page }) => {
    await signIn(page, "editor", "/settings");
    expect(new URL(page.url()).pathname).toBe("/settings");

    // Any other target brings the browser to the application's home rather
    // than to another site (§4.3).
    for (const elsewhere of ["https://elsewhere.example.invalid/", "//elsewhere.example.invalid/", "/\\elsewhere.example.invalid"]) {
        const context = await page.context().browser()!.newContext({ baseURL });
        const other = await context.newPage();
        await signIn(other, "editor", elsewhere);
        expect(new URL(other.url()).origin, elsewhere).toBe(new URL(baseURL).origin);
        expect(new URL(other.url()).pathname, elsewhere).toBe("/");
        await context.close();
    }
});

test("the instance administrator comes from the provider's groups", async ({ browser }) => {
    const admin = await signedIn(browser, "admin");
    expect((await session(admin.request)).user).toMatchObject({ email: "admin@example.invalid", instance_role: "admin" });
    await admin.context().close();

    for (const role of ["guest", "outsider"] as const) {
        const user = await signedIn(browser, role);
        expect((await session(user.request)).user).toMatchObject({ email: `${role}@example.invalid`, instance_role: "user" });
        await user.context().close();
    }
});

test("a replayed callback opens no session", async ({ browser, page }) => {
    let callback = "";
    page.on("request", (request) => {
        if (new URL(request.url()).pathname === "/auth/callback") callback = request.url();
    });
    await signIn(page, "visitor");
    expect(callback, "the provider's redirect to /auth/callback").not.toBe("");

    // The same code and state, from a browser that never started a sign-in,
    // and from the one that did, now that its transaction is consumed.
    const stranger = await browser.newContext({ baseURL });
    const strangerPage = await stranger.newPage();
    await strangerPage.goto(callback);
    expect(new URL(strangerPage.url()).searchParams.get("auth_error")).toBe("login_failed");
    expect((await session(strangerPage.request)).authenticated).toBe(false);
    await stranger.close();

    await page.goto(callback);
    expect(new URL(page.url()).searchParams.get("auth_error")).toBe("login_failed");
});

test("signing out needs the CSRF token and ends the provider's session too", async ({ page }) => {
    await signIn(page, "owner");
    const { csrf_token: csrf } = await session(page.request);

    expect((await page.request.post("/auth/logout")).status(), "without the CSRF header").toBe(403);
    expect((await page.request.post("/auth/logout", { headers: { "X-OxidGene-CSRF": "not-the-token" } })).status()).toBe(403);
    expect((await session(page.request)).authenticated).toBe(true);

    const response = await page.request.post("/auth/logout", { headers: { "X-OxidGene-CSRF": csrf! } });
    expect(response.status()).toBe(200);
    const { logout_url: logoutUrl } = (await response.json()) as { logout_url: string | null };
    expect(logoutUrl).toBeTruthy();
    const logout = new URL(logoutUrl!);
    expect(`${logout.origin}${logout.pathname}`).toBe(`${issuer}oidc/logout`);
    expect(logout.searchParams.get("client_id")).toBe("oxidgene-web");
    expect(logout.searchParams.get("id_token_hint")).toBeTruthy();
    expect(logout.searchParams.get("post_logout_redirect_uri")).toBe(`${new URL(baseURL).origin}/`);
    expect((await session(page.request)).authenticated).toBe(false);

    // RP-initiated logout: the provider forgets the user and sends the
    // browser home, and the next sign-in asks who is signing in again.
    await page.goto(logoutUrl!);
    await page.waitForURL((url) => url.origin !== provider);
    expect(new URL(page.url()).pathname).toBe("/");
    await page.goto("/auth/login?return_to=/");
    await expect(page.locator('input[name="email"]')).toBeVisible();
});

test("a session the provider ends is over in the application", async ({ page, request }) => {
    await signIn(page, "guest");
    expect((await session(page.request)).authenticated).toBe(true);

    // Ending the user's sessions at Rauthy sends the back-channel logout of
    // §4.7 to the server, which ends every session of that user.
    await endProviderSessions(request, "guest");

    await expect.poll(async () => (await session(page.request)).authenticated, { timeout: 15_000 }).toBe(false);
});

test("signing out everywhere ends every session of the user", async ({ browser }) => {
    const first = await signedIn(browser, "editor");
    const second = await signedIn(browser, "editor");
    const other = await signedIn(browser, "owner");

    const { csrf_token: csrf } = await session(first.request);
    const response = await first.request.post("/auth/logout-everywhere", { headers: { "X-OxidGene-CSRF": csrf! } });
    expect(response.status()).toBe(200);
    expect((await session(first.request)).authenticated).toBe(false);
    expect((await session(second.request)).authenticated).toBe(false);
    expect((await session(other.request)).authenticated, "another user's session").toBe(true);

    for (const page of [first, second, other]) await page.context().close();
});

test("the API refuses tokens it cannot trust", async ({ request }) => {
    // A client-credentials token of the development API client,
    // which OxidGene knows neither as an API client nor as trusted (§5.1).
    const issued = await request.post(`${issuer}oidc/token`, {
        form: {
            grant_type: "client_credentials",
            client_id: "oxidgene-dev-robot",
            client_secret: "oxidgeneDevelopmentRobotClientSecretForTheComposeStackOnly000000",
        },
    });
    expect(issued.ok(), await issued.text()).toBeTruthy();
    const token = (await issued.json()).access_token as string;

    for (const credential of [token, "not-a-token", `${token.slice(0, -4)}AAAA`]) {
        const response = await request.get("/api/v1/trees", { headers: { Authorization: `Bearer ${credential}` } });
        expect(response.status(), credential.slice(0, 16)).toBe(401);
        expect(response.headers()["www-authenticate"] ?? "").toContain("Bearer");
    }
});

// ── The web interface (docs/authentication.md §10) ───────────────────────

test("a visitor signs in from the navbar, and signs out from the account menu", async ({ page }) => {
    await page.goto("/settings");
    const navbar = page.locator(".app-nav");
    // The application settings are the device's: open to a visitor.
    await expect(page.getByRole("button", { name: "Appearance", exact: true })).toBeVisible();
    await page.goto("/");
    // The trees need an account: the page invites the visitor to sign in.
    await expect(page.getByRole("heading", { name: "Sign in to continue" })).toBeVisible();
    await navbar.getByRole("button", { name: "Sign in" }).click();
    await signInAtProvider(page, "owner");
    expect(new URL(page.url()).pathname).toBe("/");

    const trigger = navbar.getByRole("button", { name: "Account menu" });
    // The generated avatar: the initials of "Owner Example".
    await expect(trigger.getByRole("img", { name: "Owner Example" })).toHaveText("OE");
    await expect(page.getByRole("heading", { name: "Sign in to continue" })).toHaveCount(0);
    await trigger.click();
    const menu = page.getByRole("menu");
    await expect(menu.getByText("Owner Example")).toBeVisible();
    await expect(menu.getByText("owner@example.invalid")).toBeVisible();
    await expect(menu.getByRole("menuitem", { name: "Profile" })).toBeVisible();

    // Signing out ends the session here and at the provider, which sends the
    // browser home.
    await menu.getByRole("menuitem", { name: "Sign out" }).click();
    await page.waitForURL((url) => url.origin === new URL(baseURL).origin && url.pathname === "/");
    await expect(navbar.getByRole("button", { name: "Sign in" })).toBeVisible();
    expect((await session(page.request)).authenticated).toBe(false);
});

test("a failed sign-in says why on the page it returns to", async ({ page }) => {
    await page.goto("/?auth_error=signup_closed");
    const dialog = page.getByRole("dialog", { name: "Sign-in failed" });
    await expect(dialog).toContainText("only admits the users it invited");
    // The error leaves the address, so a reload does not show it again.
    await expect(page).toHaveURL((url) => url.search === "");
    await dialog.getByRole("button", { name: "Close" }).click();
    await expect(dialog).toBeHidden();
});

test("a session that ends under a page offers to sign in again to it", async ({ page, request }) => {
    await signIn(page, "guest", "/profile");
    await expect(page.getByRole("heading", { name: "Your profile" })).toBeVisible();

    await endProviderSessions(request, "guest");
    await expect.poll(async () => (await session(page.request)).authenticated, { timeout: 15_000 }).toBe(false);

    // The next request the page makes is refused.
    await page.locator(".settings-nav").getByRole("button", { name: "Sessions" }).click();
    const dialog = page.getByRole("dialog", { name: "Your session has ended" });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Sign in again" }).click();
    await signInAtProvider(page, "guest");
    expect(new URL(page.url()).pathname).toBe("/profile");
    await expect(page.getByRole("heading", { name: "Your profile" })).toBeVisible();
});

test("the profile page changes the name, the avatar and the language, and lists the sessions", async ({ page }) => {
    await signIn(page, "editor", "/profile");
    const content = page.locator(".settings-content");
    const navbar = page.locator(".app-nav");

    // The display name, then the provider's back.
    const name = page.getByLabel("Display name");
    await expect(name).toHaveValue("Editor Example");
    await name.fill("Edwina Example");
    await content.getByRole("button", { name: "Save", exact: true }).click();
    await expect(content.getByText("Saved.")).toBeVisible();
    await expect(navbar.getByRole("img", { name: "Edwina Example" })).toHaveText("EE");
    await content.getByRole("button", { name: "Use my provider's name (Editor Example)" }).click();
    await expect(name).toHaveValue("Editor Example");
    const navbarAvatar = navbar.getByRole("img", { name: "Editor Example" });
    await expect(navbarAvatar).toHaveText("EE");

    // An avatar cut from a picture, then removed.
    const chooser = page.waitForEvent("filechooser");
    await content.getByRole("button", { name: "Upload" }).click();
    await (await chooser).setFiles({ name: "portrait.png", mimeType: "image/png", buffer: png(320, 240, 7) });
    const cropper = page.getByRole("dialog", { name: "Choose your avatar" });
    const square = cropper.getByRole("slider", { name: "Avatar area" });
    await expect(square).toBeVisible();
    await square.press("ArrowRight");
    await cropper.getByRole("button", { name: "Use as avatar" }).click();
    await expect(cropper).toBeHidden();
    await expect(content.locator(".user-avatar img")).toBeVisible();
    await expect(navbarAvatar.locator("img")).toBeVisible();
    await content.getByRole("button", { name: "Remove", exact: true }).click();
    await expect(content.locator(".user-avatar img")).toHaveCount(0);
    await expect(navbarAvatar).toHaveText("EE");

    // The account's language is the interface's at once.
    const sections = page.locator(".settings-nav");
    await sections.getByRole("button", { name: "Language" }).click();
    await content.getByRole("button", { name: /Français/ }).click();
    await expect(page.getByRole("heading", { name: "Votre langue" })).toBeVisible();
    await content.getByRole("button", { name: /English/ }).click();
    await expect(page.getByRole("heading", { name: "Your language" })).toBeVisible();

    // This browser's session is listed, and marked.
    await sections.getByRole("button", { name: "Sessions" }).click();
    await expect(content.getByText("This browser")).toBeVisible();
    await expect(content.getByRole("button", { name: "Sign out", exact: true })).toBeVisible();
    await expect(content.getByRole("button", { name: "Sign out everywhere" })).toBeVisible();

    // The account section leads to the provider's account page.
    await sections.getByRole("button", { name: "Account" }).click();
    await expect(content.getByRole("link", { name: "Open" })).toHaveAttribute("href", `${issuer}account`);
    await expect(content.getByRole("button", { name: "Delete my account" })).toBeVisible();
});

test("an administrator manages the users from the application settings", async ({ browser }) => {
    // Every journey's user exists once signed in.
    const editor = await signedIn(browser, "editor");
    await editor.goto("/settings");
    await expect(editor.getByRole("button", { name: "Appearance", exact: true })).toBeVisible();
    // Only an administrator has the Instance group.
    await expect(editor.getByRole("button", { name: "Users", exact: true })).toHaveCount(0);
    await editor.context().close();

    const admin = await signedIn(browser, "admin");
    await admin.goto("/settings");
    await admin.getByRole("button", { name: "Users", exact: true }).click();
    await expect(admin.getByRole("heading", { name: "Users" })).toBeVisible();
    const rows = admin.locator(".instance-user");
    const own = rows.filter({ hasText: "admin@example.invalid" });
    await expect(own.getByText("Administrator")).toBeVisible();
    // The administrator's own account is their profile's to change.
    await expect(own.getByRole("button", { name: /^Actions for / })).toHaveCount(0);

    const editorRow = rows.filter({ hasText: "editor@example.invalid" });
    await editorRow.getByRole("button", { name: /^Actions for / }).click();
    const menu = admin.getByRole("menu");
    for (const action of ["Make administrator", "Deactivate", "Change identity…", "Erase"]) {
        await expect(menu.getByRole("menuitem", { name: action })).toBeVisible();
    }
    // The development stack's groups claim decides the role: the change is
    // refused, and the section says so.
    await menu.getByRole("menuitem", { name: "Make administrator" }).click();
    await expect(admin.getByRole("alert")).toContainText("The change was refused");
    await admin.context().close();
});
