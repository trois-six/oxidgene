"""Writes the anonymized Visualys fixtures beside this script: `python3 generate.py`.

They have the markup of pages recorded from the Côtes-d'Armor "salle
virtuelle" (the licence page, the alphabetical list of localities, a
locality's lots in their blocks, the military registers' search form and
its answers, a lot's sheets of numbered thumbnails), with fictitious localities, parishes, offices, call numbers
and identifiers; no recorded value is copied, and the pages' state fields
hold placeholders."""
import pathlib

OUT = pathlib.Path(__file__).resolve().parent


def write(name, text):
    (OUT / name).write_text(text, encoding='utf-8')


HEAD = ('<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN">\n<HTML>\n  <HEAD>\n'
        '\t<TITLE id="headTitle">Registres paroissiaux et d\'état civil</TITLE>\n'
        '\t<LINK REL="stylesheet" TYPE="text/css" HREF="../slv.css?v=20210722">\n</HEAD>\n<BODY>\n'
        '<DIV CLASS="v2Bandeau" ID="haut">\n\t<span id="ctlHaut_lblTitre">Salle virtuelle</span>\n</DIV>\n')
TAIL = '</BODY>\n</HTML>\n'


def licence():
    write('licence.html', HEAD + (
        '<form method="post" action="./licence.aspx" id="frmLicence">\n'
        '<input type="hidden" name="__VIEWSTATE" id="__VIEWSTATE" value="STATE0" />\n'
        '\t<DIV CLASS="licence">\n\t\t<H1>Annexe 1</H1>\n'
        '<P>Le réutilisateur est tenu d&apos;indiquer la source de l&apos;information.</P>\n'
        '\t\t<DIV CLASS="ButtonsAutosize">\n'
        '\t\t\t<input type="submit" name="btnAccepter" value="J&#39;accepte ces conditions" id="btnAccepter" />\n'
        '\t\t</DIV>\n\t</DIV>\n</form>\n') + TAIL)


def letters(selected):
    return '\t<DIV CLASS="Abecedaire">\n\t\t' + ''.join(
        f'<A CLASS="Letter  Small{" SelectedLetter" if letter == selected else ""}" '
        f'HREF="javascript:lettre(\'{letter}\')">{letter}</A>' for letter in 'ABEMSZ') + '\n\t</DIV>\n'


def locality_row(ident, name, parish, period):
    second = f'<A HREF="javascript:lot(\'{ident}\')">{parish}</A>' if parish else \
        f'<A HREF="javascript:lot(\'{ident}\')"></A>'
    return (f'\n\t\t<TR>\n\t\t\t<TD CLASS="Icon"><A HREF="javascript:voir(\'{name}\')" TITLE="Informations générales">'
            '<IMG SRC="images/info.gif" BORDER="0" WIDTH="16" HEIGHT="16"></A></TD>\n'
            f'\t\t\t<TD><A HREF="javascript:lot(\'{ident}\')">{name}</A>&nbsp;<SPAN CLASS="VoirAussi">&nbsp;</SPAN></TD>\n'
            f'\t\t\t<TD>{second}&nbsp;</TD>\n\t\t\t<TD>{period}</TD>\n\t\t</TR>\n')


def locality_list(name, letter, rows):
    write(name, HEAD + (
        '<SCRIPT LANGUAGE="javascript">\nfunction lettre(valeur)\n'
        '{window.location.href="commune.aspx?lettre="+valeur;}\nfunction lot(id)\n'
        '{window.location.href="plage.aspx?id="+id;}\n</SCRIPT>\n'
        '<TABLE BORDER="0" CELLPADDING="0" CELLSPACING="0" CLASS="v2Contenu">\n<TR>\n<TD CLASS="v2Contenu">\n'
        '\t<DIV CLASS="Title">Recherche alphabétique</DIV>\n' + letters(letter) +
        '\t<DIV CLASS="TMessage">\n\t<TABLE BORDER="0" CELLPADDING="0" CELLSPACING="0" CLASS="v2Liste">\n'
        '\t<THEAD>\n\t\t<TR>\n\t\t\t<TD WIDTH="32">&nbsp;</TD>\n\t\t\t<TD>Paroisse</TD>\n'
        '\t\t\t<TD WIDTH="260">&nbsp;</TD>\n\t\t\t<TD WIDTH="120">Dates extrêmes</TD>\n\t\t</TR>\n\t</THEAD>\n'
        + ''.join(locality_row(*row) for row in rows) +
        '\t</TABLE>\n\t</DIV>\n</TD>\n</TR></TABLE>\n') + TAIL)


def block(code, number, title, opened, lots):
    icon = 'tree_moins' if opened else 'tree_plus'
    text = (f'\t<tr id="TableRow{code}">\n\t<TD colspan="10" CLASS="OpenClose">\n'
            f'\t\t\t<DIV CLASS="OpenCloseIcon" ONCLICK="openBloc(\'{number}\')">\n'
            f'\t\t\t\t<img src="images/{icon}.gif" id="Image{code}" border="0" />\n\t\t\t</DIV>\n'
            f'\t\t\t<DIV CLASS="OpenCloseMessage" ONCLICK="openBloc(\'{number}\')">\n'
            f'\t\t\t\t{title}\n\t\t\t</DIV>\n\t\t</TD>\n</tr>\n')
    if not opened:
        return text
    text += ('\t<TR CLASS="Header">\n\t\t<TD WIDTH="30">&nbsp;</TD>\n\t\t<TD WIDTH="30">&nbsp;</TD>\n'
             '\t\t<TD ALIGN="right">Lot</TD>\n\t\t<TD WIDTH="30">&nbsp;</TD>\n\t\t<TD>&nbsp;</TD>\n'
             '\t\t<TD>Début</TD>\n\t\t<TD>Fin</TD>\n\t\t<TD>Acte</TD>\n'
             '\t\t<TD ALIGN="right">Nombre&nbsp;d\'images</TD>\n\t\t<TD>&nbsp;</TD>\n\t</TR>\n')
    for index, (ident, first, last, act, images) in enumerate(lots, 1):
        text += ('\t<TR>\n\t\t<TD ALIGN="center"><IMG SRC="images/transp.gif" BORDER="0"></TD>\n'
                 '\t\t<TD ALIGN="center"><IMG SRC="images/transp.gif" BORDER="0" WIDTH="10" HEIGHT="10"></TD>\n'
                 f'\t\t<TD ALIGN="right">{index}</TD>\n\t\t<TD>&nbsp;</TD>\n\t\t<TD>&nbsp;</TD>\n'
                 f'\t\t<TD><A HREF="javascript:mini(\'{ident}\',\'1\',\'1\')">{first}</A></TD>\n'
                 f'\t\t<TD><A HREF="javascript:mini(\'{ident}\',\'{images}\',\'{images}\')">{last}</A></TD>\n'
                 f'\t\t<TD>{act}</TD>\n\t\t<TD  ALIGN="right">{images}</TD>\n\t\t<TD>&nbsp;</TD>\n\t</TR>\n')
    return text


PARISH_LOTS = [
    ('900000000000101', 1641, 1681, 'BMS', 611),
    ('900000000000102', 1673, 1673, 'M', 33),
    ('900000000000103', 1682, 1692, 'BMS', 264),
]
CIVIL_LOTS = [
    ('900000000000201', 1793, 1802, 'N', 248),
    ('900000000000202', 1793, 1812, 'M', 190),
    ('900000000000203', 1793, 1812, 'D', 205),
    ('900000000000204', 1793, 1802, 'TD', 12),
    ('900000000000205', 1802, 1812, 'TD', 14),
]


def lots(name, locality, ident, parish_open, civil_open, parish=True):
    text = HEAD + (
        '<SCRIPT LANGUAGE="javascript">\nfunction openBloc(p)\n'
        f'{{window.location.href="plage.aspx?id={ident}&r="+p;}}\n</SCRIPT>\n'
        '<TABLE BORDER="0" CELLPADDING="0" CELLSPACING="0" CLASS="v2Contenu">\n<TR>\n<TD CLASS="v2Contenu">\n'
        '\t<DIV CLASS="Title">Lots d\'images</DIV>\n\t<DIV CLASS="TMessage">\n'
        '\t\tVous trouverez ci-dessous la liste des actes disponibles de <span id="LabelMessage"> la paroisse de '
        f'<span class="Important">{locality}</span></span>, classés par période.\n\t</DIV>\n'
        '\t<DIV CLASS="TMessage">\n\t<TABLE BORDER="0" CELLPADDING="0" CELLSPACING="0" CLASS="v2Liste">\n')
    if parish:
        text += block('RP', 0, 'Registres paroissiaux<span id="LabelDateRP"> de 1641 à 1792</span>',
                      parish_open, PARISH_LOTS)
    text += block('EC', 1, 'Registres d\'état civil<span id="LabelDateEC"> de 1793 à 1812</span>',
                  civil_open, CIVIL_LOTS)
    text += ('\t</TABLE>\n\t</DIV>\n\t<DIV CLASS="TMessage">\n\tB : baptême, M : mariage, S : sépulture, '
             'N : Naissance, D : Décès, TD : Table décennale.\n\t</DIV>\n</TD>\n</TR></TABLE>\n') + TAIL
    write(name, text)


def select(name, values):
    return (f'<select name="{name}" id="{name}" class="Textbox S">\n'
            '\t<option selected="selected" value=""></option>\n' +
            ''.join(f'\t<option value="{value}">{value}</option>\n' for value in values) + '</select>\n')


OFFICES = ['Exampleville', 'Sampleton', 'Sampleton Exemple']


def military(name, volumes):
    text = HEAD + (
        '<SCRIPT LANGUAGE="javascript">\nfunction mini(plage)\n{\n\tvar vUrl="planche.aspx?id="+plage;\n'
        '\twindow.location.href=vUrl;\n}\n</SCRIPT>\n'
        '<form method="post" action="./commune.aspx?lettre=*" id="frmRecherche">\n'
        '<div class="aspNetHidden">\n'
        '<input type="hidden" name="__VIEWSTATE" id="__VIEWSTATE" value="STATE/1+2=" />\n</div>\n'
        '<div class="aspNetHidden">\n'
        '\t<input type="hidden" name="__VIEWSTATEGENERATOR" id="__VIEWSTATEGENERATOR" value="0000AAAA" />\n'
        '\t<input type="hidden" name="__EVENTVALIDATION" id="__EVENTVALIDATION" value="CHECK/3+4=" />\n</div>\n'
        '<TABLE BORDER="0" CELLPADDING="0" CELLSPACING="0" CLASS="v2Contenu">\n<TR>\n<TD CLASS="v2Contenu">\n'
        '\t<DIV CLASS="Title">Recherche</DIV>\n'
        '\t\t\t<LABEL CLASS="M" FOR="lstAnnee1">Année :</LABEL>' + select('lstAnnee1', range(1867, 1922)) +
        '\t\t\t<LABEL CLASS="M" FOR="lstAnnee2">Jusqu\'à :</LABEL>' + select('lstAnnee2', range(1867, 1922)) +
        '\t\t\t<LABEL CLASS="M" FOR="lstBureau">Bureau :</LABEL>' + select('lstBureau', OFFICES) +
        '\t\t\t<LABEL CLASS="M" FOR="lstRegistre">Type :</LABEL>' + select('lstRegistre', ['Registre matricule', 'Table']) +
        '\t\t\t<input type="submit" name="btnFind" value="Rechercher" id="btnFind" />\n')
    if volumes is not None:
        text += ('\t<DIV CLASS="TMessage">Sélectionnez l\'un des lots d\'images ci-dessous.</DIV>\n'
                 '\t<DIV CLASS="TMessage">\n\t<TABLE BORDER="0" CELLPADDING="0" CELLSPACING="0" CLASS="v2Liste">\n'
                 '\t<THEAD>\n\t<TR>\n\t\t<TD WIDTH="20">&nbsp;</TD>\n\t\t<TD>Bureau</TD>\n\t\t<TD>Année</TD>\n'
                 '\t\t<TD>Cote</TD>\n\t\t<TD>Registre</TD>\n\t</TR>\n\t</THEAD>\n')
        for ident, office, year, call_number in volumes:
            text += ('\t\t<TR>\n\t\t\t<TD ALIGN="center"><IMG SRC="images/transp.gif" BORDER="0" WIDTH="16" HEIGHT="16"></TD>\n'
                     f'\t\t\t<TD>{office}</TD>\n\t\t\t<TD>{year}</TD>\n\t\t\t<TD>{call_number}</TD>\n'
                     f'\t\t\t<TD><A HREF="javascript:mini(\'{ident}\')">Registre matricule</A></TD>\n\t\t</TR>\n')
        text += '\t</TABLE>\n\t</DIV>\n'
    text += '</TD>\n</TR></TABLE>\n</form>\n' + TAIL
    write(name, text)


def sheet(name, lot, page, pages, first, last):
    """A sheet of a lot's thumbnails (`planche.aspx`), views `first` to `last`."""
    text = HEAD + (
        '<SCRIPT LANGUAGE="javascript">\nfunction ouvrir(nom)\n'
        '{window.location.href="consult.aspx?image="+nom;}\n</SCRIPT>\n'
        '<TABLE BORDER="0" CELLPADDING="0" CELLSPACING="0" CLASS="PageNavigation">\n<TR>\n'
        '\t<TD CLASS="Page">Pages :</TD>\n\t<TD CLASS="Select"><SELECT NAME="ListePage" ID="ListePage">'
        + ''.join(f'<OPTION VALUE="{n}"{" SELECTED" if n == page else ""}>{n}</OPTION>'
                  for n in range(1, pages + 1)) +
        f'</SELECT></TD>\n\t<TD CLASS="Label"><span id="LabelPage">&nbsp;/&nbsp;{pages}</span></TD>\n'
        '</TR>\n</TABLE>\n<TABLE BORDER="0" CELLPADDING="0" CELLSPACING="0">\n<TR>\n')
    for view in range(first, last + 1):
        image = f'9100{lot[-3:]}{view:08}'
        text += ('\t\t<td valign="top">\n\t\t<TABLE BORDER="0" CELLPADDING="0" CELLSPACING="0" HEIGHT="148" '
                 'WIDTH="180"><TR><TD VALIGN="top">\n\t\t\t<TABLE BORDER="0" CELLPADDING="0" CELLSPACING="2">\n'
                 '\t\t\t<TR>\n\t\t\t\t<TD COLSPAN="2" WIDTH="140" HEIGHT="90" ALIGN="center" VALIGN="center" '
                 f'CLASS="FondGris"><A CLASS="ImgC" HREF="javascript:ouvrir(\'{image}\')"><IMG BORDER="0" '
                 f'SRC="rg_ec//disk00/EXEMPLE/tn/tnEXEMPLE_{view:04}.jpg" '
                 f'ONMOUSEMOVE="zoomOn(event,this,\'{image}\')" ONMOUSEOUT="zoomOff()" HEIGHT="90"></A></TD>\n'
                 '\t\t\t</TR>\n\t\t\t<TR>\n'
                 f'\t\t\t\t<TD VALIGN="top" CLASS="MiniNum" WIDTH="10">{view}.</TD>\n'
                 '\t\t\t\t<TD VALIGN="top" CLASS="MiniDate" ALIGN="right">&nbsp;</TD>\n'
                 '\t\t\t</TR>\n\t\t\t</TABLE>\n\t\t</TD></TR></TABLE>\n\t\t</td>\n')
    text += '</TR>\n</TABLE>\n' + TAIL
    write(name, text)


licence()
locality_list('list-b.html', 'B', [
    ('900000000000011', 'Bourg (Le)', '', '1641 - 1922'),
    ('900000000000012', 'Bourgville', '', '1700 - 1922'),
])
locality_list('list-e.html', 'E', [
    ('900000000000021', 'Exampleville', '', '1600 - 1922'),
    ('900000000000022', 'Exampleville', 'Saint-Exemple', '1650 - 1792'),
    ('900000000000023', 'Exampleville', 'Hospice', '1700 - 1792'),
    ('900000000000024', 'Exemple-sur-Mer', '', '1670 - 1922'),
])
locality_list('list-s.html', 'S', [
    ('900000000000031', 'Sampleton', 'Saint-Exemple', '1650 - 1792'),
    ('900000000000032', 'Sampleton', 'Hospice', '1700 - 1792'),
])
lots('lots-closed.html', 'Bourg (Le)', '900000000000011', False, False)
lots('lots-parish.html', 'Bourg (Le)', '900000000000011', True, False)
lots('lots-civil.html', 'Bourg (Le)', '900000000000011', False, True)
lots('lots-both.html', 'Bourg (Le)', '900000000000011', True, True)
lots('lots-civil-only.html', 'Exampleville', '900000000000021', False, True, parish=False)
military('military-form.html', None)
military('military-results.html', [
    ('900000000000301', 'Exampleville', 1900, '01R9001'),
    ('900000000000302', 'Exampleville', 1900, '01R9002'),
    ('900000000000303', 'Sampleton', 1900, '01R9010'),
])
military('military-none.html', [])
# The second sheet of the births lot 900000000000201 (248 views), and the
# first of a military volume.
sheet('sheet-births-2.html', '900000000000201', 2, 11, 25, 48)
sheet('sheet-military-1.html', '900000000000302', 1, 3, 1, 24)
