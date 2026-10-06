"""Writes the anonymized Bach fixtures beside this script: `python3 generate.py`.

They have the markup and JSON shapes of pages recorded from the Bach portals
(classification scheme, finding aids, a register's show page, the viewer's
image list, the Anubis check in front of one portal), with fictitious
localities, parishes, offices, finding-aid identifiers, call numbers, viewer
hosts, folders and image names; no recorded value is copied. Only what the
adapter reads is kept, with the markup around it."""
import html
import json
import pathlib

OUT = pathlib.Path(__file__).resolve().parent


def esc(text):
    return html.escape(text, quote=True).replace('&#x27;', '&#039;')


def page(title, body):
    return ('<!DOCTYPE html>\n<html lang="fr">\n<head>\n<meta charset="utf-8">\n'
            f'<title>Moteur de recherche des Archives d\'Exemple - {esc(title)}</title>\n'
            '<link rel="stylesheet" href="/assetic/css/compiled/bach.css">\n</head>\n<body>\n'
            '<nav><ul><li><a href="/archives/search"> Chercher </a></li>'
            '<li><a href="/archives/classification-scheme"> Liste des inventaires </a></li></ul></nav>\n'
            f'<article>\n{body}\n</article>\n'
            '<footer>&copy; 2026 Archives d\'Exemple | Développé par Anaphore</footer>\n</body>\n</html>\n')


# The classification scheme: one `li.standalone` per finding aid, a title and
# a link, under unlinked grouping nodes.
def entry(n, title, document, link, date=None):
    dated = f'<span class="date" property="dc:date"> • {esc(date)}</span>' if date else ''
    return (f'<li id="de-{n}" class="standalone"> <span class="cdc_unittitle" property="dc:title">'
            f'{title}</span>{dated} <a href="/document/{document}">{esc(link)}</a> </li>')


def group(n, title, items):
    return (f'<li id="tt1-{n}"> <input type="checkbox" id="item-tt1-{n}" checked><label for="item-tt1-{n}"> '
            f'<span class="cdc_unittitle" property="dc:title">{esc(title)}</span> </label><ul> '
            + ' '.join(items) + ' </ul> </li>')


def classification():
    see = "Consulter l'inventaire et les images numérisées"
    communes = group(1, "D'Ailleurs à Zed", [
        entry(1, 'Exampleville', 'FRAD099_RPEC_EXAMPLEVILLE', see, '1603-1952'),
        entry(2, "Le Mas-d'Exemple", 'FRAD099_RPEC_MAS_D_EXEMPLE', see, '1640-1952'),
        entry(3, 'Doubleville', 'FRAD099_RPEC_DOUBLEVILLE', see),
        entry(4, 'Doubleville', 'FRAD099_RPEC_DOUBLEVILLE_BIS', see),
    ])
    civil = group(2, 'État civil', [
        entry(5, 'Registres et actes des Églises réformées', 'FRAD099_Ec_Protestant', "Consulter l'inventaire"),
        entry(6, 'Registres paroissiaux et d&#039;état civil&nbsp;: Bourg-Exemple', 'FRAD099_Ec_Bourg_Exemple',
              "Consulter l'inventaire"),
        entry(7, 'Registres paroissiaux et d&#039;état civil&nbsp;: Saint-Exemple', 'FRAD099_Ec_Saint_Exemple',
              "Consulter l'inventaire"),
    ])
    indexes = group(3, 'Archives publiques', [
        entry(8, 'Série A :', 'FRAD099_IR_00359', 'Domaine royal'),
        entry(9, 'Registres paroissiaux et d&#039;état civil :', 'FRAD099_IR_00001', 'Albeville'),
        entry(10, 'Registres paroissiaux et d&#039;état civil :', 'FRAD099_IR_00002', 'Exampleville'),
        entry(11, ' <a href="/document/FRAD099_IR_00351" title="">Catalogue de la bibliothèque</a> ',
              'FRAD099_IR_00351', 'Catalogue de la bibliothèque'),
    ])
    body = ('<header id="cdcTitle"><h2>Liste des inventaires</h2></header>'
            '<div id="inventory_contents" class="cdc"> <h3 id="contents">Contenu</h3> '
            f'<div class="css-treeview"><ul> <ul> {communes} {civil} {indexes} </ul> </ul></div></div>')
    return page('Cadre de classement', body)


# A finding aid's tree: `a.display_doc` links whose depth is the nesting of
# the `ul` lists, registers being leaves (`de-<n>`).
def tree_node(document, node, title, date=None, unitid=None, level=2):
    dated = f'<span class="date" property="dc:date"> • {esc(date)}</span>' if date else ''
    called = f' - <span class="unitid" property="dc:identifier">{esc(unitid)}</span>' if unitid else ''
    return (f'<a class="display_doc" href="/archives/show/{document}_{node}"><span class="sizeTitle{level}">'
            f'<span property="dc:title" class="unit_title_lb">{esc(title)}</span>{dated}</span>{called}'
            '<span class="media_informations"></span></a>')


def render(document, items, depth=1):
    out = []
    for item in items:
        node, title = item[0], item[1]
        date, unitid = (item[2] if len(item) > 2 else None), (item[3] if len(item) > 3 else None)
        children = item[4] if len(item) > 4 else None
        link = tree_node(document, node, title, date, unitid, depth + 1)
        if children:
            out.append(f'<li id="{node}">\n<input type="checkbox" id="item-{node}" checked><label for="item-{node}">\n\n'
                       f'{link}\n\n</label><ul>\n{render(document, children, depth + 1)}</ul>\n</li>\n')
        else:
            out.append(f'<li id="{node}" class="standalone">\n<strong> • </strong>\n\n{link}\n\n</li>\n')
    return ''.join(out)


def aid(document, title, items):
    body = (f'<header id="cdcTitle"><h2>{esc(title)}</h2></header>\n'
            '<div id="inventory_contents"><h3 id="presentation">Présentation</h3>'
            '<div id="inventory_presentation"><dl><dt><h2>Intitulé</h2></dt>'
            f'<dd>{esc(title)}</dd></dl></div>\n<h3 id="contents">Contenu</h3>\n'
            f'<div class="css-treeview"><ul>{render(document, items)}</ul></div>\n</div>\n'
            '<script type="text/javascript">$(\'.css-treeview\').prepend(_p);</script>')
    return page(f'Voir le document {document}', body)


# One commune's aid (the Gard, Tarn and Vaucluse shape): parish registers by
# collection and confession, decennial tables by period then act, civil
# acts with Republican years, acts by kind, publications of banns.
COMMUNE = [
    ('tt1-1', 'Registres paroissiaux', None, None, [
        ('tt2-1', 'Collection communale', None, None, [
            ('tt3-1', 'Catholiques', None, None, [
                ('de-1', 'Paroisse Saint-Exemple', '1631-1721', 'GG 1'),
                ('de-2', 'Paroisses Saint-Exemple et Notre-Dame-d\'Exemple', '1699-1782', 'GG 2'),
                ('de-3', 'Paroisse Notre-Dame-d\'Exemple', '1631-1792', 'GG 3'),
            ]),
            ('tt3-2', 'Protestants', None, None, [
                ('de-4', '1769-1792', None, 'GG 5'),
            ]),
        ]),
        ('tt2-2', 'Collection du greffe : catholiques', None, None, [
            ('de-5', 'Paroisse Saint-Exemple', '1676-1701', '9 E 1 1'),
            ('de-6', 'Paroisse Notre-Dame-d\'Exemple', '1603-1701', '9 E 1 3'),
        ]),
    ]),
    ('tt1-2', 'État civil', None, None, [
        ('tt2-3', 'Tables décennales', None, None, [
            ('tt3-3', '1792-an XI', None, None, [
                ('de-9', 'Naissances', '1792-an XI', 'TD 1'),
                ('de-10', 'Mariages', '1792-an XI', 'TD 2'),
            ]),
            ('de-12', '1802-1812', None, 'TD 4'),
        ]),
        ('tt2-4', 'Actes', None, None, [
            ('de-13', '1793-an X', None, '9 E 2'),
            ('de-14', 'An XI-1812', 'an XI-1812', '9 E 3'),
        ]),
        ('tt2-5', "Actes d'état civil", None, None, [
            ('tt3-6', 'Naissances', None, None, [
                ('de-20', 'mars 1813-avril 1822', None, '9 E 10'),
            ]),
            ('tt3-7', 'Mariages (1813-1832)', None, None, [
                ('de-21', 'première partie', None, '9 E 11'),
                ('de-22', 'seconde partie', None, '9 E 12'),
            ]),
            ('tt3-8', 'Publications de mariage', None, None, [
                ('de-23', '1813-1822', None, '9 E 13'),
            ]),
        ]),
    ]),
]

# The aid of every commune (the Haute-Marne and Guadeloupe shape): letters,
# then communes written in capitals, whose registers name several acts.
DEPARTMENT = [
    ('tt1-1', 'A', None, None, [
        ('tt2-1', 'AVAL-EXEMPLE', None, None, [
            ('de-1', 'Baptêmes, mariages, sépultures', '1694-1774', '9 E 1/1'),
            ('de-2', 'Baptêmes, mariages, sépultures ; puis Naissances, mariages, décès', '1775-1802', '9 E 1/2'),
            ('de-3', 'Naissances, mariages, décès, tables décennales', '1802-1842', '9 E 1/3'),
            ('de-4', 'Naissances, mariages, décès', '1843-1862', '9 E 1/4'),
        ]),
        ('tt2-2', 'AMONT-EXEMPLE', None, None, [
            ('de-5', 'Baptêmes, mariages, sépultures', '1700-1792', '9 E 2/1'),
        ]),
    ]),
    ('tt1-2', 'M', None, None, [
        ('tt2-3', "MAS-D'EXEMPLE (LE)", None, None, [
            ('de-6', 'Baptêmes, sépultures', '1650-1700', '9 E 3/1'),
            ('de-7', 'Mariages', '1650-1700', '9 E 3/2'),
        ]),
    ]),
]

# An aid of decennial tables (the Haute-Marne shape): only its title says
# what its registers are.
TABLES = [
    ('tt1-1', 'A', None, None, [
        ('tt2-1', 'AVAL-EXEMPLE', None, None, [
            ('de-1', '1803-1812', None, '9 M 1/1'),
            ('de-2', '1813-1822', None, '9 M 1/2'),
        ]),
    ]),
]

# A military series (the Tarn-et-Garonne shape): conscription lists, then
# the registers of each recruitment bureau by class, volumes of matricules
# beside their alphabetical index.
MILITARY = [
    ('tt1-1', 'Recrutement', '1867-1940', None, [
        ('tt2-1', 'Listes du contingent', '1867-1871', None, [
            ('de-1', '1867', None, '9 R 1/1'),
            ('de-2', '1871', None, '9 R 1/2', [
                ('pa-1', 'Listes', '1871', '9 R 1 2'),
                ('pa-2', 'Répertoire alphabétique', '1871', '9 R 1 3'),
            ]),
        ]),
        ('tt2-2', 'Registres matricules', '1872-1940', None, [
            ('tt3-1', "Bureau de recrutement d'Exampleville", '1872-1873', None, [
                ('tt4-1', 'Classe 1872', None, None, [
                    ('de-6', 'Volume unique, n° 1-2787', '1872', '9 R 2/7'),
                    ('de-7', 'Répertoire alphabétique', '1872', '9 R 2/8'),
                ]),
                ('tt4-2', 'Classe 1873', None, None, [
                    ('de-8', 'N° 1-500.', '1873', '9 R 2/9'),
                    ('de-9', 'N° 501-1000.', '1873', '9 R 2/10'),
                ]),
            ]),
            ('tt3-2', "Bureau de recrutement de Saint-Exemple (arrondissements d'Ailleurs)", '1872', None, [
                ('tt4-3', 'Classe 1872', None, None, [
                    ('de-10', 'Volume unique, n° 1-1500', '1872', '9 R 3/1'),
                ]),
            ]),
        ]),
    ]),
]


def show(document, node, title, unitid, links):
    figure = ''
    if links is not None:
        anchors = ''.join(f'<div><a href="{esc(link)}" target="_blank"><img class="default_img" src="/img/img.png" '
                          'alt="Consulter l\'image" title="Consulter l\'image"></a></div>' for link in links)
        figure = ('<figure id="relative_documents"><header><h3>Documents relatifs</h3></header><div><section>'
                  f'{anchors}</section></div></figure>')
    called = (f'<section class="cote"><strong>Cotes extrêmes </strong><span class="unitid" '
              f'property="dc:identifier">{esc(unitid)}</span></section>') if unitid else ''
    body = (f'<header>\n<h2 property="dc:title">{esc(title)}'
            f'<a href="/document/{document}#{node}"class="treeLinkResults"\n title="Situer dans l\'inventaire">'
            'Situer dans l\'inventaire</a></h2>\n</header>\n'
            '<ul><li><a href="#relative_documents">Fichiers liés</a></li></ul>'
            f'<div id="{node}" class="content">\n{called}\n</div>{figure}')
    return page(f'Voir le document {document}_{node}', body)


def images(folder, names):
    return {
        "breadcrumb": {"unitid": "9 E 1/1", "cUnittitle": "Baptêmes", "cLegalstatus": None, "cAudience": None,
                       "link": "<a href=\"https://archives.example.org/document/FRAD099_00000001E\">Exemple</a>"},
        "count": len(names),
        "data": [{"name": name, "url": f"/viewer/show/full/{folder}/{name}?isMobOrTab=undefined",
                  "referenceStripThumbnailUrl": f"/viewer/show/thumb/{folder}/{name}", "type": "image",
                  "crossOriginPolicy": "Anonymous", "ajaxWithCredentials": True} for name in names],
        "pageCurrent": 0,
        "showThumb": True,
        "seriesHasImages": False,
    }


# The Anubis proof of work some portals answer in place of a page; the
# recorded one names the client's address, which this one does not.
CHALLENGE = ('<!doctype html><html lang="en"><head><title>Making sure you&#39;re not a bot!</title>'
             '<link rel="stylesheet" href="/.within.website/x/xess/xess.css?cachebuster=devel">'
             '<script id="anubis_challenge" type="application/json">{"rules":{"algorithm":"fast","difficulty":2},'
             '"challenge":{"id":"00000000-0000-0000-0000-000000000000","method":"fast","difficulty":2}}</script>'
             '</head><body><main><h1 id="title">Making sure you&#39;re not a bot!</h1></main>'
             '<script src="/.within.website/x/cmd/anubis/static/js/main.mjs" type="module"></script></body></html>\n')


def write(name, text):
    (OUT / name).write_text(text, encoding='utf-8')


def main():
    write('classification.html', classification())
    write('aid-commune.html', aid('FRAD099_RPEC_EXAMPLEVILLE', "Actes paroissiaux et d'état civil d'Exampleville", COMMUNE))
    write('aid-department.html', aid('FRAD099_00000001E', "Registres paroissiaux et d'état civil", DEPARTMENT))
    write('aid-tables.html', aid('FRAD099_00000164M', "Tables décennales de l'état civil", TABLES))
    write('aid-military.html', aid('FRAD099_IR_00197', 'Préparation et recrutement militaire', MILITARY))
    first, last = 'EX_001_00001_0001.jpg', 'EX_001_00001_0012.jpg'
    write('show-range.html', show(
        'FRAD099_RPEC_EXAMPLEVILLE', 'de-1', 'Paroisse Saint-Exemple', 'GG 1',
        [f'https://viewer.example.org/series/EXEMPLE/REGISTRES/EX_001_00001?s={first}&e={last}'
         '&levelDescription=FRAD099_RPEC_EXAMPLEVILLE_de-1']))
    write('show-folder.html', show(
        'FRAD099_00000001E', 'de-1', 'Baptêmes, mariages, sépultures', '9 E 1/1',
        ['https://archives.example.org/viewer/series/E/9E/EX_0001_001_01/']))
    write('show-none.html', show('FRAD099_RPEC_EXAMPLEVILLE', 'de-4', '1769-1792', 'GG 5', None))
    write('show-several.html', show(
        'FRAD099_RPEC_EXAMPLEVILLE', 'de-2', 'Paroisses', 'GG 2',
        ['https://viewer.example.org/series/EXEMPLE/REGISTRES/EX_002?levelDescription=FRAD099_RPEC_EXAMPLEVILLE_de-2',
         'https://viewer.example.org/series/EXEMPLE/REGISTRES/EX_003?levelDescription=FRAD099_RPEC_EXAMPLEVILLE_de-2']))
    names = [f'EX_0001_001_01_{n:04}.jpg' for n in range(1, 13)]
    write('images.json', json.dumps(images('E/9E/EX_0001_001_01', names), ensure_ascii=False, indent=1) + '\n')
    write('challenge.html', CHALLENGE)


if __name__ == '__main__':
    main()
