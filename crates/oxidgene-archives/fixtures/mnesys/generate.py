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
}


def esc(text):
    return html.escape(text, quote=True).replace('&#x27;', '&#039;')


def image_id(serial):
    return f'00000000-0000-4000-8000-{serial:012d}'


def row(portal, number, name, serial, title, period, images, collection, context, call_number=None):
    """One `li.element-list`; `context` follows the collection entry."""
    first = image_id(serial)
    cote = ''
    if call_number:
        cote = ('<div  class="content-sub-part">\n                <h3>Cote</h3>\n'
                f'                <p class="referenceCodes">{esc(call_number)}</p>\n            </div>')
    entries = ''.join(f'<li>\n                    {esc(entry)}        </li>\n            '
                      for entry in context)
    return f'''        <li class="element-list">

<div class="img-element">
    <span><span class="sr-only">Résultat n°</span>{number}</span>

            <div class="img image-thumbnail">
            <a href="/ark:/{portal['naan']}/{name}/{first}" class="bloc-list-picture d-block" title="Visualiser le media" rel="noopener noreferrer" target="_blank">
                <img class="list-picture img-fluid" src="/images/{first}_search_result_thumbnail.jpg" alt="{esc(title)}">
            </a>

                        <p class="info-list-picture">
                        {images} media{'s' if images != 1 else ''}

        </p>

        </div>

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
                    <div class="content-sub-part">
                <h3>Date</h3>
                <p><span>{esc(period)}</span></p>
            </div>
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


main()
