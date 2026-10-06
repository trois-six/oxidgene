"""Writes the anonymized THOT fixtures beside this script: `python3 generate.py`.

They have the markup of the answers recorded from the portals, with
fictitious localities, call numbers, identifiers and ARK names; no recorded
value is copied, and only what the adapter reads is kept. The portals serve
windows-1252, which the transports read as UTF-8: every accented letter of
a fixture is the U+FFFD a transport hands the adapter."""
import pathlib

OUT = pathlib.Path(__file__).resolve().parent
IV = '/thot_internet'
IV_ORIGIN = 'https://archives-en-ligne.ille-et-vilaine.fr'
CORSE = '/Internet_THOT'


def write(name, text):
    """As a transport reads the windows-1252 page: accented letters lost."""
    lossy = text.encode('cp1252').decode('utf-8', errors='replace')
    (OUT / name).write_text(lossy, encoding='utf-8')


# ------------------------------------------------------------- the session

write('droite.html', f'''<!DOCTYPE html>
<html lang="fr"><head><script type="text/javascript">
\t\t\tfunction initPage() {{
\t\t\t\tanimateLoadBar();
\t\t\t\tthis.location = "{IV}/FrmAccueilDroite.asp?checkCookie=20000101000000";
\t\t\t}}
</script></head><body onload="javascript: initPage();">Chargement en cours...</body></html>
''')

write('checked.html', '''<!DOCTYPE html>
<html lang="fr"><head><script type="text/javascript">
\t\t\twindow.open( "FrmSommaireFrame.asp", "_top" );
</script></head></html>
''')

write('cookies-refused.html', '''<html><body><div>Votre navigateur bloque les cookies nécessaires au fonctionnement du site.</div></body></html>
''')

write('expired.html', '''<!DOCTYPE html>
<html lang="fr"><head><script type="text/javascript">
\t\t\talert( "Votre session a expiré, vous allez être redirigé vers le sommaire." );
</script></head></html>
''')

write('module.html', '''<!DOCTYPE html>
<html lang="fr"><head><title>THOT Internet</title></head>
<frameset rows="178,*"><frame name="FrmHaut" src="FrmRechHaut.asp?MOD=10"><frame name="FrmMilieu" src="FrmRechDOCCritere.asp?MOD=10"></frameset>
</html>
''')

write('haut.html', '''<html><body><table><tr><td id="titre" class="titre">Recherche documentaire : </td></tr></table></body></html>
''')

write('challenge.html', '''<!DOCTYPE html><html lang="en-US"><head><title>Just a moment...</title></head>
<body><script>(function(){window._cf_chl_opt={cType: 'managed'};}());</script></body></html>
''')

# ----------------------------------------------------------------- forms


def select(criterion, values):
    options = ''.join(f'<option value="{value}">{value}</option>' for value in values)
    return (f'<input type="hidden" id="txt_CIN_IDX{criterion}" name="txt_CIN_IDX{criterion}" value="0" />\n'
            f'<input type="hidden" name="txt_CIN_CH{criterion}" value="" />\n'
            f'<select class="input width100" id="txt_CIN_LISTE{criterion}" onchange="javascript: doQuery( this.value, '
            f'document.FormRecherche.txt_CIN_CH{criterion}, document.FormRecherche.txt_CIN_IDX{criterion} );">'
            f'<option value="">&nbsp;</option>{options}</select>\n')


def checkboxes(criterion, values):
    boxes = ''.join(
        f'<input type="checkbox" onclick="javascript: doQuery( concatenerCheckbox( document.FormRecherche.cbx_txt_CIN_CH{criterion} ), '
        f'document.FormRecherche.txt_CIN_CH{criterion}, document.FormRecherche.txt_CIN_IDX{criterion} );" '
        f'id="cbx_txt_CIN_CH{criterion}_{index}" name="cbx_txt_CIN_CH{criterion}" value="{value}" />'
        f'<label class="texte" for="cbx_txt_CIN_CH{criterion}_{index}">{value}</label>\n'
        for index, value in enumerate(values))
    return (f'<input type="hidden" name="txt_CIN_IDX{criterion}" value="0" />\n'
            f'<input type="hidden" name="txt_CIN_CH{criterion}" value="" />\n{boxes}')


DEX = ('<label class="richText" for="txt_CIN_DEX_D">Années</label>\n'
       '<input type="text" class="input" name="txt_CIN_DEX_D" id="txt_CIN_DEX_D" value="" size="4" maxlength="4" />\n'
       '<input type="text" class="input" name="txt_CIN_DEX_F" id="txt_CIN_DEX_F" value="" size="4" maxlength="4" />\n')


def interval(criterion):
    return (f'<label class="richText" for="intervalleDate1_{criterion}">Dates extrêmes</label>\n'
            f'<input type="hidden" id="txt_CIN_CH{criterion}" name="txt_CIN_CH{criterion}" value="" />\n'
            f'<input type="text" class="input" name="intervalleDate1_{criterion}" id="intervalleDate1_{criterion}" value="" '
            f'onblur="javascript: formateDex( this ); concatenerDates( document.FormRecherche.intervalleDate1_{criterion}, '
            f'document.FormRecherche.intervalleDate2_{criterion}, document.FormRecherche.txt_CIN_CH{criterion} );" />\n'
            f'<input type="text" class="input" name="intervalleDate2_{criterion}" id="intervalleDate2_{criterion}" value="" />\n')


def form(body):
    return ('<!DOCTYPE html>\n<html><head><title>Recherche documentaire</title></head><body>\n'
            '<form name="FormRecherche" method="post" action="FrmRechDOCCritere.asp" target="FrmMilieu" '
            'onsubmit="javascript: return verifSaisie();" class="centrer">\n'
            f'{body}'
            '<input type="hidden" name="b_ExecForm" value="1" />\n'
            '<input type="hidden" name="txt_IDX_OCC" value="" />\n'
            '<input type="submit" value="Rechercher" />\n</form>\n'
            '<div class="credits">Numérisation : un partenaire fictif</div>\n</body></html>\n')


LOCALITIES = ['BOURG-EXEMPLE (LE)', 'EXAMPLEVILLE', 'LA-VILLE-EXEMPLE', 'SAMPLETON']
ACTS = ['bans', 'baptemes', 'catholicite', 'deces', 'mariages', 'naissances', 'sepultures', 'tables']

write('form-registers.html', form(select(0, LOCALITIES) + DEX + select(2, ACTS)))
write('form-census.html', form(select(0, ['EXAMPLEVILLE', 'EXAMPLEVILLE (NORD-EST)', 'SAMPLETON']) + interval(1)))
write('form-military.html', form(
    select(0, ['AUTRES DEPARTEMENTS (HORS EXEMPLE)', 'EXAMPLEVILLE', 'SAMPLETON', 'SAMPLETON (ARRONDISSEMENT)',
               'SAMPLETON (SUBDIVISION MILITAIRE)'])
    + select(1, ['REGISTRES MATRICULES', 'TABLE DES REGISTRES MATRICULES']) + interval(2)))
write('form-successions.html', form(
    select(0, ['EXAMPLEVILLE (BUREAU DE L\'ENREGISTREMENT)', 'SAMPLETON (BUREAU DE L\'ENREGISTREMENT)'])
    + select(1, ['ENREGISTREMENT SAUF TSA (MAUVAIS ETAT)', 'TABLES DE SUCCESSIONS ET ABSENCES']) + interval(2)))
write('form-corse.html', form(
    select(0, ['ABBAYE (EXAMPLEVILLE, EXEMPLE, FRANCE ; HAMEAU)', 'EXAMPLEVILLE (EXEMPLE, FRANCE)',
               'SAMPLETON (AUTRE-EXEMPLE, FRANCE)'])
    + DEX + checkboxes(2, ['BAPTEMES', 'DECES', 'MARIAGES', 'NAISSANCES', 'PUBLICATIONS DE MARIAGE',
                           'SEPULTURES', 'TABLE DECENNALE'])))

# ---------------------------------------------------------------- results


def heading(title, sort):
    return (f'<th scope="col"><a class="header_tri" title="Trier" href="FrmRechListeHaut.asp?RechDoc=1&amp;page=1'
            f'&amp;numTri={sort}&amp;triOrder=ASC&amp;imprimer="><span>{title}</span></a></th>\n')


def lot(base, application, idfic, idlot, ref, flag='0'):
    return (f'<td class="texteCentre"><a class="lienCadre" title="Fiche descriptive détaillée&#xA; (nouvelle fenêtre)" '
            f'href="javascript: openFiche( \'{idfic}\', \'{ref}\', \'{base}\', \'\',\'\',\'{application}\' );"><img src="{base}/images/zoom.png" alt="" /></a></td>\n'
            f'<td class="texteCentre"><a class="lienCadre" title="Afficher le document numérisé&#xA; (nouvelle fenêtre)" '
            f'href="javascript: openLot( \'{idfic}\', \'{idlot}\', \'{ref}\', \'{application}\', \'\', \'{flag}\' );">'
            f'<img src="{base}/images/numerise.png" alt="" /></a></td>\n')


def results(count, headings, rows, pages=1, page=1):
    pager = ''
    if pages > 1:
        links = ''.join(f'<a href="FrmRechListeHaut.asp?RechDoc=1&amp;page={n}">{n}</a> ' for n in range(1, pages + 1) if n != page)
        pager = f'<div class="resultatpage2">Page : {links}</div>\n'
    head = ''.join(heading(title, index + 1) for index, title in enumerate(headings))
    head += '<th scope="col" class="width0">Fiche détaillée</th>\n<th scope="col" class="width0">Voir le document</th>\n'
    body = ''
    for index, (cells, link) in enumerate(rows):
        tds = ''.join(f'<td class="texteGauche">{cell}</td>\n' for cell in cells)
        body += f'<tr class="l{2 - index % 2}">\n{tds}{link}</tr>\n'
    table = (f'<table class="tabListe width100" summary="Liste de résultats">\n<colgroup><col width="15%" /></colgroup>\n'
             f'<thead>\n<tr>\n{head}</tr>\n</thead>\n<tbody>\n{body}</tbody>\n</table>\n') if rows else ''
    return ('<!DOCTYPE html>\n<html><head><title>Résultats de la recherche</title></head>\n'
            '<body onload="javascript: initPage( \'\' );"><div class="container">\n'
            '<form name="FormListe" method="post" action="../FrmSaisieDateMiseADispo.asp" target="FrmBas">\n'
            f'<table class="width100"><tr><td>\n<div class="resultatrech">{count}</div>\n</td></tr></table>\n'
            f'{pager}{table}</form></div></body></html>\n')


IV_HEADINGS = ['Cote(s)', 'Commune', 'Topographie', 'Date', 'Type d\'acte', 'Type de collection']


def iv_row(call_number, locality, period, acts, collection, idfic, idlot, flag='0'):
    cells = [call_number, locality, '', f'<span class="texteNowrap">{period}</span>', acts, collection]
    return cells, lot(IV, 'THOPDESC', idfic, idlot, idlot, flag)


write('results-one.html', results('Une seule fiche correspond à votre recherche', IV_HEADINGS, [
    iv_row('9 NUM 99001 12', 'EXAMPLEVILLE', '1850', 'Naissances', 'GREFFE', '900012', '700012'),
]))
write('results-copies.html', results('3 fiches correspondent à votre recherche', IV_HEADINGS, [
    iv_row('9 NUM 99001 2', 'EXAMPLEVILLE', '1793 - 1794', 'Naissances', 'COMMUNE', '900002', '700002'),
    iv_row('9 NUM 99001 52', 'EXAMPLEVILLE', '1793 - 1794', 'Naissances', 'GREFFE', '900052', '700052'),
    iv_row('9 NUM 99001 53', 'EXAMPLEVILLE', '1793 - 1794', 'Naissances', 'GREFFE', '900053', 'cfecFichier'),
]))
write('results-table.html', results('2 fiches correspondent à votre recherche', IV_HEADINGS, [
    iv_row('9 NUM 99001 7', 'EXAMPLEVILLE', '1674 - 1733', 'Tables Baptêmes Mariages Sépultures', 'COMMUNE', '900007', '700007'),
    iv_row('9 NUM 99001 8', 'EXAMPLEVILLE', '1700', 'Baptêmes/Mariages/Sépultures', 'COMMUNE', '900008', '700008'),
]))
write('results-restricted.html', results('Une seule fiche correspond à votre recherche', IV_HEADINGS, [
    iv_row('9 NUM 99001 90', 'EXAMPLEVILLE', '1925', 'Décès', 'GREFFE', '900090', '700090', flag='1'),
]))
write('results-page-1.html', results('41 fiches correspondent à votre recherche', IV_HEADINGS, [
    iv_row(f'9 NUM 99002 {n}', 'BOURG-EXEMPLE (LE)', str(1800 + n), 'Naissances', 'COMMUNE', str(910000 + n), str(710000 + n))
    for n in range(1, 4)
], pages=2, page=1))
write('results-page-2.html', results('41 fiches correspondent à votre recherche', IV_HEADINGS, [
    iv_row('9 NUM 99002 41', 'BOURG-EXEMPLE (LE)', '1841', 'Naissances', 'COMMUNE', '910041', '710041'),
], pages=2, page=2))
write('results-none.html', results('Aucune fiche ne correspond à votre recherche', IV_HEADINGS, []))

write('results-census.html', results('2 fiches correspondent à votre recherche',
                                     ['Cote(s)', 'Intitulé', 'Commune', 'Type de document', 'Dates'], [
    (['9 NUM 99100 4', 'Exampleville, recensement de population : liste nominative', 'EXAMPLEVILLE', 'LISTE NOMINATIVE', '1851'],
     lot(IV, 'THOPDESC', '920004', 'THOPDESC_720004', '720004')),
    (['9 NUM 99100 5', 'Exampleville, recensement de population : liste nominative', 'EXAMPLEVILLE', 'LISTE NOMINATIVE', '1856'],
     lot(IV, 'THOPDESC', '920005', 'THOPDESC_720005', '720005')),
]))
write('results-military.html', results('3 fiches correspondent à votre recherche',
                                       ['Cote(s)', 'Intitulé', 'Type d\'acte', 'Date'], [
    ([f'9 R {990 + n}', f'Subdivision militaire de Sampleton. Volume {n}, numéros matricules {first}-{last}.',
      'REGISTRES MATRICULES', '1900'], lot(IV, 'THOPDESC', str(930000 + n), f'THOPDESC_{730000 + n}', str(730000 + n)))
    for n, (first, last) in enumerate([(1, 500), (501, 1000), (1001, 1250)], start=1)
]))
write('results-successions.html', results('2 fiches correspondent à votre recherche',
                                          ['Cote(s)', 'Intitulé', 'Type de document', 'Bureau', 'Dates'], [
    ([f'9 Q 9/{n}', f'Vol.{n} du 1er janvier {first} au 31 décembre {last}', 'TABLES DE SUCCESSIONS ET ABSENCES',
      'EXAMPLEVILLE (BUREAU DE L\'ENREGISTREMENT)', f'{first} - {last}'],
     lot(IV, 'THOPDESC', str(950000 + n), f'THOPDESC_{750000 + n}', str(750000 + n)))
    for n, (first, last) in enumerate([(1815, 1819), (1820, 1825)], start=4)
]))
write('results-corse.html', results('3 fiches correspondent à votre recherche', ['Cote(s)', 'Intitulé', 'Dates'], [
    (['99 NUM 1', 'Etat-civil - Registre des actes de naissances, mariages, publications de mariage et décès '
      'de la commune d\'Exampleville de 1865 à 1873.', '1865 - 1873'],
     lot(CORSE, 'THOTDESC', '940001', 'THOTDESC_740001', '740001')),
    (['99 NUM 3', 'Etat-civil - Registre des actes de naissances de la commune d\'Exampleville de 1874 à 1880.', '1874 - 1880'],
     lot(CORSE, 'THOTDESC', '940003', 'THOTDESC_740003', '740003')),
    (['99 NUM 5', 'Etat-civil - Tables décennales de la commune d\'Exampleville de 1873 à 1882.', '1873 - 1882'],
     lot(CORSE, 'THOTDESC', '940005', 'THOTDESC_740005', '740005')),
]))

# ---------------------------------------------------------------- viewer

write('viewer.html', f'''<!DOCTYPE html>
<html><head><meta charset="windows-1252"><title>Visionneuse Thot</title></head>
<body><div id="myContainer"></div></body>
<script type="text/javascript">
\t\t\t\t\tvar repVisionneuse = "{IV}/Ressources";
\t\t\t\t\tvar visoParam = "zSlidePath={IV}/download/thot/100000001/slides_00000001.xml";
\t\t\t\t\tvisoParam += "&zSkinPath=" + repVisionneuse + "/Skins";
</script></html>
''')


def slides(ark, count):
    # The ARK base is on the portal's own origin, which the adapter checks.
    setup = (f'<SETUP AUTOPLAY="0" AFFPERMALIENS="1" URLARK="{IV_ORIGIN}{IV}/ark:/99999" '
             f'MSGMAIL="Je vous recommande cette page : " />' if ark else '<SETUP IMPRSIMPLE="0" VIGNETTE="1" />')
    views = ''.join(
        f'<SLIDE MEDIA="https://archives.example.org/LOT/EXAMPLE/VIEW_{n:04d}" NAME="9 NUM 99001 12 - EXAMPLEVILLE" '
        + (f'LIENARK="exmpl0000000/100001/{n}" ' if ark else '')
        + f'FOLDERID="700012" DOCID="{800000 + n}" />\n'
        for n in range(1, count + 1))
    return f'<SLIDEDATA>\n{setup}\n{views}</SLIDEDATA>\n'


write('slides-ark.xml', slides(True, 3))
write('slides-plain.xml', slides(False, 3))
