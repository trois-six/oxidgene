"""Writes the anonymized Arkothèque fixtures beside this script:
`python3 generate.py`.

They have the markup and JSON shapes of answers recorded from the portals,
with fictitious localities, parishes, call numbers, records, files and ARK
names; no recorded value is copied. Engine, filter and field references are
the portals' public ones, as in the catalogue. Only what the adapter reads is
kept."""
import html, json, pathlib

OUT = pathlib.Path(__file__).resolve().parent
AD44 = dict(engine='arko_default_6a6b4ac0309a5', image_field='arko_default_6a6b4d70c2bc6',
            locality_field='arko_default_6a3b876245227', period_field='arko_default_6a3b7db3a0215')
AD72 = dict(engine='arko_default_6319dbb9a1c43', image_field='arko_default_6319dbb1469ae',
            locality_field='arko_default_63075c72e9577')
AD72_LATE = dict(AD72, engine='arko_default_678f538cb2d58')

def esc(text):
    return html.escape(text, quote=True).replace('&#x27;', '&#039;')

def span(champ, kind, text):
    return f'<span style="pointer-events: none;" data-champ="{champ}" data-type-champ="{kind}">{esc(text)}</span>'

def linked(rebond, term, champ, kind, text):
    return (f'<button data-rebond="{rebond}" data-term="{esc(term)}">\n'
            f'            {span(champ, kind, text)}\n        </button>')

def td(content, cls='pas_de_rebond'):
    return f'<td class="{cls}">\n            {content}\n        </td>'

def empty(champ, kind='varchar'):
    return f'<!-- Valeur vide pour le champ: {champ} type: {kind} -->'

def images(portal, record, file_id, count):
    data = json.dumps({"refUniqueFiche": record, "refUniqueField": portal['image_field'],
                       "refUniqueMoteur": portal['engine'], "position": None,
                       "idArkoFile": file_id, "mediaType": "image"}, separators=(',', ':'))
    url = f"/_recherche-api/visionneuse-infos/{portal['engine']}/{record}/{portal['image_field']}/image/{file_id}"
    return ('<div class="container_zone_images ">\n'
            '    <span class="oeil_et_nombre">\n'
            '        <button class="bouton_rond visualiser icone-images rec-visionneuse-icone rec-visionneuse-media-image" '
            f'aria-label="Visualiser les images" title="Visualiser les images" data-nb="1" data-visionneuse="{esc(data)}" '
            f'data-visionneuse-url="{url}">\n            <svg class="svg_oeil"></svg>\n        </button>\n'
            '        <span class="coche"><span></span></span>\n'
            f'        <span class="nombre_images">({count} images)</span>\n'
            '    </span>\n</div>')

def row(cells):
    return ('<tr class="resultat_container  visibilite-visible-front">\n        '
            + '\n        '.join(cells) + '\n    </tr>')

def table(headers, rows):
    head = ''.join(f'<th>{esc(h)}</th>' for h in headers)
    return ('\n<table class="tableau_resultat_facettes">\n    <caption>Tableau de résultats</caption>\n'
            f'    <thead>\n        <tr>{head}</tr>\n    </thead>\n    <tbody>\n    '
            + '\n    '.join(rows) + '\n    </tbody>\n</table>\n')

def answer(portal, headers, registers, render, total=None):
    rows = [row(render(portal, register)) for register in registers]
    results = [{"id": 100000 + index, "intitule": register.get('cote', ''),
                "refUnique": register['record']} for index, register in enumerate(registers)]
    return {"refUnique": portal['engine'],
            "resultats": {"total": len(registers) if total is None else total,
                          "count": len(registers), "results": results,
                          "html": table(headers, rows), "aggregations": [], "modeRestit": "tableau"}}

AD44_HEADERS = ["Commune", "Ancienne commune", "Paroisse (institution)", "Type d'actes", "Période", "Commentaire", "Image"]

def ad44_row(portal, r):
    key = f"{r['commune']}[[arko_fiche_0000000000c{r['record'][-3:]}]]"
    return [td(linked(portal['locality_field'], key, 'liaison_commune', 'relation_fiche', r['commune'])),
            td(linked('arko_default_6a4ce6b2e8668', r.get('ancienne', r['commune']), 'ancienne_commune', 'varchar', r.get('ancienne', r['commune']))),
            td(span('paroisse', 'varchar', r['paroisse']) if 'paroisse' in r else empty('paroisse')),
            td(span('type_actes_affichage', 'varchar', r['actes']), ''),
            td(linked(portal['period_field'], f"{r['date']}[[0000000000000000000000000000000000000000]]", 'date', 'date', r['date'])),
            td(span('commentaire', 'texte', r['commentaire']) if 'commentaire' in r else empty('commentaire', 'texte'), ''),
            td(images(portal, r['record'], r['file'], r['images']), '')]

AD72_HEADERS = ["Commune", "Cote", "Paroisse", "", "Description", ""]

def ad72_row(portal, r):
    key = f"{r['commune']}[[arko_fiche_0000000000c{r['record'][-3:]}]]"
    cells = [td(linked(portal['locality_field'], key, 'commune', 'relation_fiche', r['commune']), ''),
             td(span('cote', 'cote', r['cote']) if r.get('cote') else empty('cote', 'cote'), '')]
    if 'actes_only' in r:
        cells.append(td(span('actes_affichage', 'varchar', r['actes_only']), ''))
    else:
        cells += [td(span('paroisse', 'varchar', r['paroisse']) if 'paroisse' in r else empty('paroisse'), ''),
                  td(span('dates_extremes', 'varchar', r['dates']), ''),
                  td(span('actes_affichage', 'varchar', r['actes']), '')]
    cells.append(td(images(portal, r['record'], r['file'], r['images']), ''))
    return cells

def write(name, data):
    (OUT / name).write_text(json.dumps(data, ensure_ascii=False, indent=1) + '\n')

write('ad44-one.json', answer(AD44, AD44_HEADERS, [
    dict(record='arko_fiche_0000000000a01', file=900001, commune='Exampleville', paroisse='Saint-Exemple',
         actes='Baptêmes', date='1658-1668', cote='E dépôt 99', images=46)], ad44_row))
write('ad44-several.json', answer(AD44, AD44_HEADERS, [
    dict(record='arko_fiche_0000000000a11', file=900011, commune='Exampletown', ancienne='Exampletown-Ouest',
         paroisse='Saint-Martin', actes='Baptêmes, mariages et sépultures', date='1700-1701 (janvier)',
         commentaire='Table', cote='9 E 250 / 1', images=15),
    dict(record='arko_fiche_0000000000a12', file=900012, commune='Exampletown', ancienne='Exampletown-Est',
         paroisse='Saint-Médard', actes='Baptêmes, mariages et sépultures', date='1700', cote='9 E 251 / 2', images=16),
    dict(record='arko_fiche_0000000000a13', file=900013, commune='Exampletown', paroisse='Saint-Jacques',
         actes='Baptêmes, mariages et sépultures', date='1699-1700 (janvier)',
         commentaire='Collection départementale', cote='9 E 109 / 84', images=9),
    dict(record='arko_fiche_0000000000a14', file=900014, commune='Exampletown', paroisse='Saint-Similien',
         actes='Baptêmes, mariages et sépultures', date='1700', cote='9 E 109 / 214', images=48)], ad44_row))
write('ad44-none.json', answer(AD44, AD44_HEADERS, [], ad44_row))
write('ad44-period-segments.json', answer(AD44, AD44_HEADERS, [
    dict(record='arko_fiche_0000000000a21', file=900021, commune='Exampleville', paroisse='Saint-Exemple',
         actes='Sépultures', date='1598-1613 , 1656-1667', cote='E dépôt 99', images=120),
    dict(record='arko_fiche_0000000000a22', file=900022, commune='Exampleville', paroisse='Saint-Exemple',
         actes='Sépultures', date='1613-1655', cote='E dépôt 99', images=80)], ad44_row))

write('ad72-several.json', answer(AD72, AD72_HEADERS, [
    dict(record='arko_fiche_0000000000b01', file=910001, commune='Exampleville', cote='1MI 999 R1',
         dates='BMS 1595-1692 (consulter le détail dans la première vue)', actes='BMS', images=120),
    dict(record='arko_fiche_0000000000b02', file=910002, commune='Exampleville', cote='1MI 999 R1',
         dates='BMS 1692-1729', actes='BMS', images=89),
    dict(record='arko_fiche_0000000000b03', file=910003, commune='Exampleville', cote='1MI 999 R1',
         dates='BMS 1730-1764', actes='BMS', images=115),
    dict(record='arko_fiche_0000000000b04', file=910004, commune='Exampleville', cote='1MI 999 R1',
         dates='BMS 1765-1792', actes='BMS', images=113)], ad72_row))
write('ad72-text-match.json', answer(AD72, AD72_HEADERS, [
    dict(record='arko_fiche_0000000000b11', file=910011, commune='Saint-Jean-lès-le-Bourg', cote='2 Mi EC 999_6-8',
         dates='N 1793-1812', actes='N', images=263),
    dict(record='arko_fiche_0000000000b12', file=910012, commune='Bourg (Le)', cote='5Mi 999_108-110',
         dates='NMD 1793', actes='NMD', images=922),
    dict(record='arko_fiche_0000000000b13', file=910013, commune='Bourg (Le)', cote='5Mi 999_111-112',
         dates='NMD an III-an IV', actes='NMD', images=418),
    dict(record='arko_fiche_0000000000b14', file=910014, commune='Voisins-lès-le-Bourg', cote='2 Mi EC 998_1',
         dates='NMD 1793-1802', actes='NMD', images=301)], ad72_row, total=98))
write('ad72-after-1902.json', answer(AD72_LATE, ["Commune", "Cote", "Actes", ""], [
    dict(record='arko_fiche_0000000000b21', file=920001, commune='Exampleville', cote='frad072_9NUM999_0004',
         actes_only='N 1903 - 1912', images=214),
    dict(record='arko_fiche_0000000000b22', file=920002, commune='Exampleville', cote='frad072_9NUM999_0007',
         actes_only='N 1913 - 1922', images=135),
    dict(record='arko_fiche_0000000000b23', file=920003, commune='Exampleville', cote='frad072_9NUM999_0010',
         actes_only='N 1923 - 1925', images=32)], ad72_row))

def viewer(portal, record, numeric, file_id, count, licence, naan):
    sources = []
    for index in range(count):
        sources.append({
            "details": {"type": "url", "url": f"/_recherche-api/visionneuse-detail/{portal['engine']}/{record}/{portal['image_field']}/image/{file_id}"},
            "src": f"/_recherche-images/show/{numeric}/image/{file_id}/{index}",
            "infosImage": ({"@context": "http://iiif.io/api/image/2/context.json", "@id": None,
                            "protocol": "http://iiif.io/api/image", "width": 3352, "height": 2248}
                           if index == 0 else None),
            "ARKLink": f"/ark:{naan}/{index + 1:032x}.fiche={record}.moteur={portal['engine']}",
            "infosClasseur": {"refUniqueMoteur": portal['engine'], "refUniqueFiche": record,
                              "refUniqueField": portal['image_field'], "mediaType": "image",
                              "idArkoFile": file_id, "position": index + 1},
            "infosSourcesComplementaire": {"refUniqueFiche": record, "positionImage": index,
                                           "imageLienComplet": f"internal/register/{index + 1:04}.jpg"},
            "refUniqueFiche": record, "allowDownload": True, "title": "Exampleville - Registre"})
    return {"medias": [{"type": "image", "licence": licence, "loaded": True, "sources": sources}],
            "initMediaIndex": 0, "initSourceIndex": 0}

write('ad44-viewer.json', viewer(AD44, 'arko_fiche_0000000000a01', 100001, 900001, 3, '', '42067'))
write('ad44-info.json', {
    "@context": "http://iiif.io/api/image/2/context.json",
    "@id": "http://internal-host.example.invalid:8182/iiif/2/internal%2Fregister%2F0002.jpg",
    "protocol": "http://iiif.io/api/image", "width": 3352, "height": 2248,
    "sizes": [{"width": 105, "height": 70}, {"width": 210, "height": 141}, {"width": 419, "height": 281},
              {"width": 838, "height": 562}, {"width": 1676, "height": 1124}, {"width": 3352, "height": 2248}],
    "tiles": [{"width": 1024, "height": 1024, "scaleFactors": [1, 2, 4, 8, 16, 32]}],
    "profile": ["http://iiif.io/api/image/2/level2.json",
                {"formats": ["jpg", "png"], "qualities": ["default", "gray"], "supports": ["sizeByW", "cors"]}]})
print(sorted(p.name for p in OUT.iterdir()))
