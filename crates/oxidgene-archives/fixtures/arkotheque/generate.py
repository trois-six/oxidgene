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

# The live checks' discovery (Archive Portals §9.1): the search page names
# its engine and content components; the engine's bare answer declares its
# filters, display modes and, per filter field, the values it offers, the
# most frequent first, with their record keys. The localities are fictitious;
# the act values are the catalogue's.
(OUT / 'ad44-search-page.html').write_text(
    '<!DOCTYPE html>\n<html lang="fr">\n<head><title>Recherche</title></head>\n<body>\n'
    f'<div class="arko-recherche filtres_facettes" data-moteur="{AD44["engine"]}" data-contenu="1289790" data-component="filtres"></div>\n'
    f'<div class="arko-recherche resultats" data-moteur="{AD44["engine"]}" data-contenu="1289789" data-component="resultats"></div>\n'
    '</body>\n</html>\n')

def terms(field, keys):
    return {field: {"doc_count": sum(count for _, count in keys),
                    f"{field}_terms": {"doc_count_error_upper_bound": 0, "sum_other_doc_count": 0,
                                       "buckets": [{"key": key, "doc_count": count} for key, count in keys]}}}

AD44_ACT_FIELD = 'arko_default_6a6b489eb7a6c'
write('ad44-engine.json', {
    "refUnique": AD44['engine'],
    "filtres": [
        {"refUnique": "arko_default_6a6b4ba5532ce", "type": "select", "intitule": "Commune",
         "properties": [{"fieldName": AD44['locality_field']}]},
        {"refUnique": "arko_default_6a6b4ba578da0", "type": "select", "intitule": "Type d'acte",
         "properties": [{"fieldName": AD44_ACT_FIELD}]},
        {"refUnique": "arko_default_6a6b4ba58e64b", "type": "slider", "intitule": "Période",
         "properties": [{"fieldName": AD44['period_field']}]}],
    "restits": [{"refUnique": "arko_default_6a6b4d95b8dc8", "mode": {"intituleCourt": "tableau"}}],
    "frontConfig": {"possibleResultSize": [25, 50, 100]},
    "resultats": {"total": 3, "count": 0, "results": [], "html": "", "aggregations": [{
        # The most frequent first; the check takes the alphabetical first.
        **terms(AD44['locality_field'], [("Sampleton[[arko_fiche_0000000000c002]]", 120),
                                         ("Exampleville[[arko_fiche_0000000000c001]]", 80)]),
        **terms(AD44_ACT_FIELD, [("Baptêmes et naissances[[arko_fiche_6a6b3d70f0fdf]]", 90),
                                 ("Baptêmes[[arko_fiche_6a6b3d70f2db3]]", 40),
                                 ("Naissances[[arko_fiche_6a6b3d7100e74]]", 30),
                                 ("Mariages[[arko_fiche_6a6b3d7104771]]", 30),
                                 ("Sépultures[[arko_fiche_6a6b3d71061cb]]", 20),
                                 ("Décès[[arko_fiche_6a6b3d7107318]]", 20),
                                 ("Tables décennales[[arko_fiche_6a6b3d7108c8d]]", 10)])}]}})

# The shapes the other portals' engines render (Archive Portals §4.3): cells
# named by `data-champ`, several spans of one name, bare cells read by their
# column, titles holding a call number with the locality or the period
# after it, rows without an image count, and pages of a long answer. Each
# fixture serves the catalogued collection named in its comment.

def generic_row(portal, r):
    """`r['cells']`: (champ or None, text or list of texts) in column order."""
    cells = []
    for champ, value in r['cells']:
        if champ is None:
            cells.append(td(esc(value)))
        elif isinstance(value, list):
            items = ''.join(f'<li>{span(champ, "varchar", v)}</li>' for v in value)
            cells.append(td(f'<ul>{items}</ul>', ''))
        else:
            cells.append(td(span(champ, 'varchar', value), ''))
    if r.get('viewer', True):
        zone = images(portal, r['record'], r['file'], r['images'])
        if r['images'] is None:
            zone = zone.replace('<span class="nombre_images">(None images)</span>', '')
        cells.append(td(zone, ''))
    return cells

def generic(portal, registers, total=None):
    rows = [row(generic_row(portal, register)) for register in registers]
    results = [{"id": 200000 + index, "intitule": register.get('title', ''),
                "refUnique": register['record']} for index, register in enumerate(registers)]
    return {"refUnique": portal['engine'],
            "resultats": {"total": len(registers) if total is None else total,
                          "count": len(registers), "results": results,
                          "html": table([], rows), "aggregations": [], "modeRestit": "tableau"}}

def record(n):
    return f'arko_fiche_{n:013x}'

# fr-ad36 `registers`: the act filter value `Baptêmes / Naissances` serves
# both baptisms and births, so the act cells, one span per kind, tell them
# apart.
AD36 = dict(engine='arko_default_61a0b234355b3', image_field='arko_default_0000000003601')
write('ad36-shared-acts.json', generic(AD36, [
    dict(record=record(0xd01), file=930001, images=95, title='9 E 999/1', cells=[
        ('cote', '9 E 999/1'), ('commune', 'Exampleville'), ('type_acte', ['Baptêmes', 'Mariages', 'Sépultures']),
        ('date', '1690-1800')]),
    dict(record=record(0xd02), file=930002, images=284, title='9 E 999/2', cells=[
        ('cote', '9 E 999/2'), ('commune', 'Exampleville'), ('type_acte', ['Naissances']), ('date', '1793-1802')])]))

# fr-ad15 `registers`: the locality qualified by its department, a hamlet's
# by its commune too; the acts in a bare cell, the period in the title.
AD15 = dict(engine='arko_default_5f8ef8b61e0d4', image_field='arko_default_0000000001501')
write('ad15-qualified.json', generic(AD15, [
    dict(record=record(0xe01), file=940001, images=327, title='1737-1754', cells=[
        ('isadg_unitid', '9 Mi 99/2'), (None, 'Exampleville Collection départementale Registres paroissiaux Baptêmes, mariages, sépultures'),
        ('isadg_controlaccess_geogname', 'Exampleville (Exemple, France)')]),
    dict(record=record(0xe02), file=940002, images=266, title='1730-1792', cells=[
        ('isadg_unitid', '9 Mi 98/5'), (None, 'Hameau Collection départementale Registres paroissiaux Baptêmes, mariages, sépultures'),
        ('isadg_controlaccess_geogname', 'Hameau (Exampleville, Exemple, France)')]),
    dict(record=record(0xe03), file=940003, images=120, title='1846-1867', cells=[
        ('isadg_unitid', '9 Mi 99/5'), (None, 'Exampleville Collection départementale Etat civil Naissances'),
        ('isadg_controlaccess_geogname', 'Exampleville (Exemple, France)')])]))

# fr-ad08 `military-registers`: no locality filter, the bureau in a cell, the
# matricules a volume spans in two cells.
AD08 = dict(engine='arko_default_6777a77d98594', image_field='arko_default_0000000000801')
write('ad08-matricules.json', generic(AD08, [
    dict(record=record(0xf01 + index), file=950001 + index, images=150, title='9R 155', cells=[
        ('ark_fiche_cote', '9R 155'), ('ark_fiche_date', '1900'), ('bureau', bureau), ('intitule', 'Registre matricule'),
        ('mat_debut', str(first)), ('mat_fin', str(first + 99))])
    for index, (bureau, first) in enumerate([('Exampleville', 1), ('Exampleville', 101), ('Sampleton', 1)])]))

# fr-ad10 `registers`: the call number in the title, followed by the
# locality.
AD10 = dict(engine='arko_default_694266c456574', image_field='arko_default_0000000001001')
write('ad10-titles.json', generic(AD10, [
    dict(record=record(0xa01 + index), file=960001 + index, images=count, title=f'9E{number} Bourg (Le)', cells=[
        ('communes_aube', 'Bourg (Le)'), ('date', period), ('type_acte', ['Naissance'])])
    for index, (number, period, count) in enumerate([(99901, '1848-1860', 304), (99902, '1861-1872', 280)])]))

# fr-ad65 `civil-status`: the call number in the title, followed by the
# period; one value of the act filter for every kind, the kinds as codes.
AD65 = dict(engine='arko_default_636924f5785e7', image_field='arko_default_0000000006501')
write('ad65-codes.json', generic(AD65, [
    dict(record=record(0xb01 + index), file=970001 + index, images=count, title=f'9 E 9/{volume} 1802 - 1803', cells=[
        ('commune', localities), ('titre_court', kind), ('fr_date_publication', 'An XI')])
    for index, (volume, kind, count, localities) in enumerate([
        (4, 'N', 3, ['Exampleville']), (5, 'M', 2, ['Sampleton', 'Exampleville']), (6, 'D', 4, ['Exampleville'])])]))

# fr-ad72 `censuses`: several lists a year — by alphabetical part, by
# collection —, told apart by call number and image count.
AD72_CENSUS = dict(engine='arko_default_6319dda7da548', image_field='arko_default_0000000007201')
write('ad72-census.json', generic(AD72_CENSUS, [
    dict(record=record(0xc01 + index), file=980001 + index, images=count, title=cote, cells=[
        ('commune', 'Exampleville'), ('cote', cote), ('date_affichage', period)])
    for index, (cote, period, count) in enumerate([
        ('9 Mi 9999_ 19', '1931 (A-H, collection communale)', 662), ('9 Mi 9999_ 20', '1931 (I-Z, collection communale)', 540),
        ('9 M 999/2', '1931 (collection départementale)', 662), ('9 Mi 9999_ 18', '1926', 610)])]))

# fr-ad72 `registers-before-1902`, a populated locality: the engine has no
# period filter, so the cited register may be on a later page (100 rows on
# the portal, 3 here); the text match also brings other localities.
AD72_PAGES = dict(AD72)
def populated(first, count, cited=None):
    registers = []
    for index in range(first, first + count):
        commune = "Saint-Exemple-lès-le-Bourg" if index % 2 == 0 else "Bourg (Le)"
        registers.append(dict(record=record(0x1000 + index), file=990000 + index, images=200 + index % 50,
                              title=f'9Mi 999_{index}', cells=[
                                  ('commune', commune), ('cote', f'9Mi 999_{index}'),
                                  ('dates_extremes', f'M {1700 + index}'), ('actes_affichage', 'M')]))
    if cited is not None:
        at, cote, period, images_count = cited
        registers[at] = dict(record=record(0x2000), file=999999, images=images_count, title=cote, cells=[
            ('commune', 'Bourg (Le)'), ('cote', cote), ('dates_extremes', period), ('actes_affichage', 'M')])
    return registers
write('ad72-page-1.json', generic(AD72_PAGES, populated(0, 3), total=5))
write('ad72-page-2.json', generic(AD72_PAGES, populated(3, 2, (1, '9Mi 999_374-376', 'M 1880-1882', 567)), total=5))

# fr-ad24 `censuses`: the year filter takes its listed value with its key.
AD24 = dict(engine='arko_default_6076a37360df7', image_field='arko_default_0000000002401')
write('ad24-census-engine.json', {
    "refUnique": AD24['engine'],
    "filtres": [
        {"refUnique": "arko_default_6076a4257d83d", "type": "select", "intitule": "Commune",
         "properties": [{"fieldName": "arko_default_6076a29089cef"}]},
        {"refUnique": "arko_default_6076a425828ef", "type": "select", "intitule": "Année de recensement",
         "properties": [{"fieldName": "arko_default_6076a3b1d558e"}]}],
    "restits": [{"refUnique": "arko_default_6076a524314f9", "mode": {"intituleCourt": "tableau"}}],
    "resultats": {"total": 2, "count": 0, "results": [], "html": "", "aggregations": [{
        "arko_default_6076a29089cef": {"buckets": [{"key": "Exampleville (Exemple, France)[[arko_fiche_0000000000c101]]", "doc_count": 20}]},
        **terms("arko_default_6076a3b1d558e", [("1836 [[0000000000000000000000000000000000001836]]", 12),
                                              ("1841 [[0000000000000000000000000000000000001841]]", 11)])}]}})
write('ad24-census.json', generic(AD24, [
    dict(record=record(0x3001), file=999101, images=11, title='FRAD099_6MI11', cells=[
        ('lieux', 'Exampleville (Exemple, France)'), ('precision', 'Hameau'), ('date', '1836'), ('cote', 'FRAD099_6MI11')]),
    dict(record=record(0x3002), file=999102, images=None, title='FRAD099_6MI12', cells=[
        ('lieux', 'Exampleville (Exemple, France)'), ('date', '1841'), ('cote', 'FRAD099_6MI12')])]))
# fr-ad40 `registers`, a portal read by its pages: the search page as its
# scripts render it, with the filters drawn by their references, the results
# table under the results component and the count in its heading, or a
# notice when nothing matched; no record titles, each record named by its
# row's viewer address only.
AD40 = dict(engine='arko_default_62a88e82782fb', image_field='arko_default_0000000004001')
AD40_FILTERS = ['arko_default_62a88ef4ebacf', 'arko_default_62a88ef500cdf', 'arko_default_63b88760a57af']

def rendered(portal, registers, total=None):
    rows = [row(generic_row(portal, register)) for register in registers]
    count = len(registers) if total is None else total
    filters = ''.join(
        f'<div class="filtre_de_recherche"><div aria-controls="aria-filtre-{ref}" role="button"></div>'
        f'<div class="filtre_de_recherche_items sous_field_{ref}" id="aria-filtre-{ref}"></div></div>'
        for ref in AD40_FILTERS)
    shown = f'{count:,}'.replace(',', '\u202f')
    heading = ('<div class="tetiere_resultat_facette"><div class="nombre_resultat_facettes" aria-live="polite" '
               f'aria-label="{count} résultats"><span>{shown}</span>  résultats</div></div>')
    engine = portal['engine']
    headers = ["Commune", "Paroisse ou ancienne commune", "Période", "Type d'acte", "Observations", "Cote", "Images"]
    # Nothing matched: the component shows a notice, no count and no table.
    results = (f'{heading}<div class="recherche-resultats-container">{table(headers, rows)}</div>' if rows
               else '<div class="alerte"><p>Aucun résultat</p></div>')
    return ('<!DOCTYPE html><html lang="fr"><head><title>État civil</title></head><body>\n'
            f'<div class="arko-recherche filtres_facettes" data-moteur="{engine}" data-contenu="1736088" '
            f'data-component="filtres">{filters}</div>\n'
            f'<div class="arko-recherche resultats_facettes" data-moteur="{engine}" data-contenu="1736087" '
            f'data-component="resultats"><div>{results}</div></div>\n</body></html>\n')

def landes(index, commune, period, act, cote, images, places=()):
    return dict(record=record(0x4000 + index), file=994000 + index, images=images, cells=[
        ('commune', commune), ('autres_lieux', list(places)), ('periode', period),
        ('type_acte', [act]), (None, ''), ('cote', cote)])

(OUT / 'ad40-rendered.html').write_text(rendered(AD40, [
    landes(1, 'Exampleville', '1845-1848', 'Naissances', '9 E 99/1', 210),
    landes(2, 'Exampleville', '1849-1852', 'Naissances', '9 E 99/2', 296, ['Saint-Exemple']),
    landes(3, 'Exampleville-lès-Bois', '1849-1855', 'Naissances', '9 E 98/4', 120),
], total=1234))
(OUT / 'ad40-rendered-none.html').write_text(rendered(AD40, [], total=0))

print(sorted(p.name for p in OUT.iterdir()))
