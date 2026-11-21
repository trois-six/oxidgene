// Pictures held only as somebody else's address (docs/cross-cutting.md §7.2,
// docs/ui-common.md §4.5): a file that names one does not make the reader's
// browser call it. The placeholder names the host; one click loads its
// pictures for this window; "Always" trusts the host for the tree, across a
// reload; a private address is never requested.
//
// The suite aborts every request that leaves 127.0.0.1 (`pageGuards`), so
// what is asserted is the request the browser attempts, or does not.

import type { APIRequestContext, Page } from "@playwright/test";

import { apiUrl, expect, importGedcom, test } from "./fixtures";

const HOST = "pictures.example.org";
const PORTRAIT = `https://${HOST}/album/portrait.jpg`;
/// A private address, over `https` so that the application's own rule is
/// what holds it back, not the policy's refusal of plain `http`.
const PRIVATE = "https://192.168.1.10/camera/snapshot.jpg";

/// A file as a third party would send it: one person, with a picture on
/// another site and one on a private network.
const GEDCOM = [
    "0 HEAD",
    "1 GEDC",
    "2 VERS 5.5.1",
    "2 FORM LINEAGE-LINKED",
    "1 CHAR UTF-8",
    "0 @I1@ INDI",
    "1 NAME Remote /Fixture/",
    "1 SEX F",
    "1 OBJE @M1@",
    "1 OBJE @M2@",
    "0 @M1@ OBJE",
    `1 FILE ${PORTRAIT}`,
    "2 FORM jpg",
    "2 TITL Portrait on another site",
    "0 @M2@ OBJE",
    `1 FILE ${PRIVATE}`,
    "2 FORM jpg",
    "2 TITL Snapshot on a private network",
    "0 TRLR",
    "",
].join("\n");

/// A tree holding the file's person, and that person's id.
async function importedTree(request: APIRequestContext, name: string): Promise<{ treeId: string; personId: string }> {
    const created = await request.post(`${apiUrl}/api/v1/trees`, { data: { name } });
    expect(created.ok(), await created.text()).toBeTruthy();
    const treeId = (await created.json()).id as string;
    await importGedcom(request, treeId, GEDCOM);
    const profiles = (await (await request.get(`${apiUrl}/api/v1/trees/${treeId}/profiles?first=10`)).json()) as {
        edges: Array<{ node: { person_id: string } }>;
    };
    expect(profiles.edges).toHaveLength(1);
    return { treeId, personId: profiles.edges[0].node.person_id };
}

/// The requests the page attempts to the two hosts of the file, as they
/// start. Attach before the page is opened.
function remoteRequests(page: Page): string[] {
    const attempted: string[] = [];
    page.on("request", (request) => {
        const { hostname } = new URL(request.url());
        if (hostname === HOST || hostname === "192.168.1.10") attempted.push(request.url());
    });
    return attempted;
}

test("a picture a file names on another site waits for the reader, and a private address is never requested", async ({ page, request }, testInfo) => {
    const { treeId, personId } = await importedTree(request, `E2E remote media ${testInfo.workerIndex}-${Date.now()}`);
    const attempted = remoteRequests(page);
    // The import trusted nothing.
    const read = await (await request.get(`${apiUrl}/api/v1/trees/${treeId}`)).json();
    expect(read.trusted_media_hosts).toEqual([]);

    await page.goto(`/trees/${treeId}/persons/${personId}`);
    const asked = page.locator(".media-tile .remote-gate.is-tile:not(.is-refused)");
    const refused = page.locator(".media-tile .remote-gate.is-refused");
    await expect(asked).toHaveCount(1);
    await expect(asked.locator(".remote-gate-host")).toHaveText(HOST);
    await expect(refused).toHaveCount(1);
    await expect(refused).toContainText("192.168.1.10");
    await expect(refused.getByRole("button")).toHaveCount(0);
    // The page is drawn, portrait included, and nothing was requested.
    await expect(page.locator(".pd-header-top")).toBeVisible();
    expect(attempted).toEqual([]);

    // Opened in the viewer, the picture still waits, and says why.
    await asked.locator(".remote-gate-host").click();
    const viewer = page.locator(".media-viewer");
    const stage = viewer.locator(".remote-gate.is-stage");
    await expect(stage).toContainText(HOST);
    await expect(stage).toContainText("tells that site");
    expect(attempted).toEqual([]);
    await viewer.getByRole("button", { name: "Close" }).first().click();
    await expect(viewer).toHaveCount(0);

    // One click loads the pictures of that host, without opening the tile.
    await asked.getByRole("button", { name: `Load the pictures held by ${HOST}`, exact: true }).click();
    await expect.poll(() => attempted).toContain(PORTRAIT);
    await expect(asked).toHaveCount(0);
    await expect(viewer).toHaveCount(0);
    expect(attempted.filter((url) => url.startsWith(PRIVATE))).toEqual([]);

    // The choice was this window's: after a reload the picture waits again.
    attempted.length = 0;
    await page.reload();
    await expect(asked).toHaveCount(1);
    await expect(page.locator(".pd-header-top")).toBeVisible();
    expect(attempted).toEqual([]);

    // "Always" trusts the host for the tree, and says so.
    await asked.getByRole("button", { name: `Always load the pictures held by ${HOST}` }).click();
    await expect(page.locator(".toast-text")).toContainText(HOST);
    await expect.poll(() => attempted).toContain(PORTRAIT);
    const trusted = await (await request.get(`${apiUrl}/api/v1/trees/${treeId}`)).json();
    expect(trusted.trusted_media_hosts).toEqual([HOST]);

    // It holds across a reload: the picture is requested without a click.
    attempted.length = 0;
    await page.reload();
    await expect(page.locator(".pd-header-top")).toBeVisible();
    await expect.poll(() => attempted).toContain(PORTRAIT);
    await expect(asked).toHaveCount(0);
    // The private address stays refused, whatever was trusted.
    await expect(refused).toHaveCount(1);
    expect(attempted.filter((url) => url.startsWith(PRIVATE))).toEqual([]);
});

test("the tree settings list the trusted hosts, add one by name and stop trusting one", async ({ page, request }, testInfo) => {
    const { treeId, personId } = await importedTree(request, `E2E trusted hosts ${testInfo.workerIndex}-${Date.now()}`);
    const attempted = remoteRequests(page);

    await page.goto(`/trees/${treeId}/settings`);
    await page.getByRole("button", { name: "Privacy", exact: true }).click();
    const card = page.locator(".settings-card", { hasText: "Pictures held by other sites" });
    await expect(card).toContainText("No host is trusted yet.");

    // A private host is not one to trust.
    const field = card.getByRole("textbox", { name: "Host name or address of a picture" });
    await field.fill("192.168.1.10");
    await card.getByRole("button", { name: "Trust a host", exact: true }).click();
    await expect(card).toContainText("This is not a public host name.");

    // The address of a picture names its host.
    await field.fill(PORTRAIT);
    await card.getByRole("button", { name: "Trust a host", exact: true }).click();
    await expect(card.locator(".settings-host-name")).toHaveText([HOST]);

    // The person's picture is now requested without a click.
    await page.goto(`/trees/${treeId}/persons/${personId}`);
    await expect.poll(() => attempted).toContain(PORTRAIT);
    await expect(page.locator(".media-tile .remote-gate.is-tile:not(.is-refused)")).toHaveCount(0);

    // The owner stops trusting it: the picture waits again.
    await page.goto(`/trees/${treeId}/settings`);
    await page.getByRole("button", { name: "Privacy", exact: true }).click();
    await card.getByRole("button", { name: `Stop trusting ${HOST}` }).click();
    await expect(card).toContainText("No host is trusted yet.");
    const read = await (await request.get(`${apiUrl}/api/v1/trees/${treeId}`)).json();
    expect(read.trusted_media_hosts).toEqual([]);
});
