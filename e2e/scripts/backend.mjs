// Starts `oxidgene-server` on a throwaway SQLite database and media root.
//
// Playwright runs this as a web server. Every run gets a fresh temporary
// directory, so the tests never see a developer's data and never leave any
// behind: the directory is removed when the server stops.

import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const repository = resolve(import.meta.dirname, "..", "..");
const scratch = mkdtempSync(join(tmpdir(), "oxidgene-e2e-"));
const apiPort = process.env.E2E_API_PORT ?? "18080";
const webPort = process.env.E2E_WEB_PORT ?? "18081";

const server = spawn("cargo", ["run", "--locked", "--quiet", "--package", "oxidgene-server"], {
    cwd: repository,
    stdio: "inherit",
    env: {
        ...process.env,
        OXIDGENE_HOST: "127.0.0.1",
        OXIDGENE_PORT: apiPort,
        OXIDGENE_DATABASE_URL: `sqlite://${join(scratch, "e2e.db")}?mode=rwc`,
        OXIDGENE_MEDIA_BACKEND: "filesystem",
        OXIDGENE_MEDIA_ROOT: join(scratch, "media"),
        OXIDGENE_WORK_DIR: join(scratch, "work"),
        OXIDGENE_CORS_ORIGIN: `http://127.0.0.1:${webPort}`,
        OXIDGENE_LOG_LEVEL: process.env.OXIDGENE_LOG_LEVEL ?? "warn",
    },
});

let stopping = false;
function stop(signal) {
    if (stopping) return;
    stopping = true;
    server.kill(signal);
}
process.on("SIGINT", () => stop("SIGINT"));
process.on("SIGTERM", () => stop("SIGTERM"));

server.on("exit", (code, signal) => {
    rmSync(scratch, { recursive: true, force: true });
    process.exit(stopping ? 0 : (code ?? (signal ? 1 : 0)));
});
