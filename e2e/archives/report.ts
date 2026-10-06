// The live checks' report (docs/archives.md §9.1): the shapes Rust writes
// (crates/oxidgene-archives/src/live/mod.rs), and the run's one JSON file,
// `report.json` under OXIDGENE_LIVE_REPORT_DIR, which the scheduled workflow
// reads.

import * as fs from "node:fs";
import * as path from "node:path";

export type Outcome = "ok" | "challenged" | "unreachable" | "drift";
export type Step = "search_page" | "discovery" | "resolution" | "opening" | "images";

export interface Failure {
    step: Step;
    outcome: Outcome;
    expected: string;
    received: string;
}

export interface Opening {
    platform: string;
    url: string;
    view: number;
    view_count: number | null;
    image: { picture: string; thumbnail: string; width: number; height: number } | null;
}

export interface CollectionReport {
    collection: string;
    index: number;
    platform: string;
    transport: "native" | "browser";
    outcome: Outcome;
    failure: Failure | null;
    requests: number;
    locality: string | null;
    citation: string | null;
    opening: Opening | null;
}

export interface ArchiveReport {
    archive: string;
    outcome: Outcome;
    collections: CollectionReport[];
}

// The anti-bot pages portals show in place of their own: the very file the
// adapters and the desktop's archive window classify pages with
// (crates/oxidgene-archives/src/platform/challenges.json, `markup::anti_bot`).
export interface Signature {
    vendor: string;
    guard: "challenge" | "block";
    markers: string[];
    unless?: string[];
}

const antiBot = JSON.parse(
    fs.readFileSync(new URL("../../crates/oxidgene-archives/src/platform/challenges.json", import.meta.url), "utf8"),
) as { signatures: Signature[]; widgets: string[] };

// The anti-bot page a page's markup is, if any: every marker of a signature
// and none of its `unless`, lower-cased, the first match winning.
export function antiBotPage(html: string): Signature | null {
    const lowered = html.toLowerCase();
    return (
        antiBot.signatures.find(
            (signature) => signature.markers.every((marker) => lowered.includes(marker)) && !(signature.unless ?? []).some((marker) => lowered.includes(marker)),
        ) ?? null
    );
}

// How a report names an anti-bot page: `cloudflare block`, `anubis challenge`.
export function antiBotName(signature: Signature): string {
    return `${signature.vendor} ${signature.guard}`;
}

const SEVERITY: Outcome[] = ["ok", "challenged", "unreachable", "drift"];

export function worst(outcomes: Outcome[]): Outcome {
    return outcomes.reduce<Outcome>((worst, outcome) => (SEVERITY.indexOf(outcome) > SEVERITY.indexOf(worst) ? outcome : worst), "ok");
}

export function drift(step: Step, expected: string, received: string): Failure {
    return { step, outcome: "drift", expected, received };
}

function readArchives(file: string): ArchiveReport[] {
    try {
        return (JSON.parse(fs.readFileSync(file, "utf8")) as { archives: ArchiveReport[] }).archives;
    } catch {
        return [];
    }
}

// The collections the Rust test checked over the native transport.
export function nativeReport(dir: string, archive: string): CollectionReport[] | null {
    const file = path.join(dir, "native.json");
    if (!fs.existsSync(file)) return null;
    return readArchives(file).find((report) => report.archive === archive)?.collections ?? null;
}

// Adds or replaces one archive's report in the run's report.
export function record(dir: string, report: ArchiveReport): void {
    fs.mkdirSync(dir, { recursive: true });
    const file = path.join(dir, "report.json");
    const archives = readArchives(file).filter((known) => known.archive !== report.archive);
    archives.push(report);
    fs.writeFileSync(file, `${JSON.stringify({ archives }, null, 2)}\n`);
}

// One line per failed collection, for the test annotations and the issues.
export function summary(report: ArchiveReport): string {
    const failed = report.collections.filter((collection) => collection.failure);
    if (failed.length === 0) return `${report.archive}: ${report.outcome}`;
    return failed
        .map(({ collection, failure }) => `${report.archive}/${collection}: ${failure!.outcome} at ${failure!.step}: expected ${failure!.expected}; received ${failure!.received}`)
        .join("\n");
}
