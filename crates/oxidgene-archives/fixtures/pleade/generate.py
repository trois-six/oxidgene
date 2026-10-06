"""Writes the anonymized Pleade fixtures beside this script: `python3 generate.py`.

They have the markup and JSON shapes of the answers recorded from the
portals, with fictitious localities, call numbers, component identifiers and
ARK names; no recorded value is copied, and only what the adapter reads is
kept."""
import json
import pathlib

OUT = pathlib.Path(__file__).resolve().parent
MAY = 'https://archives.lamayenne.fr/archives-en-ligne'
P64 = 'https://earchives.le64.fr/archives-en-ligne'


def write(name, text):
    (OUT / name).write_text(text, encoding='utf-8')


# ------------------------------------------------------------- the form

LABELS = [
    'Bourg-Exemple (Exemple, France)',
    "Exampleville (Exemple, France ; jusqu'à 1919) [aujourd'hui : Exampleville-sur-Mer (Exemple, France)]",
    'Exampleville-sur-Mer (Exemple, France)',
    'Chapelle-Exemple du Bourg',
    'Sampleton (Exemple, France)',
    "Sampleton (Exemple, France ; jusqu'à 1840) [aujourd'hui : Sampleton (Exemple, France)]",
    'Le Hameau-Exemple (Exemple, France)',
]
FACETS = 'udate;fgeogcommune;fanciennescommune;finstitution_paroisse;ftypedocec;flisteacte;fbdate'


def hidden(name, value, extra=''):
    return f'<input type="hidden" {extra}name="{name}" value="{value}"/>'


def options(values):
    return '<option value=""></option>' + ''.join(f'<option value="{v}">{v}</option>' for v in values)


write('may-form.html', '<!DOCTYPE html><html><body><div class="pl-form">'
      f'<form action="{MAY}/custom-results.html" id="etat-civil" name="etat-civil" method="get" class="pl-form-advanced-srch">'
      + hidden('base', 'ead2', 'class="pl-form-srch-base" ')
      + hidden('rbase', 'ead2', 'class="pl-form-srch-rbase" ')
      + hidden('form-display-modes', 'custom')
      + hidden('facets', FACETS)
      + hidden('name', 'etat-civil') + hidden('linkBack', 'true') + hidden('n-start', '0')
      + hidden('cop1', 'AND') + hidden('champ1', 'fcommunes_paroisses', 'id="query1-field" ')
      + hidden('cop2', 'AND') + hidden('champ2', 'ftypedocec', 'id="query2-field" ')
      + hidden('cop3', 'AND') + hidden('champ3', 'id_annee')
      + hidden('skippage', 'here') + hidden('sf', 'fbdate', 'class="pl-form-srch-sf" ')
      + f'<select id="ead2-fcommunes_paroisses-slct" name="query1" tabindex="1" class="pl-form-slct">{options(LABELS)}</select>'
      + f'<select id="ead2-ftypedocec-slct" name="query2" tabindex="2" class="pl-form-slct">{options(["Registres d&#039;actes", "Tables"])}</select>'
      + '<input type="text" name="du3" id="query3_du" placeholder="AAAA"/>'
      + '<input type="text" name="db3" id="query3_db" placeholder="AAAA"/>'
      + '<input type="text" name="de3" id="query3_de" placeholder="AAAA"/>'
      + '<select id="cal-form-0-0" class="pl-form-slct" name="JR" size="1"><option value="Jours">Jours</option></select>'
      + '</form></div></body></html>\n')


def row(parity, ark, component, locality, period, kind, acts, call_number, comment=''):
    paragraphs = ''.join(f'<p>{text}</p>' for text in [kind] + acts)
    return (f'<tr class="{parity}">'
            f'<td class="img-tab"><a href="{MAY}/ark:/99999/{ark}?context=ead2::{component}" '
            f'onclick="return windowManager.winFocus(this.href, \'viewer\');" class="pl-pgd-medias-thumbnail">'
            f'<img src="{MAY}/ark:/99999/{ark.split("/")[0]}/f1.miniature" title="Voir"/></a></td>'
            f'<td class="commune" data-title="Commune"><p>{locality}</p></td>'
            f'<td class="date" data-title="Année">{period}</td>'
            f'<td class="type" data-title="Type">{paragraphs}</td>'
            f'<td class="cote" data-title="Cote">{call_number}</td>'
            f'<td class="commentaires" data-title="Commentaires">{comment}</td>'
            f'<td class="panier-tab" data-title=""><button class="pl-bskt-bttn-doc">Ajouter</button></td></tr>')


def results(rows, total=None, page=1, pages=1):
    total = len(rows) if total is None else total
    word = 'résultat' if total == 1 else 'résultats'
    pager = '' if pages == 1 else (
        '<span id="_pagination" class="navpage"><a onclick="windowManager.loadAjaxContent(\'functions/ead/'
        f'custom-results.ajax-html?base=ead2&amp;p={page + 1}\');" class="navigation-type-next">&gt;</a></span>')
    count = (f'<p class="pl-results-count"><span class="nbresults">{total} {word} </span>'
             f'<span class="nbpages">Page <strong>{page}</strong> de {pages}</span>{pager}</p>')
    return ('<!DOCTYPE html\n  PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN" "http://www.w3.org/TR/html4/loose.dtd">\n'
            '<div xmlns="http://www.w3.org/1999/xhtml" class="pl-list-results-and-facets">'
            '<div id="results-facet-box"><ul><li><a href="?base=ead2"><span class="facet-filter-value">Exampleville (Exemple, France)</span></a></li></ul></div>'
            f'<div id="pl-tab-results-ajax" class="pl-results-ajax">{count}'
            '<div class="pl-results"><table id="table-results" summary="Les résultats de recherche" class="tri">'
            '<thead class="titres"><tr><th class="img-tab"></th><th class="tcommune"><span>Commune</span></th>'
            '<th class="thate"><span>Année</span></th><th class="ttype"><span>Type</span></th><th class="tCote"><span>Cote</span></th>'
            '<th class="tcommentaires"><span>Commentaires</span></th><th class="panier-tab"></th></tr></thead>'
            f'<tbody class="results-tab">{"".join(rows)}</tbody></table></div>{count}</div></div>\n')


ACTS = "Registres d&#039;actes"
EX = 'Exampleville-sur-Mer (Exemple, France)'
write('may-results-one.html', results([
    row('odd', 'rexample0001/f1', 'FRAD999_EX_de-1', EX, '1843-1852', ACTS, ['Naissances', 'Mariages', 'Décès'], '9 E 99/11'),
]))
write('may-results-several.html', results([
    row('odd', 'rexample0002/f1', 'FRAD999_EX_de-2', EX, '1793-1800', ACTS, ['Naissances', 'Mariages', 'Décès'], 'E dépôt 999/E8'),
    row('even', 'rexample0003/3_0001.JPG', 'FRAD999_EX_de-3', EX, '1793-1802', ACTS, ['Naissances', 'Mariages', 'Décès'], '9 E 99/6'),
    row('odd', 'rexample0004/f1', 'FRAD999_EX_de-4', EX, '1656-1676', ACTS, ['Baptêmes', 'Mariages', 'Sépultures'], 'E dépôt 999/E2',
        'B.1662-1667, S.1656-1667'),
], total=21, pages=2))
write('may-results-page-2.html', results([
    row('odd', 'rexample0021/f1', 'FRAD999_EX_de-21', EX, '1923-1932', ACTS, ['Naissances', 'Mariages', 'Décès'], '9 E 99/21'),
], total=21, page=2, pages=2))
write('may-results-tables.html', results([
    row(parity, f'rexample01{n}/f1', f'FRAD999_EX_de-1{n}', EX, '1843-1852', 'Tables', [f'Tables décennales des {kind}'], '9 E 335/10')
    for n, (parity, kind) in enumerate([('odd', 'naissances'), ('even', 'mariages'), ('odd', 'décès')])
]))
write('may-results-sampleton.html', results([
    row('odd', 'rexample0031/f1', 'FRAD999_SA_de-1', "Sampleton (Exemple, France ; jusqu'à 1840) [aujourd'hui : Sampleton (Exemple, France)]",
        '1800-1809', ACTS, ['Naissances', 'Mariages', 'Décès'], '9 E 98/1'),
]))
write('may-results-none.html', '<!DOCTYPE html\n  PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN" "http://www.w3.org/TR/html4/loose.dtd">\n'
      '<div xmlns="http://www.w3.org/1999/xhtml" class="pl-list-results-and-facets"><div id="pl-tab-results-ajax" class="pl-results-ajax">'
      '<p class="pl-message"><h4 class="pl-results-zero"><strong>Aucun résultat</strong> n\'a été trouvé pour votre recherche.</h4></p>'
      '</div></div>\n')


def manifest(base, name, count):
    canvases = [{'@id': f'{base}/ark:/99999/{name}/f{n}', '@type': 'sc:Canvas', 'label': str(n),
                 'height': 3000, 'width': 2000, 'images': []} for n in range(1, count + 1)]
    return json.dumps({'@context': 'http://iiif.io/api/presentation/2/context.json',
                       '@id': f'{base}/iiif/ark:/99999/{name}/manifest.json', '@type': 'sc:Manifest',
                       'label': 'Exampleville', 'attribution': 'Archives fictives',
                       'sequences': [{'@type': 'sc:Sequence', 'canvases': canvases}]}, indent=1) + '\n'


write('manifest-12.json', manifest(MAY, 'rexample0002', 12))
write('manifest-40.json', manifest(MAY, 'rexample0003', 40))

# ------------------------------------------------------------------ trees


def li(ident, title, illustrated=None, children=None, base=P64, aid='FRAD999_IR0001'):
    span = f'<span class="{illustrated}">&nbsp;\n<!--u--></span>' if illustrated else ''
    nested = '' if children is None else f'<ul>{"".join(children)}</ul>\n'
    return (f'<li id="{ident}">{span}<a name="link" href="https://example.org/pleade/ead.html?id={aid}&amp;c={ident}">'
            f'{title}</a>{nested}</li>\n')


def toc(items):
    return ('<!DOCTYPE html\n  PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN" "http://www.w3.org/TR/html4/loose.dtd">\n'
            '<div xmlns="http://www.w3.org/1999/xhtml" xmlns:xs="http://www.w3.org/2001/XMLSchema">\n'
            f'<ul id="treeRoot">\n{"".join(items)}</ul>\n</div>\n')


ANC, IMG = 'anc_illustrated', 'image_illustrated'
A = 'FRAD999_IR0001'

write('aid-root.html', toc([
    li(f'{A}_e0000001', 'Registres paroissiaux et d\'état civil', ANC, [
        li(f'{A}_G1', 'Exampleville', ANC, []),
        li(f'{A}_G2', 'Hauts-Exemples (Les)', ANC, []),
        li(f'{A}_G3', 'Sampleton', ANC, []),
    ]),
]))
write('aid-commune.html', toc([
    li(f'{A}_BMSEX', 'Baptêmes, mariages, sépultures', ANC, [
        li(f'{A}_EX1', 'Collection départementale', IMG),
        li(f'{A}_EX2', 'Collection communale', IMG),
    ]),
    li(f'{A}_NMDEX', 'Naissances, mariages, décès', ANC, [
        li(f'{A}_e0000100', 'Collection départementale', ANC, []),
        li(f'{A}_e0000200', 'Collection communale', ANC, []),
    ]),
    li(f'{A}_e0000300', 'Tables décennales (TD)', ANC, [
        li(f'{A}_e0000301', '1843-1853', IMG),
        li(f'{A}_e0000302', '1853-1863', IMG),
    ]),
]))


def collection(prefix):
    return toc([
        li(f'{A}_{prefix}10', 'Naissances', ANC, [
            li(f'{A}_{prefix}11', '1793-1806', IMG),
            li(f'{A}_{prefix}12', '1843-1852', IMG),
        ]),
        li(f'{A}_{prefix}20', 'Mariages', ANC, [li(f'{A}_{prefix}21', '1843-1852', IMG)]),
        li(f'{A}_{prefix}30', 'Décès', ANC, [li(f'{A}_{prefix}31', '1843-1852', IMG)]),
    ])


write('aid-departementale.html', collection('e00001'))
write('aid-communale.html', collection('e00002'))


def fragment(ident, title, ark, period=None, call_number=None, base=P64, note=''):
    rows = ''
    if call_number:
        rows += (f'<tr class="pl-tbl-lgn-unitid"><th scope="row" class="pl-tbl-th"><span class="pl-tbl-th-sp">Cotes extrêmes</span></th>'
                 f'<td class="pl-tbl-unitid">{call_number}</td></tr>')
    rows += (f'<tr class="pl-tbl-lgn-unittitle"><th scope="row" class="pl-tbl-th"><span class="pl-tbl-th-sp">Intitulé</span></th>'
             f'<td class="pl-tbl-unittitle">{title}</td></tr>')
    if period:
        rows += (f'<tr class="pl-tbl-lgn-unitdate"><th scope="row" class="pl-tbl-th"><span class="pl-tbl-th-sp">Dates extrêmes</span></th>'
                 f'<td class="pl-tbl-unitdate">{period}</td></tr>')
    dao = '' if ark is None else (
        '<div class="pl-pgd-sect pl-ead-dao"><div class="pl-pgd-medias-content"><a class="pl-pgd-medias-thumbnail" '
        'title="Cliquer sur cet aperçu" '
        f'href="{base}/ark:/99999/{ark}/f1?context=ead::{ident}" onclick="return eadWindow.getWindowManager().openImgViewer(this.href);">'
        f'<img border="0" src="{base}/ark:/99999/{ark}/f1.vignette" alt="Aperçu"/></a></div></div>')
    return ('<!DOCTYPE div\n  PUBLIC "-//W3C//DTD XHTML 1.0 Transitional//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd">\n'
            f'<div xmlns="http://www.w3.org/1999/xhtml" class="pl-pgd-subdoc"><div id="{ident}" class="pl-pgd-component">'
            '<div class="pl-pgd-cnt" id="pl-pgd-cnt"><div class="pl-pgd-c pl-ead-c pl-ead-att-level-file">'
            f'<table class="pl-pgd-fv pl-ead-did" summary="Identification">{rows}</table>{note}{dao}</div>'
            '<div class="pl-pgd-children pl-pgd-sect"/></div></div></div>\n')


write('fragment-bms-departementale.html', fragment(f'{A}_EX1', 'Collection départementale', 'rbmsexample01', '1743-1791',
                                                   note='<div class="pl-pgd-sect pl-ead-scopecontent"><p class="pl-ead-p">lacunes : 1747, 1752</p></div>'))
write('fragment-bms-communale.html', fragment(f'{A}_EX2', 'Collection communale', 'rbmsexample02', '1700-1760'))
write('fragment-nmd-departementale.html', fragment(f'{A}_e0000112', '<span class="pl-ead-unitdate">1843-1852</span>', 'rnmdexample01'))
write('fragment-nmd-communale.html', fragment(f'{A}_e0000212', '<span class="pl-ead-unitdate">1843-1852</span>', 'rnmdexample02'))
write('fragment-tables.html', fragment(f'{A}_e0000302', '1853-1863', 'rtdexample01'))
write('fragment-without-images.html', fragment(f'{A}_e0000112', '1843-1852', None))
write('manifest-p64-82.json', manifest(P64, 'rnmdexample01', 82))
write('manifest-p64-90.json', manifest(P64, 'rnmdexample02', 90))

# The military registers: collection, kind, class, recruitment office, volumes.
R = 'FRAD999_RM'
write('rm-root.html', toc([
    li(f'{R}_e0000002', 'Informations sur l\'instrument de recherche', aid=R),
    li(f'{R}_e0000020', 'Registres matricules militaires : collection des copies numériques', ANC, [
        li(f'{R}_tt1-1', 'Registres matricules numérisés', ANC, [], aid=R),
    ], aid=R),
]))
write('rm-collection.html', toc([
    li(f'{R}_tt2-1', 'Listes du contingent départemental de la garde nationale mobile', ANC, [
        li(f'{R}_tt3-1', 'Classe 1868', ANC, [], aid=R),
    ], aid=R),
    li(f'{R}_tt2-2', 'Registres matricules du recrutement de l\'armée', ANC, [
        li(f'{R}_tt3-40', 'Classe 1899', ANC, [], aid=R),
        li(f'{R}_tt3-41', 'Classe 1900', ANC, [], aid=R),
    ], aid=R),
]))
write('rm-class.html', toc([
    li(f'{R}_tt4-78', 'Bureau de recrutement de Exampleville', ANC, [
        li(f'{R}_de-173', 'Matricules 1-502 • R 9001', IMG, aid=R),
        li(f'{R}_de-174', 'Matricules 503-1004 • R 9002', IMG, aid=R),
    ], aid=R),
    li(f'{R}_tt4-77', 'Bureau de recrutement de Sampleton', ANC, [
        li(f'{R}_de-170', 'Matricules 1-500 • R 9003', IMG, aid=R),
    ], aid=R),
]))
write('rm-fragment.html', fragment(f'{R}_de-174', 'Matricules 503-1004', 'rrmexample01', '1900', 'R 9002', base=MAY))
