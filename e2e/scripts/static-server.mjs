// Serves the built web bundle. A path without a file extension that names no
// file is a client-side route, answered with `index.html` so the router
// handles deep links; a missing asset is a plain 404.
//
// Dependency-free on purpose: the e2e suite pulls in Playwright and nothing
// else.

import { createReadStream, statSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, normalize, resolve, sep } from "node:path";

const repository = resolve(import.meta.dirname, "..", "..");
const root = resolve(
    process.env.E2E_WEB_ROOT ?? join(repository, "target", "dx", "oxidgene-web", "debug", "web", "public"),
);
const port = Number(process.env.E2E_WEB_PORT ?? "18081");
// What a deployment serves next to the bundle: the web image's runtime
// configuration, which the page loads before the WebAssembly.
const deployed = { "/runtime-config.js": join(repository, "docker", "runtime-config.js") };

const TYPES = {
    ".html": "text/html; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
    ".mjs": "text/javascript; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".json": "application/json",
    ".wasm": "application/wasm",
    ".svg": "image/svg+xml",
    ".png": "image/png",
    ".ico": "image/x-icon",
    ".woff2": "font/woff2",
};

function isFile(path) {
    try {
        return statSync(path).isFile();
    } catch {
        return false;
    }
}

try {
    statSync(join(root, "index.html"));
} catch {
    console.error(`No web bundle under ${root}: run \`just e2e\`, which builds it first.`);
    process.exit(1);
}

createServer((request, response) => {
    const pathname = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    const candidate = normalize(join(root, pathname));
    const inside = candidate === root || candidate.startsWith(root + sep);
    let file = join(root, "index.html");
    if (inside && isFile(candidate)) {
        file = candidate;
    } else if (Object.hasOwn(deployed, pathname)) {
        file = deployed[pathname];
    } else if (extname(pathname) !== "") {
        response.writeHead(404).end();
        return;
    }
    response.writeHead(200, {
        "content-type": TYPES[extname(file)] ?? "application/octet-stream",
        // Each test run starts from an empty browser cache, so the debug
        // WebAssembly (tens of megabytes) can be kept for the run; the page
        // itself is always fetched again.
        "cache-control": extname(file) === ".html" ? "no-cache" : "max-age=3600",
    });
    createReadStream(file).pipe(response);
}).listen(port, "127.0.0.1");
