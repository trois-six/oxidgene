"""Writes the anonymized fixtures of the Gers portal beside this script: `python3 generate.py`.

They have the markup of pages recorded from the portal's modules (the
search form with its locality lists, which every answer repeats, the
results tables of the civil status, parish registers, tables, censuses
and tables of successions, a viewer's list of views, the bot-mitigation
redirect), with fictitious localities, parishes, call numbers and
identifiers; no recorded value is copied."""
import pathlib

OUT = pathlib.Path(__file__).resolve().parent


def write(name, text):
    (OUT / name).write_text(text, encoding='utf-8')


HEAD = ('<!doctype html>\n<html lang="fr">\n<head>\n    <meta charset="UTF-8">\n'
        '    <link rel="stylesheet" href="../../../css/styles.css">\n</head>\n<body>\n'
        '<div class="soustitre">Recherche</div>\n')
TAIL = '</body>\n</html>\n'

COMMUNES = ['Bourg-Exemple ', 'Exampleville', "L'Isle-Exemple", 'Sampleton']
FORMERS = ['Vieux-Exemple']


def select(name, everything, values, selected=None):
    options = ''.join(
        f'                                <option value="{value}"{" selected" if value == selected else ""}>'
        f'{value.strip()}</option>\n' for value in values)
    return (f'<select name="{name}" class="selectpicker" data-live-search="true" style="width:200px;" '
            f'onchange="submit()">\n                                <option value="all">{everything}</option>\n'
            f'{options}                        </select>\n')


def form(fields, boxes, selected):
    locality, former = fields
    text = ('<form name="etape1" role="form" method="POST" action="./">\n'
            '<div class="row ligne_recherche"><div class="col-xs-4 col-md-3 entete_recherche">Commune :</div>\n'
            + select(locality, 'Toutes les communes', COMMUNES, selected) + '</div>\n')
    if former:
        text += ('<div class="row ligne_recherche"><div class="col-xs-4 col-md-3 entete_recherche">'
                 'Ancienne commune :</div>\n' + select(former, 'Toutes les anciennes communes', FORMERS) + '</div>\n')
    text += ('De&nbsp;<input type="number" name="annee_d" class="form-control form_date" value=""  '
             'placeholder="Min 1793" >&nbsp;à&nbsp;\n<input type="number" name="annee_f" value="" '
             'class="form-control form_date" placeholder="Max 1932">\n')
    for box in boxes:
        text += (f'<input type="checkbox" class="custom-control-input chk_etat" id="{box}" name="{box}" checked>\n')
    text += ('<button type="submit" name="valider"  value="valider" class="btn-rougecd btn ">Rechercher</button>\n'
             '</form>\n')
    return text


def table(headings, rows):
    text = ('<div class="resultats">\n<div class="titre">Résultats de votre recherche :</div>\n'
            '<table id="myTable" style="width:100%" border="1" cellspacing="0" class="table tableau_td display">\n'
            '<thead>\n<tr class="entete">\n<!--<td>Type</td>-->\n'
            + ''.join(f'<td>{heading}</td>\n' for heading in headings) + '</tr>\n</thead>\n')
    for cells, link, images in rows:
        text += '<tr>\n' + ''.join(f'<td>{cell}</td>\n' for cell in cells)
        text += ('<td class="vision">\n'
                 f'<a target="_blank" href="../visu/?{link}"><img src="icone_matricule.png" border="0" width="30">\n'
                 f'</a>\n<br/><span style="font-size:0.8em;">{images} vues</span>\n</td>\n</tr>\n')
    return text + '</table>\n</div>\n'


def answer(name, fields, boxes, selected, headings, rows):
    write(name, HEAD + form(fields, boxes, selected) + table(headings, rows) + TAIL)


EC = (('lieu', 'ancienne'), ['chk_naissance', 'chk_pub_mariage', 'chk_mariage', 'chk_deces'])
EC_HEADINGS = ['Commune', 'Ancienne<br/>commune', 'Période', 'Cote', 'Contenu', 'Vues']

answer('ec-exampleville.html', *EC, 'Exampleville', EC_HEADINGS, [
    (['Exampleville', '', '1792-1842', '5 E 9001',
      'Naissances / Mariages / Décès <br>Les actes sont lacunaires pour la période 1792-1800.'],
     'id=9001&fichier=500100&lieu=Exampleville&annee=1792', 393),
    (['Exampleville', '', '1798-1842', '5 E 9002',
      'Mariages <br>Ce registre contient les actes de naissance et de décès du canton pour les ans VII et VIII.'],
     'id=9002&fichier=600200&lieu=Exampleville&annee=1798', 349),
    (['Exampleville', '', '1843-1862', '5 E 9003', 'Naissances / Publications de mariages / Mariages / Décès'],
     'id=9003&fichier=700300&lieu=Exampleville&annee=1843', 512),
])
answer('ec-none.html', *EC, None, EC_HEADINGS, [])
answer('ec-isle.html', *EC, "L'Isle-Exemple", EC_HEADINGS, [
    (["L'Isle-Exemple", '', '1793-1802', '5 E 9101', 'Naissances / Mariages / Décès'],
     "id=9101&fichier=800100&lieu=L%27Isle-Exemple&annee=1793", 120),
])
answer('ec-former.html', *EC, None, EC_HEADINGS, [
    (['Sampleton', 'Vieux-Exemple', '1793-1835', '5 E 9201', 'Naissances / Mariages / Décès'],
     'id=9201&fichier=810000&lieu=Sampleton+&annee=1793', 235),
])

answer('rp-bourg.html', ('lieu', 'ancienne'), ['chk_naissance', 'chk_mariage', 'chk_deces'], 'Bourg-Exemple ',
       ['Commune', 'Paroisse', 'Annexe', 'Période', 'Cote', 'Contenu', 'Vues'], [
           (['Bourg-Exemple', '', 'Saint-Exemple (Bourg-Exemple - annexe de Sampleton)', '1740-1748',
             '5 E 9301 (1)', 'Baptêmes / Mariages / Sépultures /'],
            'td=9301&fichier=80000&lieu=Bourg-Exemple&annee=1740', 20),
           (['Bourg-Exemple', 'Bourg-Exemple', '', '1740-1748', '5 E 9302 (1)',
             'Baptêmes / Mariages / Sépultures /'],
            'td=9302&fichier=90000&lieu=Bourg-Exemple&annee=1740', 53),
       ])

answer('td-exampleville.html', ('lieu', 'ancienne'), ['chk_naissance', 'chk_mariage', 'chk_deces'], 'Exampleville',
       ['Commune', 'Ancienne<br/>commune', 'Période', 'Cote', 'Contenu', 'Vues'], [
           (['Exampleville', '', '1802-1812', '5 E 9401',
             'Naissances / Mariages / Décès <br>Fonds du greffe du tribunal.'],
            'td=9401&fichier=12301&lieu=Exampleville&annee=1802', 19),
           (['Exampleville', '', '1802-1812', '5 E 9402',
             'Naissances / Mariages / Décès <br>Fonds de la préfecture.'],
            'td=9402&fichier=24569&lieu=Exampleville&annee=1802', 17),
       ])

answer('census-exampleville.html', ('LIEU', 'ANCIENNE'), [], 'Exampleville',
       ['Commune', 'Ancienne<br/>commune', 'Année', 'Cote', 'Contenu', 'Vues'], [
           (['Exampleville', '', '1836', '6 M 901', 'Recensement de la population : liste nominative.'],
            'rec=9501&fichier=22994&lieu=Exampleville&annee=1836', 4),
           (['Exampleville', '', '1841', '6 M 902', 'Recensement de la population : liste nominative.'],
            'rec=9502&fichier=22993&lieu=Exampleville&annee=1841', 4),
       ])

answer('successions-exampleville.html', ('lieu', None), [], 'Exampleville',
       ['Bureau', 'Période', 'Cote', 'Contenu', 'Vues'], [
           (['Exampleville', '1844-1864', 'Q 9601', '/'], 'sa=9601&fichier=20037&lieu=Exampleville&annee=1844', 198),
           (['Exampleville', '1864-1879', 'Q 9602', '/'], 'sa=9602&fichier=20036&lieu=Exampleville&annee=1864', 157),
       ])

write('viewer-listed.html', HEAD + (
    '<form method="get" action="./">\n<input type="hidden" name="rec" value="9501">\n'
    '<input type="hidden" name="lieu" value="Exampleville">\n<input type="hidden" name="annee" value="1836">\n'
    '<select class="selectpicker" data-live-search="true" id="fichier" name="fichier" placeholder="Autres vues" '
    'onChange="submit()" >\n<option selected value=\'22994\'>1 / 4</option><option value=\'37762\'>2 / 4</option>'
    '<option value=\'51730\'>3 / 4</option><option value=\'123544\'>4 / 4</option></select>\n</form>\n'
    '<img id="myContent" src="">\n') + TAIL)

write('challenge.html', '<html lang="en"><head></head><body><script>window.location.href=\'/redirect_'
      'AAAA0000====/archives_numerisees/portail/etats_civils/ec/recherche/\';</script><noscript>This website '
      'requires JS enabled and cookies</noscript></body></html>')
