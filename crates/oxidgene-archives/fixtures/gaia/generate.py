"""Writes the anonymized GAIA fixtures beside this script: `python3 generate.py`.

They have the markup of pages recorded from GAIA 9 portals (the wizard's
lists, the year form, the search's answers, the viewer), with fictitious
localities, parishes, call numbers and identifiers; no recorded value is
copied. Like the portals' pages they are ISO-8859-1, except the labels of
the two-level portal, whose database text is UTF-8 inside a Latin-1 page."""
import pathlib

OUT = pathlib.Path(__file__).resolve().parent


def write(name, *parts):
    """Each part is text, encoded as Latin-1, or bytes, written as given."""
    data = b''.join(part if isinstance(part, bytes) else part.encode('latin-1') for part in parts)
    (OUT / name).write_bytes(data)


def head(base, theme, step):
    return ('<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN">\n<HTML><HEAD>\n'
            '<meta http-equiv="Content-Type" content="text/html; charset=iso-8859-1" />\n'
            '<title>GAIA 9 : moteur de recherche - 9.4.8</title>\n</HEAD><body>\n'
            '<div id="content" class="main_contener">\n<script type="text/javascript">\n'
            '    function rechercher() {\n'
            f'        $("#resultat").load("{base}/index.php/rechercheTheme/requeteConstructor/{theme}/{step}/T/0/0", '
            "{forcepost: 'essai'}, function(data){$.unblockUI();});\n    }\n</script>\n"
            '<div id="requete" class="centrage">\nRappel de votre requête :<br/><br/>\nETAT CIVIL<br/><br/><ul></ul>\n</div>\n')


TAIL = '<div id="resultat"></div>\n</body>\n</HTML>\n'


def screen(title):
    return f'<hr/>\n<br/><div class="centrage"><span class="titre_ecran">{title}</span><br/><br/></div>\n'


def letters(base, theme):
    return ('<center><table><tr><td class="abcedaire">' + ''.join(
        f'<a class="color_alphabet" href="{base}/index.php/rechercheTheme/requeteConstructor/{theme}/1/R/{letter}/0">'
        f'{letter}&nbsp;</a>\n' for letter in 'ABCDEL') + '</td></tr>')


def link(base, theme, step, ident, label, note='', raw=None):
    """A list choice; `raw` gives the label as bytes (UTF-8 database text)."""
    parts = [f'<tr><td><div align="left">\n<a class="color_liens" href="{base}/index.php/rechercheTheme/'
             f'requeteConstructor/{theme}/{step}/A/{ident}/']
    parts += [raw if raw else label, '">', raw if raw else label,
              f'&nbsp;&nbsp;&nbsp;&nbsp;<em>{note}</em></a><br/>\n</div></td></tr>\n']
    return parts


def skip_form(base, theme, step):
    return (f'<div class="centrage"><div class="formulaire">\n<form method="POST" action="{base}/index.php/'
            f'rechercheTheme/requeteConstructor/{theme}/{step}/F/0/0">\n<div class="centrage">'
            '<input type="submit" value="Rechercher" class="bouton_84px_type1"/></div>\n</form>\n</div>\n')


def list_page(name, base, theme, step, title, choices, with_letters=True, skip=False):
    parts = [head(base, theme, step), screen(title), '</div>\n']
    parts.append(letters(base, theme) if with_letters else '<center><table>')
    parts.append('<tr><td height="5">&nbsp;</td></tr>\n')
    for choice in choices:
        parts += link(base, theme, step, *choice)
    parts.append('</table></center>\n')
    if skip:
        parts.append(skip_form(base, theme, step + 1))
    parts.append(TAIL)
    write(name, *parts)


def dates_page(name, base, theme, step):
    write(name, head(base, theme, step), screen('Dates'), '</div>\n<div class="centrage"><div class="formulaire">\n'
          f'<form method="POST" action="{base}/index.php/rechercheTheme/requeteConstructor/{theme}/{step}/A/0/0">\n'
          'Sélectionnez une ou plusieurs années :\n<select id="typeDate" name="typeDate">\n'
          '   <option value="inter">intervalle</option>\n   <option value="simple" select>simple</option>\n'
          '</select>\nDe <input type="text" name="dateDeb" id="dateDeb"/>&nbsp;&agrave; '
          '<input type="text" name="dateFin" id="dateFin"/>\n'
          'Année exacte <input type="text" id="dateSimple" name="dateSimple"/>\n'
          '<input type="submit" value="Rechercher" class="bouton_84px_type1"/>\n</form>\n</div></div>\n', TAIL)


def submit_page(name, base, theme, step):
    write(name, head(base, theme, step), '<br/>\n'
          f'<form id="formrechTheme" method="POST" action="{base}/index.php/rechercheTheme/requeteConstructor/'
          f'{theme}/{step}/T/0/0">\n<script type="text/javascript">\n   $(document).ready(function() {{\n'
          "      $('#formrechTheme').submit();\n   });\n</script>\n</form>\n</div>\n", TAIL)


SCRIPT = ('<script language="Javascript"  type="text/javascript" >\n<!--\nfunction precharger(url, id){\n'
          "  tab_images['img_'+id]=new Image();\n}\n//-->\n</script>\n")


def count_cell(total):
    text = {0: ' Aucune réponse', 1: '1 réponse'}.get(total, f'{total} réponses')
    return ('<table class="t bt" width="99.5%">\n\t<tr>\n\t\t<td align="left" style="padding-left:1%;">LISTE DES '
            f'REPONSES</td>\n\t\t<td align="right" style="padding-right:2%;">{text}</td>\n\t</tr>\n</table>\n'
            '\t\t<a name="truc"></a>\n')


def row(base, index, unit, hierarchy, title, cote, period, images=True):
    path = f'{hierarchy}:{unit}'
    clip = (f"<BR/><a  href=\"#\" onClick=\"window.open('{base}/index.php/docnumViewer/calculHierarchieDocNum/"
            f"{unit}/{path}/'+screen.height+'/'+screen.width,'','width='+screen.width+',resizable=yes');\">"
            '<img style="float:left;" alt="Document numérique" title="Document numérique" '
            f'src="{base}/application/views/interface/moteur/icone_trombone.png"/></a>') if images else '<BR/>'
    return (f'\t\t\t\t{clip}<img style="float:left;" alt="Cote" title="Document" src="{base}/application/views/'
            'interface/moteur/icone_doc.gif"/><div class="reponsemot"><span style="position:relative;top:0.3em;'
            f'display: block;"> <a id="openDetail{index}" onClick="detailNotice(0,{index},\'{path}\',{unit},'
            f'{index},0);" href="javascript:void(0)">{title}</a>&nbsp;&nbsp;<img src="{base}/application/views/'
            f'images/picopanier.gif" onclick="ajouterPanier({unit},null,\'{path}\');"/></span><span '
            'class="reponsemot" style="position:relative;top:1em !important; top: 0.8em;">'
            f'<span>{cote}</span><span style="position:absolute;left:30em;width:100px;">{period}</span></span>'
            '<br/><br/><br/></div><div class="spacer" style="height:0.5px;">&nbsp;</div>\n')


def pager(base, offsets):
    return ''.join(
        '<a href="#truc"><span class="paginationTag" style="cursor:pointer" onClick="$(\'#resultat\').load(\''
        f'{base}/index.php/rechercheTheme/paginer/{offset}\', function(data){{$.unblockUI();}});">{number}'
        '</span></a>&nbsp;' for number, offset in offsets)


def answer(name, base, total, rows, offsets=()):
    body = [SCRIPT, count_cell(total), pager(base, offsets)]
    body += [row(base, index, *values) for index, values in enumerate(rows)]
    if total == 0:
        body.append('\t\t<P align="center"> Aucune réponse à votre recherche</P>\n')
    body.append('\t\t<BR />\n<br/>\n')
    write(name, *body)


# ------------------------------------------------- one level, article suffix

MDR = '/mdr'
list_page('list-e.html', MDR, 1, 1, 'Choix de la commune ou du canton', [
    (900101, 'ERMITE (L\')'),
    (900102, 'EXAMPLEVILLE'),
    (900103, 'EXAMPLEVILLE, paroisse Saint-Exemple'),
    (900104, 'EXAMPLEVILLE, Eglise protestante'),
    (900105, 'EXEMPLE-SUR-MER [jusqu\'en 1789 et à partir de 1926]'),
    (900106, 'ÉTANG-EXEMPLE'),
    (900107, 'EXEMPLE-LE-HAUT (après 1793)'),
    (900108, 'EXEMPLE-LE-HAUT, paroisse Saint-Exemple'),
])
list_page('types.html', MDR, 1, 2, 'Types de registres', [
    (900201, 'baptêmes'), (900202, 'naissances'), (900203, 'publications des mariages'),
    (900204, 'mariages'), (900205, 'sépultures'), (900206, 'décès'), (900207, 'tables décennales'),
], with_letters=False)
dates_page('dates.html', MDR, 1, 3)
submit_page('dated.html', MDR, 1, 3)
answer('results-one.html', MDR, 1, [
    (900301, '900001:900102:900202', 'EXAMPLEVILLE. Naissances. ', '9NUM/4E1', 'An XI-1872'),
])
answer('results-several.html', MDR, 3, [
    (900302, '900001:900102:900202', 'EXAMPLEVILLE. Naissances. ', '9NUM/4E1', 'An XI-1872'),
    (900303, '900001:900102:900202',
     'EXAMPLEVILLE. Naissances. (Registre de Saint-Exemple contenant aussi les mariages)', '9NUM1/4E2', '1840-1872'),
    (900304, '900001:900102:900202', 'EXAMPLEVILLE. Naissances. ', '4E3', '1850-1860', False),
])
answer('results-none.html', MDR, 0, [])
# A parish's own registers, which its commune's entry does not list.
answer('results-parish.html', MDR, 1, [
    (900305, '900001:900103:900201', 'BMS', '9E99/1', '1740-1760'),
])
write('maintenance.html', '<html><body><h1>Site en maintenance</h1></body></html>\n')

# --------------------------------------------- census: no year form, pages

def years(first, last, step):
    return list(range(first, last + 1, step))


census = [(900400 + index, '900002:900401', f'Liste nominative {year}', f'9NUM6M1/{index}', str(year))
          for index, year in enumerate(years(1801, 1845, 1))]
list_page('census-list.html', MDR, 2, 1, 'COMMUNE', [(900401, 'Exampleville', '(après 1793)')])
submit_page('census-commune.html', MDR, 2, 2)
answer('census-page-0.html', MDR, 45, census[:20], [(2, 20), (3, 40)])
answer('census-page-1.html', MDR, 45, census[20:40], [(1, 0), (3, 40)])
answer('census-page-2.html', MDR, 45, census[40:], [(1, 0), (2, 20)])

# ------------------------------------ military registers: no localities

list_page('kinds.html', MDR, 3, 1, 'Bureaux de recrutement', [
    (900501, 'Tables alphabétiques par année de recrutement (classe)'),
    (900502, 'Registres par année de recrutement (classe)'),
], with_letters=False)
dates_page('class-dates.html', MDR, 3, 2)
submit_page('class-dated.html', MDR, 3, 3)
answer('military.html', MDR, 4, [
    (900601, '900003:900502', 'Exampleville, matricules n° 1-496.', 'R9050', '1890'),
    (900602, '900003:900502', 'Exampleville, matricules n° 497-1013.', 'R9051', '1890'),
    (900603, '900003:900502', 'Autreville, matricules n° 1-498.', 'R9053', '1890'),
    (900604, '900003:900502', 'Autreville, table alphabétique.', 'R9054', '1890', False),
])

# ----------------------- conscription lists: a list of classes, no skip

list_page('classes.html', MDR, 31, 1, 'Choisir une classe', [
    (900901, 'Classe 1816'), (900902, 'Classe 1817'),
], with_letters=False)
submit_page('class-chosen.html', MDR, 31, 2)
# A class's lists are drawn the year after it.
answer('class-lists.html', MDR, 2, [
    (900911, '900005:900901', 'Liste du contingent.', '9R234', '1817'),
    (900912, '900005:900901', "Listes du tirage au sort de l'arrondissement d'Exampleville par canton.",
     '9R235', '1817'),
])

# ------------------------- no kinds, plain localities with a department

list_page('list-l.html', MDR, 14, 1, 'Choisissez un lieu', [
    (900701, 'La Ville-Exemple (Département)'),
    (900702, 'Lavalle-Exemple (Département)'),
])
dates_page('dates-14.html', MDR, 14, 4)
submit_page('dated-14.html', MDR, 14, 4)
answer('results-titles.html', MDR, 3, [
    (900801, '900004:900005:900701', 'Tables des naissances, mariages, décès de La Ville-Exemple. 1843-1852', '', ''),
    (900802, '900004:900006:900701', 'Naissances, mariages, décès.', '6E99/8', '1849-1860', False),
    (900803, '900004:900007:900701', 'Naissances, mariages, décès.', '5MI999', '1849-1860'),
])

# ------------------------------ two levels, UTF-8 labels in a Latin-1 page

AUDE = '/mdr_aude'
list_page('two-level-list.html', AUDE, 1, 1, 'Choix des communes', [(901001, 'EXAMPLEVILLE')])
list_page('categories.html', AUDE, 1, 2, 'Choix du registre', [
    (901101, '', '', 'Registres paroissiaux (avant 1793)'.encode()),
    (901102, '', '', "Registres d'état-civil".encode()),
    (901103, '', '', 'Tables décennales communales'.encode()),
], with_letters=False, skip=True)
list_page('acts.html', AUDE, 1, 3, "Choix de l'acte", [
    (901201, '', '', 'Naissances'.encode()),
    (901202, '', '', 'Mariages'.encode()),
    (901203, '', '', 'Décès'.encode()),
], with_letters=False, skip=True)
dates_page('two-level-dates.html', AUDE, 1, 5)
submit_page('two-level-dated.html', AUDE, 1, 5)
before, after = row(AUDE, 0, 901301, '901000:901001:901102:901202', 'TITLE', '99NUM/5E9/15',
                    '1846-1855').split('TITLE')
write('two-level-one.html', SCRIPT, count_cell(1), before,
      'Exampleville. Actes de naissance, mariage, décès.'.encode(), after)
write('two-level-tables.html', SCRIPT, count_cell(1), row(
    AUDE, 0, 901302, '901000:901001:901103:901204', 'Exampleville. Tables des naissances, mariages, décès.',
    '99NUM/5E9/20', '1802-1892'))

# ------------------------------- census by year links, two-level portal

list_page('census-years-list.html', AUDE, 3, 1, 'Choix des communes', [(901400, 'EXAMPLEVILLE')])
list_page('years.html', AUDE, 3, 2, 'Choix de la date', [
    (901401, '1836'), (901402, '1846'), (901403, '1851'),
], with_letters=False, skip=True)
submit_page('year-dated.html', AUDE, 3, 3)
answer('year-one.html', AUDE, 1, [
    (901501, '901005:901006:901402', 'EXAMPLEVILLE 1846', '', ''),
])

# ----------------------------------------------------------------- viewer

DOC = ('{"typeMedia":"image","src":"ZXhlbXBsZQ","chemin":"Exemple@EXEMPLE_0001.jpg","ressourceCode":"w001",'
       '"ressourceVignette":"v001","titre":"","description":" <b>Document 9NUM\\/4E1<\\/b>","cote":"9NUM\\/4E1",'
       '"dateDebut":"1850","dateFin":"","idUd":"900301","cheminHierarchie":"900001:900102:900202:900301",'
       '"folio":"1","preNum":"vue 1"}')
write('viewer.html', '<!DOCTYPE HTML>\n<HTML><HEAD><title>GAIA 9 : moteur de recherche</title></HEAD><body>\n'
      '<canvas id="render"></canvas>\n<script type="text/javascript">\n    $(document).ready(function() {\n'
      f'                    main({{\n                        docs : [{",".join([DOC] * 3)}],\n'
      '                        numPage : 1,\n                    });\n    });\n</script>\n</body></HTML>\n')
