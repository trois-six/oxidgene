import { readFileSync } from "node:fs";
import { join } from "node:path";
import { crc32 } from "node:zlib";

import { apiUrl, expect, test } from "./fixtures";
import { png } from "./requests";

const fixture = readFileSync(join(import.meta.dirname, "..", "fixtures", "family-blocks.ged"));

test("a GEDCOM file is imported into a tree from its card's menu", async ({ page, request }) => {
    const name = `E2E import ${Date.now()}`;
    const created = await request.post(`${apiUrl}/api/v1/trees`, { data: { name } });
    expect(created.ok()).toBeTruthy();
    const treeId = (await created.json()).id as string;

    await page.goto("/");
    // Only this tree's card: the other tests' imports refresh the list.
    await page.locator(".home-search-input").fill(name);
    const card = page.locator(".tree-card").filter({ has: page.locator(".tree-card-name", { hasText: name }) });
    await card.getByRole("button", { name: `Actions for ${name}` }).click();
    await page.getByRole("menuitem", { name: "Import", exact: true }).click();

    const modal = page.locator(".import-modal");
    await expect(modal.getByText(`Import into “${name}”`)).toBeVisible();
    await expect(modal.getByRole("tab", { name: "A file" })).toBeVisible();
    await expect(modal.getByRole("tab", { name: "From Geneanet" })).toBeVisible();
    const start = modal.getByRole("button", { name: "Import", exact: true });
    await expect(start).toBeDisabled();

    await modal.locator("#oxidgene-file-import-input").setInputFiles({
        name: "family-blocks.ged",
        mimeType: "application/octet-stream",
        buffer: fixture,
    });
    await expect(modal.locator(".import-drop-name")).toHaveText("family-blocks.ged");
    await start.click();

    await expect(modal.getByRole("heading", { name: "Import complete" })).toBeVisible({ timeout: 30_000 });
    const people = modal.locator(".import-stat").filter({ has: page.getByText("people", { exact: true }) });
    await expect(people.locator(".import-stat-value")).toHaveText("30");

    const persons = await (await request.get(`${apiUrl}/api/v1/trees/${treeId}/persons?first=100`)).json();
    expect(persons.edges).toHaveLength(30);
});

/// A ZIP whose entries are stored, not deflated: the shape of a saved
/// Geneanet session (docs/geneanet-media-import.md §8).
function storedZip(entries: [string, Buffer][]): Buffer {
    const locals: Buffer[] = [];
    const centrals: Buffer[] = [];
    let offset = 0;
    for (const [name, data] of entries) {
        const fileName = Buffer.from(name, "utf8");
        const crc = crc32(data) >>> 0;
        const local = Buffer.alloc(30);
        local.writeUInt32LE(0x04034b50, 0);
        local.writeUInt16LE(20, 4);
        local.writeUInt16LE(0x21, 12);
        local.writeUInt32LE(crc, 14);
        local.writeUInt32LE(data.length, 18);
        local.writeUInt32LE(data.length, 22);
        local.writeUInt16LE(fileName.length, 26);
        const central = Buffer.alloc(46);
        central.writeUInt32LE(0x02014b50, 0);
        central.writeUInt16LE(20, 4);
        central.writeUInt16LE(20, 6);
        central.writeUInt16LE(0x21, 14);
        central.writeUInt32LE(crc, 16);
        central.writeUInt32LE(data.length, 20);
        central.writeUInt32LE(data.length, 24);
        central.writeUInt16LE(fileName.length, 28);
        central.writeUInt32LE(offset, 42);
        locals.push(local, fileName, data);
        centrals.push(central, fileName);
        offset += local.length + fileName.length + data.length;
    }
    const directory = Buffer.concat(centrals);
    const end = Buffer.alloc(22);
    end.writeUInt32LE(0x06054b50, 0);
    end.writeUInt16LE(entries.length, 8);
    end.writeUInt16LE(entries.length, 10);
    end.writeUInt32LE(directory.length, 12);
    end.writeUInt32LE(offset, 16);
    return Buffer.concat([...locals, directory, end]);
}

test("the web build imports a Geneanet tree with the media of a saved session", async ({ page, request }) => {
    const name = `E2E Geneanet session ${Date.now()}`;
    const created = await request.post(`${apiUrl}/api/v1/trees`, { data: { name } });
    expect(created.ok()).toBeTruthy();
    const treeId = (await created.json()).id as string;

    const gw = Buffer.from("encoding: utf-8\n\nfam FICTIF Ada.0 + EXEMPLE Basile.0\n", "utf8");
    const rendition = "https://example.invalid/normal.jpg";
    const session = storedZip([
        [
            "session.json",
            Buffer.from(
                JSON.stringify({
                    deposits: [{ id: 1, views: [{ id: 10, files: { normal: rendition } }] }],
                    references: [{ deposit: { id: 1, views: [{ id: 10 }] }, firstname: "Ada", lastname: "FICTIF", reference_extra_geneweb: { ref: "fictif|ada|" } }],
                    view_references: {},
                    oxidgene_media: { [rendition]: "media/00000.png" },
                    // Keyed by deposit: the browser's read hands them back
                    // with string keys.
                    oxidgene_deposit_sizes: { "1": 4096 },
                }),
                "utf8",
            ),
        ],
        ["media/00000.png", png(16, 16, 3)],
    ]);

    await page.goto("/");
    await page.locator(".home-search-input").fill(name);
    const card = page.locator(".tree-card").filter({ has: page.locator(".tree-card-name", { hasText: name }) });
    await card.getByRole("button", { name: `Actions for ${name}` }).click();
    await page.getByRole("menuitem", { name: "Import", exact: true }).click();
    const modal = page.locator(".import-modal");
    await modal.getByRole("tab", { name: "From Geneanet" }).click();

    // Step 1, the .gw.
    let chooser = page.waitForEvent("filechooser");
    await modal.getByRole("button", { name: "Choose the .gw file" }).click();
    await (await chooser).setFiles({ name: "family.gw", mimeType: "application/octet-stream", buffer: gw });

    // Step 3, which the web build cannot sign in from, loads the saved file.
    await expect(modal.getByText("This step needs the desktop app")).toBeVisible();
    chooser = page.waitForEvent("filechooser");
    await modal.getByRole("button", { name: "Load a saved file…" }).click();
    await (await chooser).setFiles({ name: "geneanet-session.zip", mimeType: "application/zip", buffer: session });

    // Step 4 previews it, and step 5 sends the file again with the tree.
    await modal.getByRole("button", { name: "Continue", exact: true }).click();
    await expect(modal.getByText("your media are in the file you loaded")).toBeVisible();
    await modal.getByRole("button", { name: "Start the import" }).click();
    await expect(modal.getByRole("heading", { name: "Import complete" })).toBeVisible({ timeout: 30_000 });

    const persons = await (await request.get(`${apiUrl}/api/v1/trees/${treeId}/persons?first=100`)).json();
    expect(persons.edges).toHaveLength(2);
    const media = await (await request.get(`${apiUrl}/api/v1/trees/${treeId}/media?first=100`)).json();
    expect(media.edges).toHaveLength(1);
});
