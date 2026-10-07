"""Writes the anonymized Mnesys Expo fixtures beside this script:
`python3 generate.py`.

They have the markup and JSON shapes of answers recorded from three
portals, with fictitious localities, parishes, call numbers, ARK names and
image identifiers; no recorded value is copied. Only what the adapter reads
is kept, plus the footer markup that follows the results, which the row
scans must stop before."""
import html, json, pathlib

OUT = pathlib.Path(__file__).resolve().parent

PORTALS = {
    'ad37': dict(host='archives.example-37.test', naan='99937'),
    'ad14': dict(host='archives.example-14.test', naan='99914'),
    'ad51': dict(host='archives.example-51.test', naan='99951'),
    'ad19': dict(host='archives.example-19.test', naan='99919'),
    'ad25': dict(host='archives.example-25.test', naan='99925'),
    'ad27': dict(host='archives.example-27.test', naan='99927'),
    'ad58': dict(host='archives.example-58.test', naan='99958'),
    'ad59': dict(host='archives.example-59.test', naan='99959'),
    'ad68': dict(host='archives.example-68.test', naan='99968'),
    'ad69': dict(host='archives.example-69.test', naan='99969'),
    'ad90': dict(host='archives.example-90.test', naan='99990'),
}


def esc(text):
    return html.escape(text, quote=True).replace('&#x27;', '&#039;')


def image_id(serial):
    return f'00000000-0000-4000-8000-{serial:012d}'


def picture(portal, name, serial, title, images):
    """The row's link to its first image and its count: `images` is a
    count, the count's own text (`2 lots 892 medias`), `False` for a link
    without a count, or `None` for a register listed without images."""
    if images is None:
        return ''
    first = image_id(serial)
    count = images if isinstance(images, str) else f"{images} media{'s' if images != 1 else ''}"
    info = '' if images is False else f'''
                        <p class="info-list-picture">
                        {count}

        </p>
'''
    return f'''
            <div class="img image-thumbnail">
            <a href="/ark:/{portal['naan']}/{name}/{first}" class="bloc-list-picture d-block" title="Visualiser le media" rel="noopener noreferrer" target="_blank">
                <img class="list-picture img-fluid" src="/images/{first}_search_result_thumbnail.jpg" alt="{esc(title)}">
            </a>
{info}
        </div>
'''


def row(portal, number, name, serial, title, period, images, collection, context, call_number=None):
    """One `li.element-list`; `context` follows the collection entry."""
    cote = ''
    if call_number:
        cote = ('<div  class="content-sub-part">\n                <h3>Cote</h3>\n'
                f'                <p class="referenceCodes">{esc(call_number)}</p>\n            </div>')
    entries = ''.join(f'<li>\n                    {esc(entry)}        </li>\n            '
                      for entry in context)
    date = ''
    if period:
        date = f'''<div class="content-sub-part">
                <h3>Date</h3>
                <p><span>{esc(period)}</span></p>
            </div>'''
    return f'''        <li class="element-list">

<div class="img-element">
    <span><span class="sr-only">Résultat n°</span>{number}</span>
{picture(portal, name, serial, title, images)}
</div>

<section class="content">
        <div class="intitup">
                <a
            href="https://{portal['host']}/ark:/{portal['naan']}/{name}"
            title="Voir la notice complète : {esc(title)}"
             rel="noopener noreferrer" target="_blank"
        >
            <h2>
                <span>{esc(title)}</span>
            </h2>
        </a>

    <div class="date-cote content-part clearfix">
                    {date}
                            {cote}
            </div>

<ul class="context content-part clearfix">
            <li>
                    <div class="context-content">Contexte : {esc(collection)}</div>
                </li>
            {entries}<li>                {esc(title)}
            </li>
</ul>


    </div>
    </section>
                        <div class="record-actions">
    </div>

        </li>
'''


FOOTER = '''            <ul class="icon-list">
                    <li class="link
icon-only">
        <a href="https://social.example.test/archives" title="Social" rel="noopener noreferrer" target="_blank">
                    <i class="nf nf-facebook-square nf-2x" aria-hidden="true"></i>
                    </a>
    </li>
            </ul>
'''


def page(total, rows):
    count = f'{total} résultat{"s" if total != 1 else ""}'
    return f'''<!DOCTYPE html>
<html lang="fr">
<head><meta charset="utf-8"><title>Résultats de la recherche</title></head>
<body class="search-results">
    <main>
        <section class="search-header">
            <p class="count"><span class="result">
                                                                {count}

                                    </span></p>
        </section>
            <div class="container">
                <h1 hidden="true">Résultats de la recherche</h1>
<section id="list-container" class="active">
            <ol>{''.join(rows)}                    </ol>

            </section>
            </div>
    </main>
    <footer>
{FOOTER}    </footer>
</body>
</html>
'''


NONE = '''<!DOCTYPE html>
<html lang="fr">
<head><meta charset="utf-8"><title>Résultats de la recherche</title></head>
<body class="no-results">
    <main>
            <div class="container">
                <h1 hidden="true">Résultats de la recherche</h1>
<section id="list-container" class="active">
            <div class="no-result">
    <p>Aucun résultat ne correspond à votre requête</p>
</div>

    </section>
            </div>
    </main>
</body>
</html>
'''


def window(portal, name, entries, as_list):
    """The viewer endpoint's answer: an array for a window starting at the
    first image, an object keyed by the zero-based index otherwise."""
    items = {}
    for index, serial in entries:
        uuid = image_id(serial)
        items[str(index)] = {
            'url': f"https://{portal['host']}/ark:/{portal['naan']}/{name}/{uuid}",
            'record': {'arkId': {'arkName': name, 'naan': int(portal['naan'])},
                       'title': ['Register'], 'referenceCode': [],
                       'locationKeywords': ['Exampleville (Exampleshire, France)']},
            'uuid': uuid,
            'location': {'original': f"https://{portal['host']}/images/{uuid}.jpg",
                         'thumb': f"https://{portal['host']}/images/{uuid}_thumbnail.jpg",
                         'iiif': None},
            'type': 'image', 'format': 'jpg', 'title': None,
        }
    return list(items.values()) if as_list else items


def manifest(portal, name, serials, sizes):
    items = []
    for index, (serial, (width, height)) in enumerate(zip(serials, sizes)):
        uuid = image_id(serial)
        base = f"https://{portal['host']}/iiif/ark:/{portal['naan']}/{name}/{uuid}"
        items.append({
            'id': f'{base}/canvas/{index}', 'type': 'Canvas',
            'label': {'fr': [f'Image n°{index + 1}']}, 'height': height, 'width': width,
            'thumbnail': [{'id': f"https://{portal['host']}/images/{uuid}_thumbnail.jpg", 'type': 'Image'}],
            'items': [{'id': f'{base}/canvas/{index}/annotationPage/0', 'type': 'AnnotationPage',
                       'items': [{'type': 'Annotation', 'motivation': 'painting',
                                  'body': {'id': f'{base}/full/max/0/default.jpg', 'type': 'Image',
                                           'height': height, 'width': width,
                                           'service': [{'id': base, 'type': 'ImageService1',
                                                        'profile': 'https://iiif.io/api/image/3/level0.json'}]}}]}],
        })
    return {'@context': ['http://www.w3.org/ns/anno.jsonld', 'http://iiif.io/api/presentation/3/context.json'],
            'id': f"https://{portal['host']}/iiif/ark:/{portal['naan']}/{name}/group/0/manifest.json",
            'type': 'Manifest', 'items': items}


def info(portal, name, serial, width, height):
    uuid = image_id(serial)
    return {'@context': 'http://iiif.io/api/image/3/context.json',
            'id': f"https://{portal['host']}/iiif/ark:/{portal['naan']}/{name}/{uuid}",
            'type': 'ImageService3', 'protocol': 'http://iiif.io/api/image', 'profile': 'level0',
            'width': width, 'height': height, 'sizes': [{'width': width, 'height': height}],
            'tiles': [{'width': width, 'height': height, 'scaleFactors': [1]}], 'qualities': ['default']}


def write(name, content):
    text = content if isinstance(content, str) else json.dumps(content, ensure_ascii=False, indent=1) + '\n'
    (OUT / name).write_text(text, encoding='utf-8')


def main():
    p37, p14, p51 = PORTALS['ad37'], PORTALS['ad14'], PORTALS['ad51']
    civil = "Registres d'état civil numérisés"
    parish = 'Registres paroissiaux numérisés'
    tables = "Tables décennales de l'état civil numérisées"

    # AD37: one register for the citation (Exampleville, births 1850).
    write('ad37-one.html', page(1, [row(
        p37, 1, 'aaaaaaaaaaaa', 100, 'Naissances, 1850-1860', '1850-1860', 315, civil,
        ['Exampleville', 'Exampleville'], '6NUM8/999/050 (Cote)')]))
    # AD37: the births' decennial table is returned beside the registers.
    write('ad37-tables.html', page(3, [
        row(p37, 1, 'bbbbbbbbbbbb', 200, 'Naissances', '1850-1860', 30, tables,
            ['Exampleville', 'Exampleville', '1850 - 1860', 'Naissances'], '6NUM2/999/001 (Cote)'),
        row(p37, 2, 'aaaaaaaaaaaa', 100, 'Naissances, 1850-1860', '1850-1860', 315, civil,
            ['Exampleville', 'Exampleville'], '6NUM8/999/050 (Cote)'),
        row(p37, 3, 'cccccccccccc', 300, 'Naissances, mariages, décès, divorces', '1850-1860', 19, tables,
            ['Exampleville', 'Saint-Exemple-Hors', '1850 - 1860', 'Naissances, mariages, décès, divorces'],
            '6NUM2/999/002 (Cote)')]))
    # AD37: parish registers of two parishes, one a merged commune.
    write('ad37-several.html', page(3, [
        row(p37, 1, 'dddddddddddd', 400, 'Collection communale. Baptêmes, mariages, sépultures, 1683-1700',
            '1683-1700', 153, parish,
            ['Exampleville', 'Exampleville', 'Saint-Exemple', 'Collection communale. Baptêmes, mariages, sépultures,…'],
            '6NUM7/999/007 (Cote)'),
        row(p37, 2, 'eeeeeeeeeeee', 500, 'Collection communale. Baptêmes, mariages, sépultures, 1690-1710',
            '1690-1710', 140, parish,
            ['Exampleville', 'Saint-Exemple-Hors', 'Saint-Autre', 'Collection communale. Baptêmes, mariages, sépultures,…'],
            '6NUM7/999/008 (Cote)'),
        row(p37, 3, 'ffffffffffff', 600, 'Collection du greffe. Mariages, 1700', '1700', 52, parish,
            ['Exampleville', 'Exampleville', 'Saint-Exemple', 'Collection du greffe. Mariages, 1700'],
            '6NUM6/999/005 (Cote)')]))
    write('ad37-none.html', NONE)
    write('ad37-visualizer-first.json', window(p37, 'aaaaaaaaaaaa', [(0, 100)], True))
    write('ad37-visualizer-window.json', window(p37, 'aaaaaaaaaaaa', [(2, 202), (149, 249), (150, 250)], False))
    write('ad37-info.json', info(p37, 'aaaaaaaaaaaa', 249, 3600, 2614))
    write('ad37-manifest.json', manifest(p37, 'aaaaaaaaaaaa', [100, 101, 102], [(3600, 2614), (3566, 2579), (2900, 2189)]))

    # AD14: no call number; the act is in the context, the title is the period.
    write('ad14-results.html', page(3, [
        row(p14, 1, 'hhhhhhhhhhhh', 700, '1843-1852', '1843-1852', 42, 'Etat civil (communes de A à D)',
            ['Exampleville', 'Etat civil', 'Tables décennales des Naissances']),
        row(p14, 2, 'iiiiiiiiiiii', 800, '1849-1851', '1849-1851', 518, 'Etat civil (communes de A à D)',
            ['Exampleville', 'Etat civil', 'Naissances, Mariages, Décès']),
        row(p14, 3, 'jjjjjjjjjjjj', 900, '1609-1624, 1643-1706', '1609-1706', 681, 'Etat civil (communes de A à D)',
            ['Exampleville', 'Paroisses, institutions, protestants', 'Paroisse Saint-Exemple',
             'Baptêmes, Mariages, Sépultures'])]))
    write('ad14-visualizer.json', window(p14, 'iiiiiiiiiiii', [(20, 820), (21, 821)], False))

    # AD51: article suffix, former communes, combined registers.
    write('ad51-results.html', page(2, [
        row(p51, 1, 'kkkkkkkkkkkk', 1100, 'Bourg (Le). Baptêmes, mariages, sépultures 1718-1750', '1718-1750',
            144, 'Etat civil', ['Bourg (Le)', 'Registres paroissiaux',
                                'Bourg (Le). Baptêmes, mariages, sépultures 1718-1750'], 'E dépôt 999'),
        row(p51, 2, 'llllllllllll', 1200, 'Bourg (Le). Baptêmes, mariages, sépultures 1728-1792', '1728-1792',
            337, 'Etat civil', ['Bourg (Le)', 'Registres paroissiaux',
                                'Bourg (Le). Baptêmes, mariages, sépultures 1728-1792'], '2 E 999/1')]))
    write('ad51-one.html', page(1, [row(
        p51, 1, 'mmmmmmmmmmmm', 1300, 'Exampleville. Naissances 1850', '1850', 60, 'Etat civil',
        ['Exampleville', "Registres d'état civil", 'Exampleville. Naissances 1850'], '2 E 999/71')]))
    write('ad51-visualizer.json', window(p51, 'mmmmmmmmmmmm', [(1, 1301)], False))
    series()


def form(uuid, selects, inputs):
    """A search form: one `enhanced-select` per select, by name and labels,
    then plain inputs, by name."""
    parts = [f'<input type="hidden" value="{uuid}" name="formUuid"/>',
             '<input type="hidden" value="date_asc" name="sort"/>',
             '<input type="hidden" value="list" name="mode"/>']
    for name, labels in selects.items():
        options = esc(json.dumps(labels))
        parts.append(f'<div class="enhanced-select multiselect" data-input-id="{name}" data-name="{name}" '
                     f'data-options="{options}" data-selected-options="null" '
                     'data-placeholder="Sélectionnez un ou plusieurs éléments" data-submit-as-json="false" ></div>')
    for name in inputs:
        parts.append(f'<input type="text" class="form-control" id="{name}" name="{name}" value=""/>')
    body = '\n            '.join(parts)
    return f'''<!DOCTYPE html>
<html lang="fr">
<head><meta charset="utf-8"><title>Recherche</title></head>
<body>
    <main>
        <form method="get" action="/search/results">
            {body}
        </form>
    </main>
</body>
</html>
'''


def viewer_state(portal, name, serial, images):
    """The viewer's state for a register's first image: the images of its
    lot, the lots, and the first image."""
    uuid = image_id(serial)
    return {'counts': {'media': images, 'group': 2}, 'positions': {'media': 0, 'group': 0},
            'group': {'title': None},
            'media': [{'url': f"https://{portal['host']}/ark:/{portal['naan']}/{name}/{uuid}", 'uuid': uuid,
                       'location': {'original': f"https://{portal['host']}/images/{uuid}.jpg",
                                    'thumb': f"https://{portal['host']}/images/{uuid}_thumbnail.jpg", 'iiif': None},
                       'type': 'image', 'format': 'jpg'}],
            'tableOfContents': None, 'bookmarks': []}


def series():
    """Forms of other shapes and series rows: military registers by bureau
    and matricule range, rows coding their acts as letters, upper-case
    labels, lots, rows without images, call numbers in the context."""
    p19, p25, p58, p59 = PORTALS['ad19'], PORTALS['ad25'], PORTALS['ad58'], PORTALS['ad59']
    p68, p69, p90 = PORTALS['ad68'], PORTALS['ad69'], PORTALS['ad90']
    geo = 'controlledAccessGeographicName'
    phys = 'controlledAccessPhysicalCharacteristic'

    # AD19: military registers of one bureau and class, one volume per
    # matricule range, the bureau named in the context.
    write('ad19-military-form.html', form('85c4d2cc-6374-489a-8be0-e79d0e0755b6', {
        f'0-{geo}': ['Exampleville (Corrèze, France)', 'Sampleton (Corrèze, France)'],
        '1-date': ['1889', '1890'],
        f'3-{phys}': ['registre matricule', 'table'],
    }, []))
    bureau = ["Recrutement militaire de la circonscription d'Exampleville", 'Registre matricule']
    write('ad19-military.html', page(4, [
        row(p19, index + 1, name, serial, f'Classe 1890 : matricules {first} à {last}.', '1890', images,
            'Recrutement militaire', bureau, f'R/{cote}')
        for index, (name, serial, first, last, images, cote) in enumerate([
            ('aaaaaaaaaa19', 1900, 500, 1000, 557, 9992),
            ('bbbbbbbbbb19', 2000, 1, 499, 567, 9991),
            ('cccccccccc19', 2100, 1001, 1500, 580, 9993),
            ('dddddddddd19', 2200, 1501, 1631, 154, 9994)])]))
    write('ad19-visualizer.json', window(p19, 'aaaaaaaaaa19', [(99, 1999)], False))

    # AD25: registers coding their acts as letters, no call number.
    write('ad25-registers.html', page(4, [
        row(p25, 1, 'aaaaaaaaaa25', 2500, 'BMS 1739-1750', '1739-1750', 120, 'Registres paroissiaux et état civil',
            ['Communes E', 'Exampleville']),
        row(p25, 2, 'bbbbbbbbbb25', 2600, 'M 1793-1815', '1793-1815', 80, 'Registres paroissiaux et état civil',
            ['Communes E', 'Exampleville']),
        row(p25, 3, 'cccccccccc25', 2700, 'N 1793-1815', '1793-1815', 140, 'Registres paroissiaux et état civil',
            ['Communes E', 'Exampleville']),
        row(p25, 4, 'dddddddddd25', 2800, 'BMS-NMD 1751-1792', '1751-1792', 300,
            'Registres paroissiaux et état civil', ['Communes E', 'Exampleville'])]))
    write('ad25-visualizer.json', window(p25, 'bbbbbbbbbb25', [(4, 2604)], False))

    # AD58: military registers of every bureau, which only the title names.
    write('ad58-military.html', page(4, [
        row(p58, 1, 'aaaaaaaaaa58', 5800, "Bureau d'Exampleville, classe 1890 : répertoire", '1890', 25,
            "Service historique de l'armée : registres militaires",
            ['Répertoires'], 'R 992'),
        row(p58, 2, 'bbbbbbbbbb58', 5900, "Bureau d'Exampleville, classe 1890 : fiches matricules n° 1 à 500",
            '1890', 780, "Service historique de l'armée : registres militaires",
            ['Fiches matricules'],'R 983'),
        row(p58, 3, 'cccccccccc58', 6000, "Bureau d'Exampleville, classe 1890 : fiches matricules n° 501 à 1000",
            '1890', 829, "Service historique de l'armée : registres militaires",
            ['Fiches matricules'],'R 984'),
        row(p58, 4, 'dddddddddd58', 6100, 'Bureau de Sampleton, classe 1890 : fiches matricules n° 1 à 498',
            '1890', 666, "Service historique de l'armée : registres militaires",
            ['Fiches matricules'], 'R 988')]))
    write('ad58-visualizer.json', window(p58, 'cccccccccc58', [(9, 6009)], False))

    # AD59: upper-case labels without accents, which a lookup finds; the
    # acts are letters in the title, which the context repeats alone.
    write('ad59-form.html', form('dc4e871d-0b62-41fb-9921-5ded573781b8', {
        f'0-{geo}': ['EXAMPLEVILLE', 'SAINT-EXEMPLE', 'SAINT-EXEMPLE-LES-BOIS', 'SAMPLETON - Section A, B'],
        f'1-{phys}': ['Baptêmes', 'Naissances', 'Mariages', 'Sépultures', 'Décès'],
    }, ['2-date', '3-date_begin', '3-date_end']))
    collection = "Registres numérisés des registres d'état civil du Nord"
    write('ad59-registers.html', page(3, [
        row(p59, 1, 'aaaaaaaaaa59', 5900, 'EXAMPLEVILLE / BMS [1737-1792]', '1737-1792', 381, collection,
            [], '9 Mi 999 R 001'),
        row(p59, 2, 'bbbbbbbbbb59', 6000, 'EXAMPLEVILLE / NMD, Td (sauf 1853-1862) [1823-1882]', '1823-1882', 909,
            collection, [], '9 Mi 999 R 002'),
        row(p59, 3, 'cccccccccc59', 6100, 'EXAMPLEVILLE / NMD [1838-1862]', '1838-1862', 347, collection,
            [], '9 Mi 999 R 003')]))
    write('ad59-visualizer.json', window(p59, 'aaaaaaaaaa59', [(2, 5902)], False))

    # AD68: the call number of a civil-status row is a context entry.
    write('ad68-registers.html', page(2, [
        row(p68, 1, 'aaaaaaaaaa68', 6800, '1793-1862', '1793-1862', 530, 'Naissances',
            ['Exampleville', '9Mi9/9']),
        row(p68, 2, 'bbbbbbbbbb68', 6900, 'Exampleville - Paroisse catholique - Registres de baptêmes',
            '1788-1792', 62, 'Registres paroissiaux',
            [], '9E/9/1')]))

    # AD69: volumes in two lots (images, then a document) and two call
    # numbers per cell; the bureau is in the title only.
    write('ad69-military.html', page(3, [
        row(p69, index + 1, name, serial, f'Exampleville Central : n° matricules {first}-{last}', '1900',
            f'2 lots\n                        {images} medias',
            '1Rp - Recrutement militaire : répertoires et registres',
            ['Armée active', 'Registres matricules'],
            f'{internal}, 9RP{cote}')
        for index, (name, serial, first, last, images, internal, cote) in enumerate([
            ('aaaaaaaaaa69', 6900, 1, 489, 700, 443, 9991),
            ('bbbbbbbbbb69', 7000, 490, 986, 892, 444, 9992),
            ('cccccccccc69', 7100, 987, 1488, 820, 445, 9993)])]))
    write('ad69-viewer-state.json', viewer_state(p69, 'bbbbbbbbbb69', 7000, 891))
    write('ad69-visualizer.json', window(p69, 'bbbbbbbbbb69', [(445, 7445)], False))

    # AD90: tables listed without images (`Manque`) beside digitised ones.
    office = ['Bureau de Exampleville', 'Tables de successions et absences.']
    context = "3 Q - Enregistrement (bureau de Exampleville)"
    write('ad90-succession.html', page(3, [
        row(p90, 1, 'aaaaaaaaaa90', 9000, 'Manque', None, None, context, office, '3 Q 99/3'),
        row(p90, 2, 'bbbbbbbbbb90', 9100, '1873 - 1880', '1873-1880', 191, context, office,
            '3 Q 99/20'),
        row(p90, 3, 'cccccccccc90', 9200, '1880 - 1884', '1880-1884', 173, context, office,
            '3 Q 99/21')]))
    write('ad90-visualizer.json', window(p90, 'bbbbbbbbbb90', [(9, 9109)], False))

    # AD14: a census row with an image but no count.
    write('ad14-census.html', page(1, [
        row(PORTALS['ad14'], 1, 'kkkkkkkkkk14', 1400, '1876', '1876', False, 'Recensements de population',
            ['Exampleville'])]))
    write('ad14-viewer-state.json', viewer_state(PORTALS['ad14'], 'kkkkkkkkkk14', 1400, 40))

    # AD27: former communes, whose context entry is their label followed by
    # the current commune, or their label cut short.
    p27 = PORTALS['ad27']
    collection = 'Registres paroissiaux (1529-1792 environ)'
    write('ad27-former.html', page(2, [
        row(p27, 1, 'aaaaaaaaaa27', 2700, 'BMS (1646-1792)', '1646-1792', 585, collection,
            ['Exampleville (ancienne commune) (Eure, France)/Sampleton...', 'Registres paroissiaux',
             'BMS (1646-1792)'], '9 Mi 9999 (Cote/Cotes extrêmes)'),
        row(p27, 2, 'bbbbbbbbbb27', 3300, 'BMS (1600-1700)', '1600-1700', 300, collection,
            ['Saint-Exemple-la-Longue (ancienne commune) (Eure,...', 'Registres paroissiaux',
             'BMS (1600-1700)'], '9 Mi 9998 (Cote/Cotes extrêmes)')]))
    write('ad27-visualizer.json', window(p27, 'aaaaaaaaaa27', [(219, 2919)], False))


main()
