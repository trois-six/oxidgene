"""Writes the anonymized Ligeo fixtures beside this script:
`python3 generate.py`.

They have the markup and JSON shapes of answers recorded from the Ain,
Ardèche and Haute-Garonne portals, of the other Ligeo shapes recorded on
departmental portals since (notice lists, qualified and composite locality
cells, indexes of persons, a search within a finding aid), and a
military-register table built in the same markup, with fictitious
localities, parishes, sections, call numbers, internal references, ARK names and
image paths; no recorded value is copied. Only what the adapter reads is
kept, plus the neighbouring markup it must skip (action cells, notice
links, scripts). The manifest keeps neither the portal's server paths nor
its renderings."""
import html, json, pathlib

OUT = pathlib.Path(__file__).resolve().parent
NAAN = "99999"


def esc(text):
    return html.escape(text, quote=True).replace('&#x27;', '&#039;')


def ark_id(n):
    return f"vtaexample{n:04d}"


def viewer_link(n, count, label, group="daogrp/0", css="arc_img_visu"):
    plural = "vue" if count == 1 else "vues"
    title = esc(f"{count} {plural}  - {label} (ouvre la visionneuse)")
    return (f'<div class="arc_item_img"><a href="/ark:/{NAAN}/{ark_id(n)}/{group}/layout:table/'
            f'idsearch:RECH_0000000000000000000000000000{n:04d}" target="mywindow" title="{title}" '
            f'class="{css}"><img src="/archives/img/ico_visu.gif?1" alt="{title}" /></a></div>')


def actions(n, node):
    return ('<script>var title_add_to_binder = "Ajouter cette notice à votre classeur";</script>'
            '<div class="arc_vignette_sel"><ul class="actions actions-icon">'
            f'<li class="action"><a href="/ark:/{NAAN}/{ark_id(n)}" title="Lien vers la notice" '
            'class="btn arc_arklink" target="_blank"><span>Lien vers la notice</span></a></li>'
            f'<li class="action"><a href="/espace-perso/classeur/gestion_classeur/notice/{n}/{ark_id(n)}/n:{node}" '
            'title="Ajouter cette notice à votre classeur" class="btn gestion_classeur">'
            '<span>Ajouter</span></a></li></ul></div>')


def table(headers, rows, thead=True):
    head = ''.join(
        f'<th class="" scope="col"><a href="/archive/resultats/x/sort:{i}" title="ordonner">{esc(h)}</a></th>'
        if h else '<th class="" scope="col">Accès aux images</th>' for i, h in enumerate(headers))
    body = ''.join(f'<tr class="{"pair" if i % 2 else "impair"}">{"".join(f"<td>{c}</td>" for c in row)}</tr>'
                   for i, row in enumerate(rows))
    if thead:
        return (f'<table id="resultats"><caption>Présentation des notices archivistiques par ligne</caption>'
                f'<thead><tr class="entete">{head}</tr></thead><tbody>{body}</tbody></table>')
    return (f'<table id="resultats"><caption>Présentation des notices archivistiques par ligne</caption>'
            f'<tr class="entete">{head}</tr>{body}</table>')


def page(count, content=""):
    return ('<!DOCTYPE html><html><head><title>Archives d\'Exemple</title></head><body>'
            '<form id="arc_form_rech" method="get"></form>'
            '<div id="arc_liste_update" class="arc_liste_update">'
            '<script type="text/javascript">if ($("resultat_ordonnancement-div")) '
            '$("resultat_ordonnancement-div").style.display = "none";</script>'
            f'{count}{content}<div class="paginate paginate-bottom"></div></div>'
            '<p id="credit_ligeo_archives" style="display: none;">Réalisé avec Ligeo Archives</p>'
            '</body></html>\n')


def write(name, text):
    (OUT / name).write_text(text, encoding='utf-8')


# Ain: a locality column and an act column holding act letters; no call number.
def ain_row(n, locality, acts, dates, count, reference):
    label = f"{reference} - {locality} {dates}"
    mark = f'<mark class="arc_mark">{esc(locality)}</mark>'
    if locality.startswith("Exampleville-"):
        mark = f'<mark class="arc_mark">Exampleville</mark>{esc(locality[len("Exampleville"):])}'
    return [mark, esc(acts), dates, viewer_link(n, count, label), actions(n, 88)]


AIN_HEADERS = ["Commune", "Type d’acte", "Dates", "Vues", "Action(s)"]
ain_several = [
    ain_row(1, "Exampleville", "B, M, S", "1700", 77, "EX_EC LOT00001"),
    ain_row(2, "Exampleville", "B, M, S", "1700 - 1701", 1, "EX_EC LOT00002"),
    ain_row(3, "Exampleville", "T, B", "1700 - 1790", 395, "EX_EC LOT00003"),
    ain_row(4, "Exampleville", "T, S", "1700 - 1790", 294, "EX_EC LOT00004"),
    ain_row(5, "Exampleville-lès-Bois", "B, M, S", "1700", 40, "EX_EC LOT00005"),
]
write("ain-several.html", page(
    '<p class="nb_reponses"><span>5</span> réponses à votre recherche</p>',
    table(AIN_HEADERS, ain_several)))
write("ain-one.html", page(
    '<p class="nb_reponses"><span>1</span> réponse à votre recherche</p>',
    table(AIN_HEADERS, [ain_row(11, "Exampleville", "N", "1880", 120, "EX_EC LOT00011")])))
# The portal shows its help text, no marker and no table.
write("ain-none.html", page("", '<p class="help">Saisissez votre recherche.</p>'))
# A page of a longer answer: the count exceeds the rows shown.
write("ain-paginated.html", page(
    '<p class="nb_reponses"><span>59</span> réponses à votre recherche</p>',
    table(AIN_HEADERS, ain_several[:2])))
write("ain-tables.html", page(
    '<p class="nb_reponses"><span>1</span> réponse à votre recherche</p>',
    table(AIN_HEADERS, [ain_row(21, "Exampleville", "tables décennales", "1873 - 1882", 287, "1873 - 1882")])))

# Ardèche civil status: an act title, a thesaurus-qualified locality and a
# call number shared by registers of several localities.
ARDECHE_HEADERS = ["", "Intitulé [type(s) d'acte]", "Commune", "Dates", "Commentaires",
                   "Cote ou référence", "Action(s)"]


def ardeche_row(n, title, locality, dates, cote, count):
    place = f'<div class="items">{esc(locality)} (commune ; Exampledept, France)</div>'
    return [viewer_link(n, count, title), esc(title), place, dates, "", cote, actions(n, 96)]


ardeche_civil = [
    ardeche_row(31, "Naissances.", "Exampleville", "1880", "NC 99001", 103),
    ardeche_row(32, "Naissances.", "Exampleville", "1880", "NC 99002", 88),
    ardeche_row(33, "Mariages.", "Exampleville", "1880", "NC 99001", 46),
    ardeche_row(34, "Décès.", "Exampleville", "1880", "NC 99001", 113),
    ardeche_row(35, "Naissances.", "Exampleville-lès-Bois", "1880", "NC 99001", 30),
    ardeche_row(36, "Tables décennales des décès.", "Exampleville", "1873-1882", "NC 99010", 59),
    ardeche_row(37, "Tables décennales des mariages.", "Exampleville", "1873-1882", "NC 99010", 27),
    ardeche_row(38, "Tables décennales des naissances.", "Exampleville", "1873-1882", "NC 99010", 45),
]
write("ardeche-civil.html", page(
    '<span class="arc_nbr_reponses">8 réponses dans 2 inventaires</span>',
    table(ARDECHE_HEADERS, ardeche_civil)))

# Ardèche parish registers: no act column, the link's title names the
# register, and the register opens in a `daoloc` group.
PARISH_HEADERS = ["", "Commune", "Date", "Commentaire", "Action(s)"]


def parish_row(n, locality, dates, label, count):
    place = f'<div class="items"><span class="arc_surlignage">{esc(locality)}</span></div>'
    return [viewer_link(n, count, label, group="daoloc/0"), place, dates, "", actions(n, 164)]


write("ardeche-parish.html", page(
    '<span class="arc_nbr_reponses">1 réponse dans 1 inventaire</span>',
    table(PARISH_HEADERS, [parish_row(41, "Exampleville", "1695 à 1706", "BMS", 392)])))

# Haute-Garonne: one title column, from which everything is read.
HG_HEADERS = ["Intitulé", "Date", "Vue(s)", "Action(s)"]


def hg_row(n, title, dates, count):
    return [esc(title), dates, viewer_link(n, count, title, css="arc_img_visu_indexe"), actions(n, 97)]


hg = [
    hg_row(51, "Exampleville : Saint-Exemple, paroisse de Exampleville : baptêmes, mariages, sépultures, "
               "1756-1775. (collection du greffe)", "1756-1775", 323),
    hg_row(52, "Exampleville : Saint-Autre, paroisse urbaine de Exampleville : baptêmes, mariages, sépultures, "
               "1737-1746, 1747-1752*, 1753-1755, 1756-1765*, 1766-1790. (collection du greffe)",
           "1737-1790", 291),
    hg_row(53, "Exampleville, paroisse de Saint-Exemple. 1 GG 8, registre paroissial : baptêmes, mariages, "
               "sépultures. (collection communale)", "1751-1762", 150),
    hg_row(54, "Exampleville, paroisse de Saint-Exemple. 1 GG 12, registre paroissial : tables annuelles. "
               "(collection communale)", "1674-1802", 20),
    hg_row(55, "Exampleville, Section d'Exampleville et d'Exempleton. 4 E 1 registre d'état civil : "
               "tables décennales. (collection communale)", "1802-1863", 60),
    hg_row(56, "Exampleville-lès-Bois, paroisse de Saint-Test. 2 GG 1, registre paroissial : baptêmes, "
               "mariages, sépultures. (collection communale)", "1700-1750", 80),
]
write("hg-several.html", page(
    '<span class="arc_nbr_reponses">6 réponses dans 1 inventaire</span>',
    '<VTISREP>' + table(HG_HEADERS, hg, thead=False)))

# A military-register search: by recruitment bureau and class, one row per
# volume of matricules. No such search has been recorded yet: the table has
# the markup of the portals above and the columns such a search lists, and
# the link's title names the volume, with its range of matricules, as these
# portals' titles name a register.
MATRICULE_HEADERS = ["Bureau de recrutement", "Classe", "Cote", "Matricules", "Vues", "Action(s)"]


def matricule_row(n, bureau, year, cote, first, last, count):
    label = f"Registre matricule, classe {year}, n° {first} à {last}"
    return [esc(bureau), str(year), cote, f"{first} à {last}", viewer_link(n, count, label),
            actions(n, 77)]


write("matricules.html", page(
    '<p class="nb_reponses"><span>4</span> réponses à votre recherche</p>',
    table(MATRICULE_HEADERS, [
        matricule_row(61, "Exampleville", 1870, "1 R 901", 1, 500, 412),
        matricule_row(62, "Exampleville", 1870, "1 R 902", 501, 1000, 398),
        matricule_row(63, "Exampleville", 1871, "1 R 903", 1, 520, 420),
        matricule_row(64, "Exampleville-lès-Bois", 1870, "1 R 950", 1, 300, 250),
    ])))


# A list of notices instead of a table: each notice's heading shows its call
# number and dates as classed spans, its items carry their own labels, and
# its viewer link opens the linear layout. A notice without a viewer link is
# a register not digitised.
def notice(n, cote, dates, items, count, cls):
    heading = ('<div class="arc_notice_header"><div class="arc_notice_header_content"><div class="title">'
               f'<h3><span class="cote">{esc(cote)}</span> <span class="date"> • {esc(dates)}</span> </h3>'
               '</div></div></div>')
    image = ''
    if count:
        title = esc(f"{count} vues  - {cote} (ouvre la visionneuse)")
        image = (f'<div class="arc_vignette_img"><a href="/ark:/{NAAN}/{ark_id(n)}/daogrp/0/layout:linear/'
                 f'idsearch:RECH_internet_0000{n:04d}" target="mywindow" title="{title}" '
                 f'class="arc_img_visu_noicone"><img src="/img/vignette.jpg" alt="{title}" /></a>'
                 f'<p class="nb_vues">{count} vues </p></div>')
    labelled = ''.join(f'<div class="items"><strong class="arc_libelle_strong">{esc(label)} : </strong>'
                       f'{value}</div>' for label, value in items)
    return (f'<tr class="{cls} type-notice-archive "><td>{heading}{actions(n, 11)}'
            f'<div class="arc_notice_content"><div id="D_{n}" class="togglediv">{image}{labelled}'
            '<br class="pusher" /></div></div></td></tr>')


def place_item(name):
    return f'<mark class="arc_mark">{esc(name)}</mark> (Exampledept, France)'


notices = [
    notice(71, "9 E 71/2", "1829-1861", [
        ("Contexte", "Registres paroissiaux et d'état civil &gt; Exampleville"),
        ("Dates", "1829-1861"),
        ("Sujet", "naissance / mariage / décès"),
        ("Commune ou lieu-dit", place_item("Exampleville")),
    ], 269, "arc_impair"),
    notice(72, "9 Mi 72", "1841 1860", [
        ("Dates", "1841 1860"),
        ("Sujet", "Deces / Mariage / Naissance"),
        ("Commune ou lieu-dit", place_item("Exampleville")),
    ], 173, "arc_pair"),
    notice(73, "9 Mi 73", "1843 1852", [
        ("Dates", "1843 1852"),
        ("Sujet", "Table"),
        ("Commune ou lieu-dit", place_item("Exampleville")),
    ], 16, "arc_impair"),
    notice(74, "9 Mi 74", "1620 1746", [
        ("Dates", "1620 1746"),
        ("Sujet", "Bapteme / Sepulture / Mariage"),
        ("Paroisse", "Saint-Exemple (Exampleville, Exampledept, France : paroisse)"),
        ("Commune ou lieu-dit", place_item("Exampleville")),
    ], 348, "arc_pair"),
    # Listed, not digitised: no viewer link.
    notice(75, "9 E 75/1", "1850", [
        ("Dates", "1850"),
        ("Sujet", "naissance"),
        ("Commune ou lieu-dit", place_item("Exampleville")),
    ], 0, "arc_impair"),
]
write("notices.html", page(
    '<p role="status" class="nb_reponses"><span>5</span> réponses à votre recherche</p>',
    '<div style="overflow:auto;"><div id="linear_liste"><table role="presentation" id="resultats">'
    + ''.join(notices) + '</table></div></div>'))

# A table whose locality cells qualify their places: a hamlet within its
# commune, a parish after a commune, several places in one cell, a note in
# brackets; acts read from a document type and an act column; full dates;
# a register listed without a viewer link.
QUALIFIED_HEADERS = ["Commune", "Type de document", "Type d'acte", "Dates", "Cote", "Vues", "Action(s)"]


def qualified_row(n, place, kind, acts, dates, cote, count):
    link = viewer_link(n, count, cote) if count else ''
    return [esc(place), esc(kind), esc(acts), esc(dates), esc(cote), link, actions(n, 12)]


write("qualified.html", page(
    '<span class="arc_nbr_reponses">6 réponses dans 1 inventaire</span>',
    table(QUALIFIED_HEADERS, [
        qualified_row(81, "HAMEAU (EXAMPLEVILLE, Exampledept, lieu-dit)", "Acte",
                      "Baptême, Mariage, Sépulture", "13/11/1697 - 06/11/1707", "9 E 81 GG1", 28),
        qualified_row(82, "Exampleville / Saint-Exemple (paroisse)", "Acte",
                      "Baptême, Mariage, Sépulture", "1690-1710", "9 E 82 GG1", 41),
        qualified_row(83, "Ancienne (Exampledept, France) [aujourd'hui : Exampleville (Exampledept, France)] "
                          "Exampleville (Exampledept, France)", "Acte", "Naissance", "1850", "9 E 83/1", 30),
        qualified_row(84, "Exampleville (Exampledept, France)", "Table décennale", "Naissance",
                      "1843-1852", "9 E 84 TD", 25),
        qualified_row(85, "Exampleville (Exampledept, France)", "Acte", "Naissance", "1850", "9 E 85/1", 47),
        qualified_row(86, "Exampleville (Exampledept, France)", "Acte", "Décès", "1850", "9 E 86/1", 0),
    ])))

# An index of persons: one row per person, the register's call number and
# the person's matricule, a viewer link onto the person's own view.
INDEX_HEADERS = ["Cote", "Matricule", "Nom", "Bureau", "Classe", "Vues", "Action(s)"]


def index_row(n, cote, matricule, bureau, year):
    link = viewer_link(n, 1, f"{cote} - NOMEXEMPLE Prénom (matricule {matricule})", group="daoloc/0")
    return [esc(cote), str(matricule), "NOMEXEMPLE", esc(bureau), str(year), link, actions(n, 91)]


write("index.html", page(
    '<span class="arc_nbr_reponses">1 réponse dans 1 inventaire</span>',
    table(INDEX_HEADERS, [index_row(91, "9R0001", 984, "Exampleville", 1890)])))

# A search within a finding aid: the notices of every commune whose text
# names the searched one, each notice's path in the finding aid ending with
# its commune.
def fonds_notice(n, commune, cote, title, dates, count):
    link = (f'<a href="/ark:/{NAAN}/{ark_id(n)}/dao/0/idsearch:RECH_internet_0000{n:04d}" target="mywindow" '
            f'title="{count} vues - {esc(title)} (ouvre la visionneuse)" class="arc_img_visu">'
            '<img src="/archives/img/ico_visu.gif" alt="" /></a>')
    return (f'<li class="arc_notice " id="N_{n}"><div class="arc_titre_notice"><div class="arc_notice_header">'
            f'<div class="title"><h3><span class="cote">{esc(cote)} - </span> <span class="unittitle">'
            f'{esc(title)} - </span> <span class="date">{esc(dates)}</span> <div class="arc_item_img">{link}'
            '</div> </h3></div></div><div class="arc_notice_content"><div class="items">'
            '<div class="notice_filariane"><strong class="arc_libelle_strong">Contexte : </strong>'
            f'Registres paroissiaux et état civil numérisés &gt; {esc(commune)}</div></div></div></div></li>')


# The page of a finding aid searched within: its notices under
# `div#arc_fonds_notice`, no results container nor count.
write("fonds.html", '<!DOCTYPE html><html><head><title>Archives d\'Exemple</title></head><body>'
      '<form id="arc_form_rech" method="get"><input name="RECH_S" type="text" />'
      '<input type="hidden" name="RECH_eadid" value="FRAD000_1" /></form>'
      '<script>jQuery("#arc_liste_update").on("arch.search.updated", function() {});</script>'
      '<div id="arc_fonds_notice"><ul class="arc_res_bib_num">'
      + fonds_notice(101, "EXAMPLEVILLE", "9 NUM /1EC1", "Naissances, mariages, décès.", "1872-1876", 93)
      + fonds_notice(102, "EXAMPLEVILLE", "9 NUM /1EC3", "Décès.", "1893-1904", 56)
      + fonds_notice(103, "AUTREVILLE", "9 NUM /2EC1", "Naissances, mariages, décès. avant 1890 voir EXAMPLEVILLE",
                     "1872-1876", 61)
      + '</ul></div></body></html>\n')

# A city whose registers of one year and act are split by section: the
# locality cell lists the city, then the city and its section, one after
# another (`<br/>`); the call-number search answers one row, and a call
# number written otherwise none.
SECTION_HEADERS = ["Lieux", "Registre", "Actes", "Dates", "Cote", "Accès", "Action(s)"]


def section_row(n, section, cote, count):
    city = '<span class="arc_surlignage">Exampleville</span> (Exampledept, France)'
    place = f'<div class="items">{city}<br/>{city} -- Section {section}</div>'
    return [place, "Registre d'état civil", "Sépulture ou décès", "1893", esc(cote),
            viewer_link(n, count, cote), actions(n, 629)]


sections = [section_row(81 + k, k + 1, f"4 E 9999{k + 1}", count)
            for k, count in enumerate([310, 287, 293, 301])]
write("sections.html", page(
    '<p class="nb_reponses"><span>4</span> réponses dans 1 inventaire</p>',
    table(SECTION_HEADERS, sections)))
write("section-one.html", page(
    '<p class="nb_reponses"><span>1</span> réponse dans 1 inventaire</p>',
    table(SECTION_HEADERS, sections[2:3])))

# Titles whose locality ends at `.-`, and a heading that starts with the
# register's call number.
TITLE_HEADERS = ["Commune et type d'acte", "Date", "Vues", "Action(s)"]
write("titles.html", page(
    '<span class="arc_nbr_reponses">3 réponses dans 1 inventaire</span>',
    table(TITLE_HEADERS, [
        [esc("Exampleville.- Baptêmes, mariages, sépultures"), "1750", viewer_link(111, 120, "BMS"),
         actions(111, 92)],
        [esc("Exampleville.- Tables décennales des naissances, cote 9E99/2"), "1843 - 1852",
         viewer_link(112, 60, "TD"), actions(112, 92)],
        [esc("9 M 99 - Exampleville - 1901"), "1901", viewer_link(113, 80, "RP"), actions(113, 92)],
    ])))


# The register's IIIF Presentation 2 manifest: the canvases, image services
# and view permalinks. The service host is the portal's internal one; the
# adapter never uses it. The canvases' sizes are not the images': the live
# portals declare canvases of another size and proportions than the image
# their service serves, whose `info.json` gives the true size.
def canvas(register, index, width, height):
    name = f"EX_{register:04d}_{index:03d}"
    return {
        "@id": f"https://portal.example.invalid/ark:/{NAAN}/{ark_id(register)}/canvas/0/{index}",
        "@type": "sc:Canvas",
        "label": f"Image {index} sur 6",
        "width": width,
        "height": height,
        "ligeoPermalink": f"https://portal.example.invalid/ark:/{NAAN}/{ark_id(register)}/img:{name}",
        "images": [{
            "@type": "oa:Annotation",
            "motivation": "sc:painting",
            "resource": {
                "@type": "dctypes:Image",
                "service": {
                    "@context": "http://iiif.io/api/image/2/context.json",
                    "@id": f"https://portal.example.invalid/iiif/EC/EX_{register:04d}/{name}.jpg",
                    "profile": "http://iiif.io/api/image/2/level1.json",
                },
                "format": "image/jpeg",
                "width": width,
                "height": height,
            },
        }],
    }


def manifest(register, sizes):
    return {
        "@context": "http://iiif.io/api/presentation/2/context.json",
        "@type": "sc:Manifest",
        "@id": f"https://portal.example.invalid/ark:/{NAAN}/{ark_id(register)}/manifest",
        "label": "EX_EC LOT00011 -  Exampleville 1880 - 1880",
        "sequences": [{
            "@type": "sc:Sequence",
            "canvases": [canvas(register, i + 1, w, h) for i, (w, h) in enumerate(sizes)],
        }],
    }


SIZES = [(3576, 2268), (3576, 2268), (3576, 2268), (3576, 2268), (2268, 3576), (1000, 800)]
write("manifest.json", json.dumps(manifest(11, SIZES), indent=1, ensure_ascii=False) + "\n")


# An image service's `info.json`, as the portals serve it (as `text/html`):
# Image API 3, level 1, a few sizes, an id on the internal host.
def info(width, height):
    return {
        "@context": "http://iiif.io/api/image/3/context.json",
        "id": "https://portal.example.invalid/iiif/pool1/EX/FOND.TIF",
        "type": "ImageService3",
        "protocol": "http://iiif.io/api/image",
        "profile": "level1",
        "width": width,
        "height": height,
        "sizes": [{"width": width // 2 ** k, "height": height // 2 ** k} for k in (4, 3, 2, 1)],
        "tiles": [{"width": 256, "height": 256, "scaleFactors": [1, 2, 4, 8, 16]}],
    }


write("info-wide.json", json.dumps(info(2704, 1780), indent=1) + "\n")
write("info-tall.json", json.dumps(info(1780, 2704), indent=1) + "\n")
write("info-small.json", json.dumps(info(1000, 800), indent=1) + "\n")
