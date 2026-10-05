"""Writes the anonymized Ligeo fixtures beside this script:
`python3 generate.py`.

They have the markup and JSON shapes of answers recorded from the Ain,
Ardèche and Haute-Garonne portals, with fictitious localities, parishes,
call numbers, internal references, ARK names and image paths; no recorded
value is copied. Only what the adapter reads is kept, plus the neighbouring
markup it must skip (action cells, notice links, scripts). The manifest keeps
neither the portal's server paths nor its renderings."""
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
