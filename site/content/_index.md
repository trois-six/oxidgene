+++
title = "OxidGene"
description = "A free genealogy application for the desktop and the web: import, explore, edit and question your family trees."

[extra]
headline = "Your family history, in your hands"
platforms = "Windows · macOS · Linux · your own web server — free software"
start = "@/guide/install.md"
tour_title = "A tour of the application"
# The tour, in the order of CAROUSEL in scripts/screenshots.py.
tour = [
    { name = "pedigree", caption = "The pedigree and its events panel" },
    { name = "person", caption = "A person's page" },
    { name = "events-timeline", caption = "A person's events with their sources and documents" },
    { name = "archive-register", caption = "A cited register opened on the archive's portal" },
    { name = "media-viewer", caption = "A photograph in the media viewer, with the persons it shows" },
    { name = "fan-chart-dark", caption = "A fan chart in the dark theme" },
    { name = "couple", caption = "A couple's page" },
    { name = "statistics-map", caption = "The places on a heat map" },
    { name = "kinship", caption = "Kinship between two cousins" },
    { name = "search", caption = "A search with its filters" },
    { name = "dictionary-sources", caption = "The sources of the dictionary" },
    { name = "dictionary-places", caption = "The places of the dictionary" },
    { name = "tags", caption = "The tags of the dictionary" },
    { name = "media-library", caption = "The media library" },
    { name = "tools-ancestry", caption = "Ancestry completeness, generation by generation" },
    { name = "history", caption = "A person's history, compared" },
    { name = "app-settings-dark", caption = "The application's themes" },
]

steps_title = "Start your genealogy in three steps"
steps = [
    { title = "Bring your tree in", text = "Import a GEDCOM, GEDZIP or GeneWeb file from your current software, or your Geneanet tree with its photos — or start from scratch.", link = "@/workflows/first-tree-from-gedcom.md" },
    { title = "Find your ancestors", text = "Add the people you find in the registers, with their sources, and open each cited register at the right page on the archives' portal.", link = "@/workflows/record-a-new-ancestor.md" },
    { title = "Tell the family story", text = "Charts, person pages written as a narrative, statistics on a map, kinship between cousins: share what you found.", link = "@/workflows/explore-and-share.md" },
]

features_title = "Everything a genealogist needs"
features = [
    { title = "Import from anywhere", text = "GEDCOM 5.5.1 and 7.0, GEDZIP with its media, GeneWeb, and Geneanet trees with their photos.", link = "@/guide/import-export.md" },
    { title = "Charts of every kind", text = "An interactive pedigree up to ten generations each way, fan charts, wheels, hourglass and bowtie, in a classic or heraldic style.", link = "@/guide/pedigree.md" },
    { title = "Person pages that tell a story", text = "Family connections written as a narrative, a timeline with the relatives' events, sources and explanatory sheets.", link = "@/guide/persons.md" },
    { title = "Archives one click away", text = "A cited register opens at the cited view on the archives' own portal, for every French departmental archive.", link = "@/guide/sources-archives.md" },
    { title = "Photos and documents", text = "Portraits cropped from group photos, multi-page documents, tags and a media library.", link = "@/guide/media.md" },
    { title = "Search and kinship", text = "Find anyone with filters, and see every way two persons are related.", link = "@/guide/kinship.md" },
    { title = "Statistics and maps", text = "Completeness, a heat map of the places, names, occupations, lifespans and the records of your tree.", link = "@/guide/statistics.md" },
    { title = "Tools that keep the tree clean", text = "Anomalies, places to locate, duplicates to merge, and a converter between calendars.", link = "@/guide/tools.md" },
    { title = "Nothing is ever lost", text = "Every change is kept: compare the versions of a person and restore an earlier one.", link = "@/guide/history.md" },
    { title = "Places that need no typing", text = "A built-in dictionary of the places of twelve countries, with their former names, available offline.", link = "@/guide/editing.md" },
    { title = "In your language and colours", text = "Eight languages, light and dark themes, and themes of your own.", link = "@/guide/settings.md" },
    { title = "Ask an AI assistant", text = "Let Claude or another assistant read your tree from the desktop application, and change it only if you allow it.", link = "@/guide/assistant.md" },
]

values_title = "Your genealogy, in complete freedom"
values = [
    { title = "Free software.", text = "OxidGene is published under the GNU Affero General Public License: no licence to buy, no subscription, and its source code is open." },
    { title = "Your data stays with you.", text = "The desktop application keeps your trees on your computer and works offline. No account, no tracking: it goes online only when you open an archive's portal or import from Geneanet." },
    { title = "Open standards.", text = "GEDCOM and GEDZIP in and out, with export written for the program that will read it: nothing locks your research in." },
    { title = "On your desktop or your server.", text = "One application for Windows, macOS and Linux, or a web deployment for your family or your association." },
]

gallery_title = "A look at the application"
gallery = [
    { name = "pedigree", caption = "The pedigree and its events panel", link = "@/guide/pedigree.md" },
    { name = "fan-chart-dark", caption = "A fan chart in the dark theme", link = "@/guide/pedigree.md" },
    { name = "person", caption = "A person's page", link = "@/guide/persons.md" },
    { name = "couple", caption = "A couple's page", link = "@/guide/persons.md" },
    { name = "statistics-map", caption = "The places on a heat map", link = "@/guide/statistics.md" },
    { name = "kinship", caption = "Kinship between two cousins", link = "@/guide/kinship.md" },
    { name = "tools-anomalies", caption = "Anomalies of the tree", link = "@/guide/tools.md" },
    { name = "media-library", caption = "The media library", link = "@/guide/media.md" },
    { name = "history", caption = "A person's history, compared", link = "@/guide/history.md" },
]

download_title = "Download OxidGene"
downloads = [
    { title = "Windows", text = "Windows 10 and 11, x86-64: the executable in a .zip archive.", link = "https://github.com/trois-six/oxidgene/releases" },
    { title = "macOS", text = "macOS 11 or later, Apple silicon and Intel: the application in a disk image.", link = "https://github.com/trois-six/oxidgene/releases" },
    { title = "Linux", text = "x86-64: the executable in a .tar.gz archive, using the system's WebKitGTK.", link = "https://github.com/trois-six/oxidgene/releases" },
    { title = "Web server", text = "Docker Compose or a Helm chart, with PostgreSQL and S3-compatible storage.", link = "@/guide/install.md" },
]
download_note = "No release is published yet: until the first one, build the desktop application from its source as the installation guide explains. Every release lists SHA-256 checksums."
switching = "@/workflows/first-tree-from-gedcom.md"
switching_label = "Coming from another genealogy program?"

docs_title = "Documentation"
+++
