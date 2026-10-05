"""Writes the anonymized Archinoë and Prismia fixtures beside this script:
`python3 generate.py` (the Prismia ones go to `../prismia/`).

They have the markup and JSON shapes of answers recorded from the portals,
with fictitious localities, parishes, call numbers and identifiers; no
recorded value is copied. Only what the adapters read is kept."""
import json
import pathlib

OUT = pathlib.Path(__file__).resolve().parent
PRISMIA = OUT.parent / 'prismia'


def write(directory, name, text):
    (directory / name).write_text(text, encoding='utf-8')


# ---------------------------------------------------------------- registre

def select(name, ident, options, extra=''):
    body = ''.join(f'<option value="{value}">{label}</option>' for value, label in options)
    return (f'<span class="input select"><span class="select_style"><select id="input{ident}" '
            f'name="{name}"   class="onchange" {extra} >'
            f'<option value="">-- Choisir --</option>{body}</select></span></span>')


LOCALITY_ATTRIBUTES = 'data-select="collection" data-page="registre"'


def registre_form(localities, acts, extra_selects=''):
    return ('<form id="form_registre" method="post" action="registre_liste.html">'
            f'<span class="clearfix  "><label for="inputcommune">Commune :</label>'
            f'{select("commune", "commune", localities, LOCALITY_ATTRIBUTES)}</span>'
            f'{extra_selects}'
            f'<span class="clearfix  "><label for="inputacte">Type d\'acte :</label>'
            f'{select("acte", "acte", acts)}</span>'
            '<span class="clearfix  "><label for="inputannee">Année (facultatif) :</label>'
            '<span class="input tel"><input type="tel" id="inputannee" name="annee" value=""   '
            'pattern="\\d*" maxlength="4" /></span></span>'
            '<input type="submit" id="btn_btn_submit" name="btn_submit" value="Rechercher"   ></form>\n')


# The collection select carries `data-othername="commune"`, which must not be
# read as the locality select.
COLLECTION_SELECT = ('<span class="clearfix  "><label for="inputcollection">Collection :</label>'
                     + select('collection', 'collection', [('100000019', 'Collection communale')],
                              'data-select="registre" data-page="registre" data-othername="commune"')
                     + '</span>')

AD17_LOCALITIES = [('100000001', 'Exampleville'), ('100000002', 'Le Bourg-Exemple'),
                   ('100000003', "Saint-Exemple-d'Aval"), ('100000000', '(aucun)')]
AD60_LOCALITIES = [('600000001', 'EXAMPLEVILLE'), ('600000002', 'LE BOURG-EXEMPLE'),
                   ('600000003', 'Saint-Exemple-sur-Mer')]
ACTS_17 = [('100000001', 'Baptêmes'), ('100000002', 'Mariages'), ('100000010', 'Tables décennales')]
ACTS_60 = [('', '&lt;Tous les types d\'actes&gt;'), ('600000001', 'Baptêmes'), ('600000010', 'Tables décennales')]


def row17(ident, cote, locality, acts, table, observation, period):
    cells = [cote, locality, 'Collection communale', 'Paroissial', acts, table]
    html = ''.join(f"<span class='Cell'><span>{c}</span></span>" for c in cells)
    html += (f"<span class='Cell popup_alert' data-alert='{observation}'><span>{observation}</span></span>"
             f"<span class='Cell'><span>{period}</span></span>")
    return f"<a class='Row lien_ext' href='visualiseur/registre.html?id={ident}'>{html}</a>"


HEAD17 = ('<div class="Heading">' + ''.join(
    f'<span class="Cell">{h}</span>' for h in
    ['Cote', 'Commune', 'Collection', 'Type de registre', 'Actes', 'Table',
     'Lacunes / Observations', 'Période']) + '</div>')


def results17(rows):
    return (f'<!--<a href="registre.html">Retour</a>-->\n\n<div class="total">{len(rows)} résultat(s)</div>\n'
            f'<div class="Table registre">{HEAD17}{"".join(rows)}</div>\n')


def row60(ident, cote, locality, parish, acts, period, comment=''):
    cells = [cote, locality, parish, acts, period]
    html = ''.join(f"<span class='Cell'><span>{c}</span></span>" for c in cells)
    html += f"<span class='Cell popup_alert' data-alert='{comment}'><span>{comment}</span></span>"
    return f"<a class='Row lien_ext' href='visualiseur/registre.html?id={ident}'>{html}</a>"


HEAD60 = ('<div class="Heading">' + ''.join(
    f'<span class="Cell">{h}</span>' for h in
    ['Cote', 'Commune', 'Paroisse', 'Actes', 'Dates<br/>extrêmes', 'Commentaires']) + '</div>')


def results60(rows):
    return ('<script>function alerte_nonnumerise() { alert("Ce registre n\'est pas numérisé."); }</script>\n'
            f'<div class="total">\n\t\t{len(rows)} résultat(s)\t\t\t\n</div>\n'
            f'<div class="scroller"><div class="Table registre">{HEAD60}{"".join(rows)}</div></div>\n')


NONE = '\n<div>Pas de résultat pour la recherche demandée.</div>'


def viewer(count):
    divs = ''.join(f'<div id="div_image_{n}" class="div_image"><img data-original="/cache/x/{n}.jpg"></div>'
                   for n in range(1, count + 1))
    return ('<html><body><div id="visu_pagination"></div>' + divs + '</body></html>\n')


def write_registre():
    write(OUT, 'ad17-form.html', registre_form(AD17_LOCALITIES, ACTS_17, COLLECTION_SELECT))
    write(OUT, 'ad17-one.html', results17([
        row17('100000101', '9 E 99/1', 'Exampleville', 'Baptêmes ', 'Table', 'Paroisse Saint-Exemple', '1620 - 1639')]))
    write(OUT, 'ad17-several.html', results17([
        row17('100000102', '9 E 99/2', 'Exampleville', 'Baptêmes Mariages Sépultures ', 'Pas de table',
              'Paroisse Saint-Exemple', '1699 - 1700'),
        row17('100000103', '9 E 99/3', 'Exampleville', 'Baptêmes Mariages Sépultures ', 'Pas de table',
              'Paroisse Notre-Dame-Exemple', '1699 - 1700'),
        row17('100000104', '9 E 99/4', 'Exampleville', 'Baptêmes Mariages Sépultures ', 'Pas de table',
              'Paroisse Sainte-Exemple', '1698 - 1701')]))
    write(OUT, 'ad17-with-table.html', results17([
        row17('100000105', '5 E 99', 'Exampleville', 'Tables décennales ', 'Table', 'section de Hameau', '1903 - 1912'),
        row17('100000106', '2 E 99/ 7', 'Exampleville', 'Naissances ', '', 'section de Hameau', '1912')]))
    write(OUT, 'ad17-none.html', NONE)
    write(OUT, 'ad60-licence.html',
          '<html><body><h1>Conditions de réutilisation</h1>'
          '<a href="registre.html">J\'accepte ces conditions</a></body></html>\n')
    write(OUT, 'ad60-form.html', registre_form(AD60_LOCALITIES, ACTS_60))
    write(OUT, 'ad60-one.html', results60([
        row60('600000101', '3E1/1', 'EXAMPLEVILLE', '', 'Baptême, Mariage, Sépulture', '1663-1708',
              'Lacunes pour 1672.')]))
    write(OUT, 'ad60-several.html', results60([
        row60('600000102', '3E57/9', 'LE BOURG-EXEMPLE', 'Notre-Dame-Exemple', 'Baptême, Mariage, Sépulture',
              '1598-1678'),
        row60('600000103', '3E57/14', 'LE BOURG-EXEMPLE', 'Saint-Exemple', 'Baptême, Mariage, Sépulture',
              '1577-1606')]))
    write(OUT, 'ad60-none.html', '<script>function alerte_nonnumerise() {}</script>\n' + NONE)
    write(OUT, 'viewer-12.html', viewer(12))
    write(OUT, 'viewer-40.html', viewer(40))


# ------------------------------------------------------------------ seriel

def seriel_row(index, ident, cote, lieu, dates, parish):
    return (f'<tr class="rechGrilleLignes{"Paires" if index % 2 == 0 else "Impaires"}" '
            f'onmouseover="this.className=\'rechGrilleLigneSurvolee\'"><td id="td_{index}_0" class="apercu">'
            f"<img src='ir_seriel_vignette.php?id={ident}' class='apercu_img' />"
            f'<a class="lien_externe" href="javascript:afficheImage({ident})" '
            f'title="Consulter le registre {lieu} - {dates}">Consulter</a></td>'
            f'<td id="td_{index}_1"><div class="notice"><h2><a href="javascript:afficheImage({ident})" '
            f'title="Consulter le registre {lieu} - {dates}">{lieu} - {dates}</a></h2>'
            '<table class="tab_notice" width="90%" align="center">'
            f'<tr><td class="rechDetailLibelle">Cote</td><td class="rechDetailValeur">{cote}</td></tr>'
            f'<tr><td class="rechDetailLibelle">Lieu</td><td class="rechDetailValeur">{lieu}</td></tr>'
            f'<tr><td class="rechDetailLibelle">Dates extrêmes</td><td class="rechDetailValeur">{dates}</td></tr>'
            '<tr><td class="rechDetailLibelle">Contenu</td><td class="rechDetailValeur">'
            f'<h3>Paroisse {parish}</h3><p>Baptêmes, mariages, sépultures.</p></td></tr>'
            '</table></div></td></tr>')


def seriel_page(total, rows):
    count = {0: 'Aucun résultat trouvé', 1: 'Un résultat trouvé'}.get(total, f'{total} résultats trouvés')
    return (f"<div  class='cnres' >{count}</div><div class='rechGrillePageDiv'></div>"
            '<table class="rechGrille" summary="Resultats de la recherche sous forme de grille" width="522px" >'
            '<tr class="rechGrilleEntete" ><th width="30px" >&nbsp;</th><th width="492px" >&nbsp;</th></tr>'
            + ''.join(rows) + '</table>')


def write_seriel():
    rows = [
        seriel_row(0, '300000101', '5 X 10/1', 'Exampleville', '1747-1764', 'Saint-Exemple'),
        seriel_row(1, '300000102', '5 X 10/2', 'Exampleville', '1748-1777', 'Notre-Dame-Exemple'),
        seriel_row(2, '300000103', '5 X 10/3', 'Exampleville', '1582-an I', 'Sainte-Exemple'),
        seriel_row(0, '300000104', '5 X 10/4', 'Exampleville', '1737-an I', 'Saint-Exemple-le-Haut'),
        seriel_row(1, '300000105', '5 X 10/5', 'Exampleville', '1738-an VII', 'Saint-Exemple-le-Bas'),
    ]
    write(OUT, 'ad62-page-0.html', seriel_page(5, rows[:3]))
    write(OUT, 'ad62-page-1.html', seriel_page(5, rows[3:]))
    write(OUT, 'ad62-one.html', seriel_page(1, [seriel_row(0, '300000104', '5 X 10/4', 'Exampleville',
                                                           '1737-an I', 'Saint-Exemple-le-Haut')]))
    write(OUT, 'ad62-none.html', seriel_page(0, []))
    write(OUT, 'ad62-article.html', seriel_page(1, [seriel_row(0, '300000106', '3 E 99/7', 'Le Bourg-Exemple',
                                                               '1927-1930', 'Saint-Exemple')]))
    write(OUT, 'ad62-challenge.html',
          '<html><head><meta http-equiv="Pragma" content="no-cache">'
          '<script type="text/javascript" src="/TSPD/0000000000000000?type=9"></script></head>'
          '<body>Please enable JavaScript to view the page content.</body></html>')


# --------------------------------------------------------------------- ead

def ead_entry(ident, name):
    return (f"<li><div id='item_{ident}' class='sommaire_entree sommaire_level1 aaa'>"
            f"<a href='javascript:sommaireExpand({ident})' id='bouton_{ident}' class='li_ouvre'></a>"
            f"<a href='javascript:showEntry({ident})'>{name}</a></div></li>")


def ead_root():
    # The aid's page is ISO-8859-1; a transport that decodes it as UTF-8
    # turns each accented letter into U+FFFD.
    communes = [(400000001, 'Exampleville'), (400000002, 'Ch�tel-Exemple'),
                (400000003, "Etang-Exemple (L')"), (400000004, 'Hameau-Exemple (Le)')]
    return ('<html><head><title>Etat civil</title></head><body>'
            "<div id='sommaire_contenu'><div id='toc' class='sommaire_entree'>"
            "<a href='javascript:showGeneral()'>Pr&eacute;sentation du fonds</a></div>"
            '<ul class=\'ul_sommaire\' >' + ''.join(ead_entry(i, n) for i, n in communes)
            + '</ul></div></body></html>')


def ead_toc(entries):
    items = ''.join(
        f"<li><div id='item_{i}' class='sommaire_entree'><div id='item_{i}' class='sommaire_entree'>"
        f"<a href='javascript:sommaireExpand({i})' id='bouton_{i}' class='li_ouvre'></a>"
        f"<a href='javascript:showEntry({i})' alt='{t}' title='{t}'>{t}</a></div>"
        f"<div id='toc_container_{i}'></div></li>" for i, t in entries)
    return (f'<div class="div_sommaire_sous_niveau"><ul class="sommaire_sous_niveau">{items}</ul></div>'
            "<div style='height:0px'></div>")


def ead_block(ident, link, cote, title, dates, images):
    count = f'<span class="editable" id="detailn_{ident + 11}">{images} images numériques</span>' if images else ''
    viewer_link = (f'<a href="#" onclick="lienImage({link})                    ">'
                   f'<img src="ir_ead_vignette.php?id={link}" class="editable editableImg" id="{link}">'
                   'Consulter</a>' if images else '')
    return (f'<div id="item_{ident}"><h3><div id="detailn_{ident}" style=""><div><table width="100%"><tr>'
            '<td valign="baseline" width="25">   </td><td align="left" valign="top" width="150" class="cotes">'
            f"<a href='#' onclick='if (jQuery!=undefined){{jQuery.ajax({{url:&#039;/console/x.php?ir=1&id={ident}&#039;}})}}'>"
            "<span style='padding-left:21px'></span></a> "
            f'<span class="editable" id="detailn_{ident + 1}">{cote}</span>  <span class="editable" id="">\n</span></td>'
            f'<td align="left" valign="top" class="titres"> <span class="editable" id="detailn_{ident + 2}">{title}</span>'
            ' <span class="editable" id="">\n</span></td>'
            '<td align="right" valign="top" width="200" class="dates" nowrap> <span class="editable" id="">\n</span>  '
            f'<span class="editable" id="detailn_{ident + 3}">{dates}</span></td></tr></table></div></div></h3>'
            '<div><table class="ead_detail"><tr><td class="ead_detail_libelle">Description physique</td>'
            f'<td class="ead_detail_contenu"><span class="editable" id="detailn_{ident + 9}">\n</span> {count}</td></tr>'
            f'<tr><td class="ead_detail_libelle">Documents liés</td><td class="ead_detail_contenu">{viewer_link}</td></tr>'
            '</table></div></div>')


def ead_notice(title, blocks):
    return ("<div id='fil_de_fer' boo='boo'><a href='javascript:showEntry(400000001)'>Exampleville</a></div>"
            "<div class='notice_main' id='notice_main'><div class='ir_entree_2'>"
            f'<h2> <span class="editable" id="detailn_1">{title}</span></h2><br><br>' + ''.join(blocks) + '</div></div>')


def write_ead():
    write(OUT, 'ad21-root.html', ead_root())
    write(OUT, 'ad21-toc-commune.html', ead_toc([(400000010, 'Actes (BMS puis NMD)'), (400000020, 'Tables décennales'),
                                                (400000030, 'Tables décennales cantonales')]))
    write(OUT, 'ad21-toc-actes.html', ead_toc([(400000011, 'Collection communale'),
                                              (400000012, 'Collection départementale')]))
    write(OUT, 'ad21-notice-communale.html', ead_notice('Collection communale', [
        ead_block(400000100, 400000201, 'FRAD021EC 9/001', "Registres paroissiaux et/ou d'état civil : 1648 - 1699",
                  '1648-1699', 107),
        ead_block(400000120, 400000202, 'FRAD021EC 9/002', "Registres paroissiaux et/ou d'état civil : 1700 - 1740",
                  '1700-1740', 194)]))
    write(OUT, 'ad21-notice-departementale.html', ead_notice('Collection départementale', [
        ead_block(400000140, 400000203, 'FRAD021EC 9/003', "Registres d'état civil : 1700 - 1740",
                  '1700-1740', 194),
        ead_block(400000160, 400000204, 'FRAD021EC 9/004', "Registres d'état civil : 1741 - 1792",
                  '1741-1792', 0)]))
    write(OUT, 'ad21-notice-tables.html', ead_notice('Tables décennales', [
        ead_block(400000180, 400000205, 'FRAD0213E 9/001-01', 'Tables décennales des actes de 1793 à 1802',
                  '1793-1802', 9),
        ead_block(400000200, 400000206, 'FRAD0213E 9/002', 'Tables décennales des actes de 1803 à 1812',
                  '1803-1812', 5)]))


# ----------------------------------------------------------------- prismia

API = 'https://ad47.backend.archives.prismia.fr/api'


def stub(ident, cote, images, years, parish, bounds=('1700-01-01', '1740-12-31')):
    return {
        'score': 0,
        'id': f'{API}/iiif/presentation/v3/{ident}/manifest',
        'type': 'Manifest',
        'prismUserId': str(ident),
        'prismCoteId': cote,
        'prismNbMedias': images,
        'label': {'fr': [f'Baptêmes, Mariages, Sépultures-Exampleville-{bounds[0][:4]}-{bounds[1][:4]}']},
        'prismNavDate': {'greaterThan': None, 'greaterThanOrEqualTo': f'{bounds[0]}T00:00:00+01:00',
                         'lessThan': None, 'lessThanOrEqualTo': f'{bounds[1]}T00:00:00+01:00'},
        'prismNavDateValue': years,
        'metadata': [],
        'listIndexationAgg': [
            {'tagLabel': 'Lieu géographique', 'tagArchivistiqueValues': ['Exampleville (Commune)']},
            {'tagLabel': 'Paroisse', 'tagArchivistiqueValues': parish},
            {'tagLabel': 'Actes', 'tagArchivistiqueValues': ['Baptêmes ou Naissances', 'Mariages']},
        ],
        'thumbnail': [],
        'items': [],
    }


def prismia_answer(stubs, total=None):
    return json.dumps({'searchQuery': {}, 'total': len(stubs) if total is None else total, 'maxScore': 0,
                       'listResponseObject': stubs}, ensure_ascii=False, indent=1) + '\n'


def write_prismia():
    PRISMIA.mkdir(exist_ok=True)
    write(PRISMIA, 'runtime-config.js',
          "window.prismConfig = {\n    apiKey: 'exampleKey0123',\n"
          "    serverUrl: 'https://ad47.backend.archives.prismia.fr/api/',\n\ttitle: 'Archives départementales d\\'Exemple',\n  };\n")
    write(PRISMIA, 'facets.json', json.dumps({
        'total': 0, 'nextAfter': 'Exampleville-sur-Mer',
        'searchAggsMetaTag': [
            {'key': 'Exampleville', 'label': 'Exampleville', 'docCount': 506},
            {'key': 'Exampleville-d’Aval', 'label': 'Exampleville-d’Aval', 'docCount': 131},
            {'key': 'Mas-Exemple (Le)', 'label': 'Mas-Exemple (Le)', 'docCount': 88},
            {'key': "Passage-d'Exemple (Le)", 'label': "Passage-d'Exemple (Le)", 'docCount': 12}],
        'aggsMetaTagGeognameList': []}, ensure_ascii=False, indent=1) + '\n')
    write(PRISMIA, 'query-one.json', prismia_answer([
        stub(900000001, 'E SUP EXEMPLE GG-1', 216, '1673-1681, 1686, 1692-1723, 1730, 1752-1753',
             ['Saint-Exemple, Notre-Dame-Exemple'], ('1673-01-01', '1753-12-31'))]))
    write(PRISMIA, 'query-several.json', prismia_answer([
        stub(900000002, '4E9-1', 29, '', ['Saint-Exemple'], ('1745-01-01', '1760-12-31')),
        stub(900000003, '4E9-2', 45, '1740-1752', ['Notre-Dame-Exemple'], ('1740-01-01', '1752-12-31')),
        stub(900000004, 'E SUP EXEMPLE GG-2', 192, '1700-1760', ['Sainte-Exemple'], ('1700-01-01', '1760-12-31'))]))
    write(PRISMIA, 'query-none.json', prismia_answer([]))


if __name__ == '__main__':
    write_registre()
    write_seriel()
    write_ead()
    write_prismia()
