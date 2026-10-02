// The fictitious family the screenshots show: the Landrevel tree.
//
// Every person, date, note, source and repository here is invented. Only the
// places are real municipalities, so the statistics can put them on the map;
// nobody listed ever lived in them. The photographs are public-domain studio
// portraits of anonymous sitters (see ../fixtures/media/CREDITS.md), cast as
// these fictitious persons.
//
// The tree is built by code rather than committed as a file so that it stays
// readable: a hand-written core of four generations around the SOSA root,
// with their photographs, then generated ancestors, siblings and cousins
// drawn from a seeded generator. The same seed gives the same tree, which is
// what lets `just screenshots` reproduce the committed images.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { crc32 } from "node:zlib";

const mediaDir = join(import.meta.dirname, "..", "fixtures", "media");

// ── Deterministic randomness ────────────────────────────────────────────────

/// mulberry32: a small, well-mixed 32-bit generator, seeded.
function generator(seed: number): () => number {
    let state = seed >>> 0;
    return () => {
        state = (state + 0x6d2b79f5) >>> 0;
        let t = state;
        t = Math.imul(t ^ (t >>> 15), t | 1);
        t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
        return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
}

const random = generator(1898);
const between = (low: number, high: number) => low + Math.floor(random() * (high - low + 1));
const pick = <T>(items: readonly T[]): T => items[Math.floor(random() * items.length)];
const chance = (probability: number) => random() < probability;

// ── Vocabulary ──────────────────────────────────────────────────────────────

const MONTHS = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];

/// Real municipalities, written the way the place dictionary files them.
const PLACES = {
    quimper: "Quimper, Finistère, Bretagne, France",
    locronan: "Locronan, Finistère, Bretagne, France",
    plonevez: "Plonévez-Porzay, Finistère, Bretagne, France",
    douarnenez: "Douarnenez, Finistère, Bretagne, France",
    pontLabbe: "Pont-l'Abbé, Finistère, Bretagne, France",
    plobannalec: "Plobannalec-Lesconil, Finistère, Bretagne, France",
    concarneau: "Concarneau, Finistère, Bretagne, France",
    chateaulin: "Châteaulin, Finistère, Bretagne, France",
    crozon: "Crozon, Finistère, Bretagne, France",
    brest: "Brest, Finistère, Bretagne, France",
    morlaix: "Morlaix, Finistère, Bretagne, France",
    landerneau: "Landerneau, Finistère, Bretagne, France",
    quimperle: "Quimperlé, Finistère, Bretagne, France",
    briec: "Briec, Finistère, Bretagne, France",
    pouldreuzic: "Pouldreuzic, Finistère, Bretagne, France",
    penmarch: "Penmarch, Finistère, Bretagne, France",
    lorient: "Lorient, Morbihan, Bretagne, France",
    hennebont: "Hennebont, Morbihan, Bretagne, France",
    vannes: "Vannes, Morbihan, Bretagne, France",
    auray: "Auray, Morbihan, Bretagne, France",
    pontivy: "Pontivy, Morbihan, Bretagne, France",
    saintBrieuc: "Saint-Brieuc, Côtes-d'Armor, Bretagne, France",
    guingamp: "Guingamp, Côtes-d'Armor, Bretagne, France",
    rennes: "Rennes, Ille-et-Vilaine, Bretagne, France",
    nantes: "Nantes, Loire-Atlantique, Pays de la Loire, France",
    paris: "Paris, Paris, Île-de-France, France",
    leHavre: "Le Havre, Seine-Maritime, Normandie, France",
    brooklyn: "Brooklyn, Kings, New York, USA",
    // Outside the place dictionary: the tools list it as not located.
    montreal: "Montréal, Québec, Canada",
} as const;
type Town = keyof typeof PLACES;

/// Where generated families live, weighted towards the Cornouaille coast.
const HOMES: Town[] = [
    "locronan", "plonevez", "douarnenez", "pontLabbe", "plobannalec", "concarneau", "chateaulin",
    "crozon", "briec", "pouldreuzic", "penmarch", "quimper", "quimperle", "landerneau", "morlaix",
    "hennebont", "auray", "pontivy", "guingamp", "lorient",
];
/// Where some of the generated children end up.
const AWAY: Town[] = ["brest", "lorient", "nantes", "rennes", "paris", "paris", "leHavre", "vannes", "saintBrieuc", "brooklyn"];

const MALE = ["Jean", "Yves", "Pierre", "Joseph", "Corentin", "Guillaume", "François", "Hervé", "Louis", "Alain",
    "Jacques", "René", "Mathurin", "Nicolas", "Jean-Marie", "Guénolé", "Michel", "Christophe", "Julien", "Tanguy"];
const FEMALE = ["Marie", "Anne", "Catherine", "Jeanne", "Marie-Anne", "Perrine", "Françoise", "Marguerite", "Louise",
    "Corentine", "Marie-Jeanne", "Julienne", "Hélène", "Renée", "Guillemette", "Mathurine", "Isabelle", "Barbe"];
/// Invented surnames, Breton in sound.
const SURNAMES = ["RIVALAIN", "COATMÉNEC", "KERDANIEL", "LE GUÉDIC", "PENHOADIC", "TRÉGOAT", "LANNUZEL",
    "MORVÉZEN", "QUILVÉRÉ", "BRÉLIVET", "LE BIHANIC", "CARADEUC", "HAMONIC", "TANGUYOT", "JAOUENNEC",
    "KERVAREC", "LE DRÉAU", "GOURMELEN", "STANGUÉ", "LE CORVEC"];
const TRADES_M = ["Farmer", "Fisherman", "Weaver", "Miller", "Blacksmith", "Carpenter", "Day labourer", "Sailor",
    "Baker", "Stonemason", "Innkeeper", "Tailor", "Clogmaker", "Rope maker"];
const TRADES_F = ["Farmer", "Seamstress", "Spinner", "Laundress", "Servant", "Cannery worker", "Lacemaker",
    "Shopkeeper", "Midwife"];

// ── Records ─────────────────────────────────────────────────────────────────

interface Dated {
    year: number;
    month?: number;
    day?: number;
    /// GEDCOM qualifier written before the date: ABT, BEF, AFT, EST.
    qualifier?: string;
}

interface Fact {
    tag: string;
    date?: Dated;
    place?: Town;
    /// Free text after the tag (an occupation's title).
    value?: string;
    age?: string;
    agency?: string;
    cause?: string;
    source?: { id: string; page: string; quality: number };
    note?: string;
    media?: string[];
}

interface Person {
    id: string;
    sex: "M" | "F";
    given: string;
    surname: string;
    facts: Fact[];
    famc?: string;
    fams: string[];
    notes: string[];
    media: string[];
    godparents: Array<{ id: string; role: string }>;
    /// Year of birth, kept for generation arithmetic.
    born: number;
    home: Town;
}

interface Family {
    id: string;
    husband?: string;
    wife?: string;
    children: string[];
    facts: Fact[];
    media: string[];
    witnesses: Array<{ id: string; role: string }>;
}

interface Medium {
    id: string;
    file: string;
    title: string;
    type: string;
    vignettes: Array<{ person: string; x: number; y: number; width: number; height: number }>;
}

const persons = new Map<string, Person>();
const families = new Map<string, Family>();
const media: Medium[] = [];
let nextPerson = 1;
let nextFamily = 1;

const SOURCES = [
    { id: "S1", title: "Parish registers of Locronan, baptisms, marriages and burials, 1760–1792", author: "Parish of Locronan", repo: "R1", call: "SAMPLE-PR-12" },
    { id: "S2", title: "Civil registers of Quimper, births, 1793–1902", author: "Registry office of Quimper", repo: "R1", call: "SAMPLE-EC-34" },
    { id: "S3", title: "Census of 1906, Quimper", author: "Municipality of Quimper", repo: "R1", call: "SAMPLE-CENS-1906" },
    { id: "S4", title: "Landrevel family papers: letters, school reports and photographs", author: "Landrevel family", repo: "R2", call: "BOX-3" },
    { id: "S5", title: "Civil registers of Lorient, marriages, 1890–1930", author: "Registry office of Lorient", repo: "R1", call: "SAMPLE-EC-56" },
] as const;

function date(d: Dated): string {
    const parts = [d.day && d.month ? String(d.day) : "", d.month ? MONTHS[d.month - 1] : "", String(d.year)].filter(Boolean);
    return [d.qualifier ?? "", ...parts].filter(Boolean).join(" ");
}

function day(year: number): Dated {
    return { year, month: between(1, 12), day: between(1, 28) };
}

function addPerson(sex: "M" | "F", given: string, surname: string, born: Dated, home: Town): Person {
    const id = `I${nextPerson++}`;
    const person: Person = {
        id, sex, given, surname, born: born.year, home,
        facts: [{ tag: "BIRT", date: born, place: home }],
        fams: [], notes: [], media: [], godparents: [],
    };
    persons.set(id, person);
    return person;
}

function addFamily(husband: Person | undefined, wife: Person | undefined, married?: Dated, place?: Town): Family {
    const id = `F${nextFamily++}`;
    const family: Family = { id, husband: husband?.id, wife: wife?.id, children: [], facts: [], media: [], witnesses: [] };
    if (married) family.facts.push({ tag: "MARR", date: married, place });
    husband?.fams.push(id);
    wife?.fams.push(id);
    families.set(id, family);
    return family;
}

function addChild(family: Family, child: Person): void {
    family.children.push(child.id);
    child.famc = family.id;
}

/// The day `days` after a complete date.
function later(d: Dated, days: number): Dated {
    const at = new Date(Date.UTC(d.year, (d.month ?? 1) - 1, d.day ?? 1));
    at.setUTCDate(at.getUTCDate() + days);
    return { year: at.getUTCFullYear(), month: at.getUTCMonth() + 1, day: at.getUTCDate() };
}

function die(person: Person, died: Dated, place: Town, burial = true): void {
    person.facts.push({ tag: "DEAT", date: died, place });
    if (burial) person.facts.push({ tag: "BURI", date: later(died, 2), place });
}

function work(person: Person, trade: string, from?: number): void {
    person.facts.push({ tag: "OCCU", value: trade, date: from ? { year: from } : undefined, place: person.home });
}

function photo(person: Person, file: string, title: string): void {
    const id = `M${media.length + 1}`;
    media.push({ id, file: `media/portraits/${file}`, title, type: "photo", vignettes: [] });
    person.media.push(id);
}

// ── The hand-written core ───────────────────────────────────────────────────

const etienne = addPerson("M", "Étienne", "LANDREVEL", { year: 1898, month: 3, day: 14 }, "quimper");
const marguerite = addPerson("F", "Marguerite", "DAULAC", { year: 1901, month: 6, day: 2 }, "lorient");
const louis = addPerson("M", "Louis", "LANDREVEL", { year: 1868, month: 9, day: 21 }, "quimper");
const anne = addPerson("F", "Anne", "CORVAISIER", { year: 1872, month: 1, day: 30 }, "pontLabbe");
const yves = addPerson("M", "Yves", "LANDREVEL", { year: 1839, month: 11, day: 4 }, "locronan");
const marieJeanne = addPerson("F", "Marie-Jeanne", "PENNAMEN", { year: 1842, month: 5, day: 17 }, "plonevez");
const corentin = addPerson("M", "Corentin", "CORVAISIER", { year: 1840, month: 12, day: 8 }, "pontLabbe");
const catherine = addPerson("F", "Catherine", "TRÉVEUR", { year: 1845, month: 7, day: 25 }, "plobannalec");
const herve = addPerson("M", "Hervé", "LANDREVEL", { year: 1808, month: 2, day: 11 }, "locronan");
const perrine = addPerson("F", "Perrine", "GUÉDAUX", { year: 1813, month: 10, day: 3 }, "locronan");
const joseph = addPerson("M", "Joseph", "DAULAC", { year: 1870, month: 4, day: 12 }, "lorient");
const berthe = addPerson("F", "Berthe", "MORHALLAN", { year: 1875, month: 8, day: 19 }, "hennebont");
const henri = addPerson("M", "Henri", "LANDREVEL", { year: 1895, month: 7, day: 6 }, "quimper");
const louise = addPerson("F", "Louise", "LANDREVEL", { year: 1901, month: 11, day: 23 }, "quimper");
const francois = addPerson("M", "François", "LANDREVEL", { year: 1866, month: 5, day: 9 }, "locronan");
const marie = addPerson("F", "Marie", "LANDREVEL", { year: 1870, month: 10, day: 15 }, "locronan");
const jean = addPerson("M", "Jean", "CORVAISIER", { year: 1874, month: 3, day: 3 }, "pontLabbe");
const germaine = addPerson("F", "Germaine", "CORVAISIER", { year: 1878, month: 9, day: 28 }, "pontLabbe");

// Étienne, the SOSA root.
etienne.facts[0].source = { id: "S2", page: "Births 1898, entry 212", quality: 3 };
etienne.facts[0].agency = "Registry office of Quimper";
etienne.facts.push({ tag: "BAPM", date: { year: 1898, month: 3, day: 16 }, place: "quimper" });
work(etienne, "Schoolteacher", 1919);
etienne.facts.push({ tag: "GRAD", date: { year: 1918, month: 7 }, place: "rennes", value: "Teaching certificate" });
etienne.facts.push({ tag: "RESI", date: { year: 1931 }, place: "paris", source: { id: "S4", page: "Letter to his brother, spring 1931", quality: 2 } });
die(etienne, { year: 1971, month: 2, day: 2 }, "paris");
etienne.facts.find((f) => f.tag === "DEAT")!.age = "72y";
etienne.notes.push(
    "Taught for thirty years at the boys' school of Quimper, then in Paris, where the family settled in 1931. " +
    "Kept a diary of the Quimper years, now among the family papers.",
);
photo(etienne, "rijksmuseum-rp-f-f18718.jpg", "Étienne Landrevel, about 1920");
etienne.godparents.push({ id: francois.id, role: "Godfather" }, { id: germaine.id, role: "Godmother" });

marguerite.facts[0].source = { id: "S5", page: "Marriages 1923, entry 87 (bride's birth noted)", quality: 2 };
work(marguerite, "Milliner", 1919);
die(marguerite, { year: 1984, month: 12, day: 9 }, "paris");
photo(marguerite, "rijksmuseum-rp-f-f19090.jpg", "Marguerite Daulac, about 1921");

louis.facts[0].source = { id: "S2", page: "Births 1868, entry 401", quality: 3 };
work(louis, "Carpenter", 1886);
louis.facts.push({ tag: "CENS", date: { year: 1906 }, place: "quimper", source: { id: "S3", page: "Rue Kéréon, household 112", quality: 3 } });
die(louis, { year: 1934, month: 1, day: 17 }, "quimper");
photo(louis, "rijksmuseum-rp-f-f18703.jpg", "Louis Landrevel, about 1900");
louis.notes.push("Master carpenter; built the pulpit stairs of a chapel near Locronan, as his father liked to tell.");

work(anne, "Seamstress", 1888);
die(anne, { year: 1950, month: 4, day: 2 }, "paris");
photo(anne, "rijksmuseum-rp-f-f19089.jpg", "Anne Corvaisier, about 1905");

work(yves, "Weaver", 1858);
die(yves, { year: 1901, month: 3, day: 30 }, "locronan");
yves.facts.find((f) => f.tag === "DEAT")!.age = "67y"; // The record is off by six years.
photo(yves, "rijksmuseum-rp-f-f18704.jpg", "Yves Landrevel, about 1885");
yves.facts[0].source = { id: "S1", page: "Baptisms 1839, folio 31", quality: 2 };

work(marieJeanne, "Spinner", 1860);
die(marieJeanne, { year: 1910, month: 8, day: 14 }, "locronan");
photo(marieJeanne, "rijksmuseum-rp-f-f18891.jpg", "Marie-Jeanne Pennamen, about 1900");

work(corentin, "Fisherman", 1858);
die(corentin, { year: 1895, month: 1, day: 6 }, "pontLabbe");
corentin.facts.find((f) => f.tag === "DEAT")!.cause = "Lost at sea";
// A census entry copied onto the wrong man: after his death, for the anomalies tab.
corentin.facts.push({ tag: "CENS", date: { year: 1896 }, place: "pontLabbe" });
photo(corentin, "rijksmuseum-rp-f-f18662.jpg", "Corentin Corvaisier, about 1890");

work(catherine, "Lacemaker", 1862);
die(catherine, { year: 1921, month: 11, day: 11 }, "pontLabbe");
photo(catherine, "rijksmuseum-rp-f-f18666.jpg", "Catherine Tréveur, about 1895");

work(herve, "Weaver", 1828);
die(herve, { year: 1879, month: 6, day: 2 }, "locronan");
photo(herve, "rijksmuseum-rp-f-f19061.jpg", "Hervé Landrevel, about 1875");
work(perrine, "Spinner", 1830);
die(perrine, { year: 1888, month: 2, day: 20 }, "locronan");
photo(perrine, "rijksmuseum-rp-f-f18911.jpg", "Perrine Guédaux, about 1880");

work(joseph, "Sailmaker", 1888);
die(joseph, { year: 1938, month: 10, day: 1 }, "lorient");
work(berthe, "Cannery worker", 1891);
die(berthe, { year: 1949, month: 5, day: 24 }, "lorient");

work(henri, "Sailor", 1913);
henri.facts.push({ tag: "EMIG", date: { year: 1921, month: 4 }, place: "leHavre" });
die(henri, { year: 1962, month: 9, day: 3 }, "brooklyn");
photo(henri, "rijksmuseum-rp-f-f18714.jpg", "Henri Landrevel, about 1915");

work(louise, "Lacemaker", 1917);
die(louise, { year: 1990, month: 1, day: 12 }, "quimper");
photo(louise, "rijksmuseum-rp-f-f19082.jpg", "Louise Landrevel, about 1920");

work(francois, "Clogmaker", 1884);
die(francois, { year: 1931, month: 7, day: 7 }, "locronan");
photo(francois, "rijksmuseum-rp-f-f18726.jpg", "François Landrevel, about 1895");
work(marie, "Servant", 1886);
die(marie, { year: 1944, month: 3, day: 18 }, "brest");
photo(marie, "rijksmuseum-rp-f-f18782.jpg", "Marie Landrevel, about 1890");
work(jean, "Fisherman", 1888);
die(jean, { year: 1927, month: 12, day: 30 }, "pontLabbe");
photo(jean, "rijksmuseum-rp-f-f18913.jpg", "Jean Corvaisier, about 1900");
work(germaine, "Seamstress", 1894);
die(germaine, { year: 1958, month: 6, day: 6 }, "douarnenez");
photo(germaine, "rijksmuseum-rp-f-f18884.jpg", "Germaine Corvaisier, about 1898");

// The families of the core.
const fEtienne = addFamily(etienne, marguerite, { year: 1923, month: 9, day: 15 }, "lorient");
fEtienne.facts[0].source = { id: "S5", page: "Marriages 1923, entry 87", quality: 3 };
const fLouis = addFamily(louis, anne, { year: 1893, month: 5, day: 2 }, "pontLabbe");
const fYves = addFamily(yves, marieJeanne, { year: 1864, month: 1, day: 26 }, "plonevez");
const fCorentin = addFamily(corentin, catherine, { year: 1867, month: 10, day: 8 }, "plobannalec");
const fHerve = addFamily(herve, perrine, { year: 1834, month: 6, day: 24 }, "locronan");
const fJoseph = addFamily(joseph, berthe, { year: 1896, month: 6, day: 20 }, "hennebont");
addChild(fLouis, henri);
addChild(fLouis, etienne);
addChild(fLouis, louise);
addChild(fYves, francois);
addChild(fYves, louis);
addChild(fYves, marie);
addChild(fCorentin, anne);
addChild(fCorentin, jean);
addChild(fCorentin, germaine);
addChild(fHerve, yves);
addChild(fJoseph, marguerite);
fLouis.witnesses.push({ id: francois.id, role: "Witness" }, { id: jean.id, role: "Witness" });

// The wedding photograph of Joseph and Berthe, each identified on it.
media.push({
    id: `M${media.length + 1}`,
    file: "media/portraits/rijksmuseum-rp-f-f18900.jpg",
    title: "Joseph Daulac and Berthe Morhallan, wedding photograph",
    type: "photo",
    vignettes: [
        { person: berthe.id, x: 200, y: 140, width: 100, height: 125 },
        { person: joseph.id, x: 345, y: 240, width: 100, height: 125 },
    ],
});
fJoseph.media.push(media[media.length - 1].id);

// The documents.
media.push({ id: `M${media.length + 1}`, file: "media/documents/poessneck-register-1793.jpg", title: "Baptism register, page of 1793", type: "manuscript", vignettes: [] });
herve.media.push(media[media.length - 1].id);
media.push({ id: `M${media.length + 1}`, file: "media/documents/lorient-postcard-1905.jpg", title: "Lorient, the harbour, postcard sent in 1905", type: "card", vignettes: [] });
fJoseph.media.push(media[media.length - 1].id);
etienne.media.push(media[media.length - 1].id);

// Étienne and Marguerite's children and grandchildren, for the descendant charts.
const children = [
    addPerson("F", "Jeanne", "LANDREVEL", { year: 1924, month: 7, day: 8 }, "quimper"),
    addPerson("M", "Paul", "LANDREVEL", { year: 1927, month: 2, day: 19 }, "quimper"),
    addPerson("F", "Hélène", "LANDREVEL", { year: 1931, month: 10, day: 4 }, "paris"),
];
for (const child of children) addChild(fEtienne, child);
const [jeanne, paul, helene] = children;
work(jeanne, "Nurse", 1946);
work(paul, "Engineer", 1951);
work(helene, "Librarian", 1955);
die(paul, { year: 2003, month: 5, day: 30 }, "nantes");
const jeanneSpouse = addPerson("M", "André", "KERVAREC", { year: 1921, month: 4, day: 2 }, "brest");
const fJeanne = addFamily(jeanneSpouse, jeanne, { year: 1947, month: 8, day: 30 }, "paris");
const paulSpouse = addPerson("F", "Simone", "LE CORVEC", { year: 1930, month: 1, day: 13 }, "nantes");
const fPaul = addFamily(paul, paulSpouse, { year: 1953, month: 6, day: 13 }, "nantes");
for (const [family, surname, home, years] of [
    [fJeanne, "KERVAREC", "paris", [1948, 1951, 1955]],
    [fPaul, "LANDREVEL", "nantes", [1954, 1958]],
] as const) {
    for (const year of years) {
        const sex = chance(0.5) ? "M" : "F";
        const child = addPerson(sex, pick(sex === "M" ? ["Alain", "Bernard", "Michel", "Yann"] : ["Anne", "Sylvie", "Catherine", "Martine"]), surname, day(year), home);
        addChild(family, child);
    }
}

// ── Generated ancestors, siblings and cousins ───────────────────────────────

/// Give `child` parents, of generation `generation` (the SOSA root's is 1),
/// and theirs in turn up to generation `max`. The further up, the more
/// often a parent is unknown, as in any real tree.
function ancestry(child: Person, generation: number, max: number): void {
    if (generation > max) return;
    const missing = generation >= 7 ? 0.3 : generation >= 6 ? 0.15 : 0;
    const home: Town = chance(0.75) ? child.home : pick(HOMES);
    const fatherBorn = child.born - between(25, 38);
    const motherBorn = child.born - between(21, 36);
    // A child carries the father's surname.
    const father = chance(missing) ? undefined : addPerson("M", pick(MALE), child.surname, day(fatherBorn), home);
    const mother = chance(missing) ? undefined : addPerson("F", pick(FEMALE), pick(SURNAMES), day(motherBorn), home);
    if (!father && !mother) return;
    const married = Math.min(child.born - 1, Math.max(fatherBorn, motherBorn) + between(20, 27));
    const family = addFamily(father, mother, chance(0.85) ? day(married) : undefined, home);
    if (family.facts[0] && generation <= 6) {
        family.facts[0].source = { id: "S1", page: `Marriages ${married}, folio ${between(2, 60)}`, quality: between(1, 3) };
    }
    addChild(family, child);
    siblings(family, child, generation - 1);
    const lastChild = Math.max(...family.children.map((id) => persons.get(id)!.born));
    for (const parent of [father, mother]) {
        if (!parent) continue;
        lifeOf(parent, lastChild);
        ancestry(parent, generation + 1, max);
    }
}

/// Death, burial and trade of a generated person who had a child in `childYear`.
function lifeOf(person: Person, childYear: number): void {
    work(person, pick(person.sex === "M" ? TRADES_M : TRADES_F));
    const died = Math.max(childYear + between(1, 30), person.born + between(45, 88));
    if (died < 1960) die(person, day(died), chance(0.85) ? person.home : pick(HOMES));
    if (person.born < 1793 && chance(0.6)) {
        const birth = person.facts[0].date!;
        person.facts.push({ tag: "BAPM", date: later(birth, between(0, 2)), place: person.home, source: { id: "S1", page: `Baptisms ${person.born}, folio ${between(2, 60)}`, quality: between(2, 3) } });
    }
}

/// Brothers and sisters of `child` in `family`, and for the nearer
/// generations their own families: the cousins.
function siblings(family: Family, child: Person, generation: number): void {
    const count = between(generation <= 3 ? 1 : 0, generation <= 4 ? 4 : 2);
    // Younger brothers and sisters: the parents' dates are drawn from the
    // eldest child up, so nobody is born before their parents could be.
    let year = Math.max(child.born, ...family.children.map((id) => persons.get(id)!.born));
    for (let i = 0; i < count; i++) {
        year += between(2, 3);
        const sex = chance(0.5) ? "M" : "F";
        const father = family.husband ? persons.get(family.husband) : undefined;
        const surname = father?.surname ?? child.surname;
        const home = child.home;
        const sibling = addPerson(sex, pick(sex === "M" ? MALE : FEMALE), surname, day(year), home);
        addChild(family, sibling);
        if (chance(0.25)) {
            // Died in childhood.
            die(sibling, day(year + between(1, 6)), home, false);
            continue;
        }
        const away = chance(0.2);
        if (away) sibling.home = pick(AWAY);
        work(sibling, pick(sex === "M" ? TRADES_M : TRADES_F));
        const died = year + between(40, 90);
        if (died < 1975) die(sibling, day(died), sibling.home);
        if (generation <= 4 && chance(0.7)) cousins(sibling, generation);
    }
}

function cousins(person: Person, generation: number): void {
    const sex = person.sex === "M" ? "F" : "M";
    const spouse = addPerson(sex, pick(sex === "M" ? MALE : FEMALE), pick(SURNAMES), day(person.born + between(-4, 4)), person.home);
    lifeOf(spouse, person.born + 30);
    const married = Math.max(person.born, spouse.born) + between(20, 28);
    const family = person.sex === "M" ? addFamily(person, spouse, day(married), person.home) : addFamily(spouse, person, day(married), spouse.home);
    const surname = person.sex === "M" ? person.surname : spouse.surname;
    let year = married + 1;
    for (let i = between(1, 4); i > 0; i--) {
        const childSex = chance(0.5) ? "M" : "F";
        const cousin = addPerson(childSex, pick(childSex === "M" ? MALE : FEMALE), surname, day(year), family.husband === person.id ? person.home : spouse.home);
        addChild(family, cousin);
        if (year < 1930) {
            work(cousin, pick(childSex === "M" ? TRADES_M : TRADES_F));
            if (year + 70 < 1990) die(cousin, day(year + between(30, 85)), cousin.home);
        }
        if (generation <= 2 && chance(0.5)) cousins(cousin, generation + 2);
        year += between(2, 4);
    }
}

// An uncle who left for Montréal, a place the map cannot locate.
const emigrant = addPerson("M", "Guénolé", "LANDREVEL", { year: 1872, month: 2, day: 2 }, "locronan");
addChild(fYves, emigrant);
work(emigrant, "Sailor");
emigrant.facts.push({ tag: "EMIG", date: { year: 1899 }, place: "leHavre" });
die(emigrant, { year: 1940, month: 5, day: 5 }, "montreal");

// The core's own siblings, then everyone above it.
siblings(fYves, louis, 2);
siblings(fCorentin, anne, 2);
siblings(fHerve, yves, 3);
for (const person of [marieJeanne, corentin, catherine]) ancestry(person, 4, 8);
for (const person of [herve, perrine]) ancestry(person, 5, 8);
for (const person of [joseph, berthe]) ancestry(person, 3, 3);

// Two records of one man, entered twice from two registers: the potential
// duplicates tool offers to merge them.
const homonymOne = addPerson("M", "Jean-Marie", "RIVALAIN", { year: 1812, month: 4, day: 7 }, "locronan");
work(homonymOne, "Miller");
die(homonymOne, { year: 1871, month: 1, day: 9 }, "locronan");
const homonymTwo = addPerson("M", "Jean-Marie", "RIVALAIN", { year: 1812, qualifier: "ABT" }, "locronan");
homonymTwo.facts.push({ tag: "BURI", date: { year: 1871, month: 1, day: 11 }, place: "locronan" });
work(homonymTwo, "Miller");
// A godmother recorded as godfather, for the anomalies tab.
louis.godparents.push({ id: catherine.id, role: "Godfather" });
// ── GEDCOM ──────────────────────────────────────────────────────────────────

function factLines(fact: Fact): string[] {
    const lines = [`1 ${fact.tag}${fact.value ? ` ${fact.value}` : ""}`];
    if (fact.date) lines.push(`2 DATE ${date(fact.date)}`);
    if (fact.place) lines.push(`2 PLAC ${PLACES[fact.place]}`);
    if (fact.age) lines.push(`2 AGE ${fact.age}`);
    if (fact.agency) lines.push(`2 AGNC ${fact.agency}`);
    if (fact.cause) lines.push(`2 CAUS ${fact.cause}`);
    if (fact.source) lines.push(`2 SOUR @${fact.source.id}@`, `3 PAGE ${fact.source.page}`, `3 QUAY ${fact.source.quality}`);
    return lines;
}

function personLines(person: Person): string[] {
    const lines = [
        `0 @${person.id}@ INDI`,
        `1 NAME ${person.given} /${person.surname}/`,
        `2 GIVN ${person.given}`,
        `2 SURN ${person.surname}`,
        `1 SEX ${person.sex}`,
    ];
    for (const fact of person.facts) lines.push(...factLines(fact));
    for (const note of person.notes) lines.push(`1 NOTE ${note}`);
    for (const id of person.media) lines.push(`1 OBJE @${id}@`);
    for (const godparent of person.godparents) lines.push(`1 ASSO @${godparent.id}@`, `2 RELA ${godparent.role}`);
    if (person.famc) lines.push(`1 FAMC @${person.famc}@`);
    for (const id of person.fams) lines.push(`1 FAMS @${id}@`);
    return lines;
}

function familyLines(family: Family): string[] {
    const lines = [`0 @${family.id}@ FAM`];
    if (family.husband) lines.push(`1 HUSB @${family.husband}@`);
    if (family.wife) lines.push(`1 WIFE @${family.wife}@`);
    for (const fact of family.facts) lines.push(...factLines(fact));
    for (const id of family.children) lines.push(`1 CHIL @${id}@`);
    for (const id of family.media) lines.push(`1 OBJE @${id}@`);
    return lines;
}

/// Witnesses of a family's marriage are written on the witness's own record.
function witnessLines(): Map<string, string[]> {
    const byPerson = new Map<string, string[]>();
    for (const family of families.values()) {
        for (const witness of family.witnesses) {
            const lines = byPerson.get(witness.id) ?? [];
            lines.push(`1 ASSO @${family.id}@`, `2 RELA ${witness.role}`);
            byPerson.set(witness.id, lines);
        }
    }
    return byPerson;
}

function gedcom(): string {
    const lines = [
        "0 HEAD",
        "1 SOUR OXIDGENE_SCREENSHOTS",
        "1 GEDC",
        "2 VERS 5.5.1",
        "2 FORM LINEAGE-LINKED",
        "1 CHAR UTF-8",
        "1 SUBM @U1@",
        "0 @U1@ SUBM",
        "1 NAME Sample Researcher",
        "0 @R1@ REPO",
        "1 NAME Departmental Archives (sample)",
        "1 ADDR 1 Archive Street",
        "2 CONT Sampletown",
        "0 @R2@ REPO",
        "1 NAME Landrevel family collection (sample)",
    ];
    for (const source of SOURCES) {
        lines.push(`0 @${source.id}@ SOUR`, `1 TITL ${source.title}`, `1 AUTH ${source.author}`, `1 REPO @${source.repo}@`, `2 CALN ${source.call}`);
    }
    const witnesses = witnessLines();
    for (const person of persons.values()) {
        const record = personLines(person);
        const extra = witnesses.get(person.id);
        if (extra) {
            const links = record.findIndex((line) => line.startsWith("1 FAM"));
            record.splice(links < 0 ? record.length : links, 0, ...extra);
        }
        lines.push(...record);
    }
    for (const family of families.values()) lines.push(...familyLines(family));
    for (const medium of media) {
        lines.push(`0 @${medium.id}@ OBJE`, `1 FILE ${medium.file}`, "2 FORM image/jpeg", `3 TYPE ${medium.type}`, `1 TITL ${medium.title}`);
        for (const v of medium.vignettes) lines.push(`1 _OXIDGENE_VIGNETTE @${v.person}@ ${v.x} ${v.y} ${v.width} ${v.height}`);
    }
    lines.push("0 TRLR");
    return `${lines.join("\n")}\n`;
}

// ── GEDZIP ──────────────────────────────────────────────────────────────────

/// A ZIP archive of stored (uncompressed) entries, with a fixed timestamp:
/// the JPEGs are compressed already, and a fixed date keeps it byte-stable.
function zip(entries: Array<[string, Buffer]>): Buffer {
    const locals: Buffer[] = [];
    const centrals: Buffer[] = [];
    let offset = 0;
    const dosTime = 0;
    const dosDate = ((2026 - 1980) << 9) | (1 << 5) | 1;
    for (const [name, data] of entries) {
        const nameBytes = Buffer.from(name, "utf8");
        const crc = crc32(data);
        const local = Buffer.alloc(30);
        local.writeUInt32LE(0x04034b50, 0);
        local.writeUInt16LE(20, 4);
        local.writeUInt16LE(0x0800, 6); // UTF-8 names
        local.writeUInt16LE(0, 8); // stored
        local.writeUInt16LE(dosTime, 10);
        local.writeUInt16LE(dosDate, 12);
        local.writeUInt32LE(crc, 14);
        local.writeUInt32LE(data.length, 18);
        local.writeUInt32LE(data.length, 22);
        local.writeUInt16LE(nameBytes.length, 26);
        local.writeUInt16LE(0, 28);
        const central = Buffer.alloc(46);
        central.writeUInt32LE(0x02014b50, 0);
        central.writeUInt16LE(20, 4);
        central.writeUInt16LE(20, 6);
        central.writeUInt16LE(0x0800, 8);
        central.writeUInt16LE(0, 10);
        central.writeUInt16LE(dosTime, 12);
        central.writeUInt16LE(dosDate, 14);
        central.writeUInt32LE(crc, 16);
        central.writeUInt32LE(data.length, 20);
        central.writeUInt32LE(data.length, 24);
        central.writeUInt16LE(nameBytes.length, 28);
        central.writeUInt32LE(offset, 42);
        locals.push(local, nameBytes, data);
        centrals.push(central, nameBytes);
        offset += local.length + nameBytes.length + data.length;
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

/// The family as a GEDZIP archive: `gedcom.ged` and the photographs.
export function familyArchive(): Buffer {
    const files = [...new Set(media.map((medium) => medium.file))].map(
        (file): [string, Buffer] => [file, readFileSync(join(mediaDir, file.replace(/^media\//, "")))],
    );
    return zip([["gedcom.ged", Buffer.from(gedcom(), "utf8")], ...files]);
}

/// The persons the screenshots open, by role.
export const cast = {
    root: { given: "Étienne", surname: "LANDREVEL" },
    father: { given: "Louis", surname: "LANDREVEL" },
    mother: { given: "Anne", surname: "CORVAISIER" },
    spouse: { given: "Marguerite", surname: "DAULAC" },
    grandfather: { given: "Yves", surname: "LANDREVEL" },
    weddingCouple: [{ given: "Joseph", surname: "DAULAC" }, { given: "Berthe", surname: "MORHALLAN" }],
} as const;

/// A first cousin of the SOSA root, through a brother or sister of the
/// root's father, for the kinship screenshot.
export const firstCousin = (() => {
    for (const auntOrUncle of fYves.children.map((id) => persons.get(id)!)) {
        if (auntOrUncle === louis) continue;
        for (const familyId of auntOrUncle.fams) {
            const child = families.get(familyId)!.children[0];
            if (child) {
                const cousin = persons.get(child)!;
                return { given: cousin.given, surname: cousin.surname, born: cousin.born };
            }
        }
    }
    throw new Error("the generated tree has no first cousin of the root");
})();
