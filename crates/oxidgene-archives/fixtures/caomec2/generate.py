"""Writes the anonymized CAOMEC2 fixtures beside this script: `python3 generate.py`.

They have the markup of pages recorded from the civil-status search of the
Archives nationales d'outre-mer (a territory's search form, a search's
results, the image viewer), with fictitious territories' communes, years
and volume numbers; no recorded value is copied. The portal serves
ISO-8859-1, which the transports decode as UTF-8: each accented letter of
an act's label then reads as U+FFFD, as the results pages here write them.
"""
import pathlib

OUT = pathlib.Path(__file__).resolve().parent
LOST = '�'


def write(name, text):
    (OUT / name).write_text(text, encoding='utf-8')


HEAD = ('<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Transitional//EN" '
        '"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd">\n'
        '<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="fr" lang="fr">\n<head>\n'
        '<meta http-equiv="Content-Type" content="text/html; charset=ISO-8859-1" />\n'
        '<title>ANOM - Etat civil</title>\n<link rel="stylesheet" href="sdx.css" type="text/css" />\n'
        '</head>\n<body id="accueil">\n<form action="resultats.php">\n')
TAIL = '</form>\n</body>\n</html>\n'

COMMUNES = ['EXAMPLEVILLE', 'EXAMPLEVILLE (HOPITAL)', "L'EXEMPLE-SUR-MER", 'LE BOURG-EXEMPLE',
            'SAINT-EXEMPLE']
TYPES = [('AC_NA', 'Naissance'), ('AC_AF', 'Affranchissement'), ('AC_MA', 'Mariage'),
         ('AC_DE', 'D&eacute;c&egrave;s'), ('AC_SV', 'Sans vie'),
         ('TD_TS', 'Table d&eacute;cennale tous actes')]


def select(name, options, attributes=''):
    return (f'<select {attributes}name="{name}">\n'
            '  <option value="">&#160;</option>\n'
            + ''.join(f'  <option value="{value}">{label}</option>\n' for value, label in options)
            + '</select>\n')


def form():
    write('form.html', HEAD + (
        '<table class="rechercheBDI" id="formSimple">\n<tr><td>Territoire</td><td>\n'
        + select('territoire', [('EXEMPLE', 'Exemple'), ('AUTRE EXEMPLE', 'Autre exemple')])
        + '</td></tr>\n<tr><td>Commune</td><td>\n'
        + select('commune', [(c.replace("'", '&#039;'), c.replace("'", '&#039;')) for c in COMMUNES])
        + '</td></tr>\n<tr><td>Type d\'acte</td><td>\n' + select('typeacte', TYPES, 'tabindex="100" ')
        + '</td></tr>\n<tr><td>Ann&eacute;e</td><td>\n'
        '<input tabindex="100" name="annee" value="" size="5" class="annee" type="text" />\n'
        '</td></tr>\n</table>\n') + TAIL)


def results(name, total, rows, pages=1):
    text = (f'<div class="nBresultat"><b class="territoire">Exemple</b>, <strong id="results-nb">{total}'
            '</strong> r&eacute;sultat trouv&eacute;  <small class="time">(0,0050 s.)</small></div>\n')
    if not rows:
        text += ('<div id="page">\n<p class="nores">Il n\'y a pas de r&eacute;sultats pour votre '
                 'requ&ecirc;te</p></div>\n')
        write(name, HEAD + text + TAIL)
        return
    text += (f'<table class="navigation"><tr><td width="33%">Page <strong class="page">1</strong> de\n'
             f'                {pages}                </td></tr></table>\n'
             '<div id="page">\n<table align="center" cellpadding="0" cellspacing="1" class="liste">\n'
             '<tr class="critere"><td>&#160;</td><td class="nb">Acc&egrave;s</td>'
             '<td class="commune">Commune</td><td class="annee">Date</td><td class="acte">Type</td>'
             '<td class="theme">Th&egrave;me</td>\n')
    for index, (commune, year, act, code) in enumerate(rows, 1):
        # PHP escapes an apostrophe within the script's quotes.
        linked = commune.replace(' ', '%20').replace("'", "\\'")
        query = (f'territoire=EXEMPLE&amp;commune={linked}&amp;annee={year}'
                 + (f'&amp;typeacte={code}' if code else ''))
        parity = 'impair' if index % 2 else 'pair'
        text += (f'\t<tr onclick="window.location=\'osd.php?{query}\';" class="{parity}">\n'
                 f'    \t<td class="nb">{index}</td>\n'
                 '    \t<td align="center" class="valeur"><img src="images/fdNum.gif" alt="Voir les images" /></td>\n'
                 f'    \t<td class="commune">{commune.replace(chr(39), "&#039;")}</td>\n'
                 f'    \t<td class="annee">{year}</td>\n'
                 f'    \t<td class="acte">{act}</td>\n'
                 '\t\t\t\t\t\t<td class="theme"></td>\n\t\t\t</tr>\n')
    text += '</table>\n</div>\n'
    write(name, HEAD + text + TAIL)


DECES = f'D{LOST}c{LOST}s'


def viewer():
    strip = ''.join(
        f'\t\t\t\t\t<div id="thn{index}" class="thn{" sel" if index == 0 else ""}" title="Voir le document">\n'
        f'\t\t\t\t\t\t<img data-src="/caomec2/collection/EXEMPLE/900001/EXEMPLE_900001_{index + 1:04}_thn.jpg" alt="{index + 1}"/>\n'
        f'\t\t\t\t\t\t<div class="thn_id"><span>{index + 1}</span></div>\n\t\t\t\t\t</div>\n'
        for index in range(7))
    write('viewer.html', (
        '<!DOCTYPE html>\n<html lang="fr">\n<head>\n<meta charset="UTF-8">\n'
        '<title>ANOM, Etat Civil, R&eacute;sultats</title>\n</head>\n<body class="view">\n'
        '<div id="title">Exemple  EXAMPLEVILLE  1850</div>\n'
        '<form id="paging" action="#" method="GET">\n<div id="paging-info">\n'
        '<input id="iddoc" value="1" autocomplete="off"/>\n</div>\n</form>\n'
        '<div id="imgstrip">\n' + strip + '</div>\n'
        '<div id="osdview"></div>\n</body>\n</html>\n'))


form()
results('results-one.html', 1, [('EXAMPLEVILLE', 1850, 'Tous actes', None)])
results('results-year.html', 5, [
    ('SAINT-EXEMPLE', 1852, 'Naissance', 'AC_NA'),
    ('SAINT-EXEMPLE', 1852, 'Affranchissement', 'AC_AF'),
    ('SAINT-EXEMPLE', 1852, 'Mariage', 'AC_MA'),
    ('SAINT-EXEMPLE', 1852, DECES, 'AC_DE'),
    ('SAINT-EXEMPLE', 1852, 'Sans vie', 'AC_SV'),
])
results('results-several.html', 23, [
    ('EXAMPLEVILLE', 1717, 'Tous actes', None),
    ('EXAMPLEVILLE', 1718, 'Tous actes', None),
    ('EXAMPLEVILLE', 1728, DECES, 'AC_DE'),
    ('EXAMPLEVILLE', 1728, 'Tous actes', None),
], pages=2)
results('results-apostrophe.html', 1, [("L'EXEMPLE-SUR-MER", 1890, 'Tous actes', None)])
results('results-none.html', 0, [])
viewer()
