"""Writes the anonymized fixtures of the older Mnesys interface beside this script: `python3 generate.py`.

They have the markup of pages recorded from the Savoie archives' guided
searches (a form, answers listing finding-aid nodes on one page or
several, an answer without hits, notices with and without a link to the
images), with fictitious localities, parishes, call numbers, node and ARK
identifiers; no recorded value is copied."""
import pathlib

OUT = pathlib.Path(__file__).resolve().parent

FORM = 'recherche_guidee_etat_civil_web'
DOC = 'accounts%2Fmnesys_example%2Fdatas%2Fir%2FETA%2F4E%2FFRAD999_4E_1-99%2Exml'
VIEWER = 'https://archives-numeriques.savoie.fr'


def write(name, text):
    (OUT / name).write_text(text, encoding='utf-8')


HEAD = ('<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Transitional//EN">\n<html><head>\n'
        '<meta http-equiv="Content-Type" content="text/html; charset=utf-8" />\n<title>Archives</title>\n'
        '</head><body>\n<div id="content">\n')
TAIL = '</div>\n</body>\n</html>\n<!-- \niNAO (powered by Naoned)\n-->\n'


def titled_select(field, label, options):
    return (f'<select class="form_search_{field}" name="form_search_{field}">\n<option></option>\n'
            + ''.join(f'<option value="{value}">{text}</option>\n' for value, text in options) +
            f'</select>\n<input type="hidden" name="label_{field}" value="{label}"/>\n'
            f'<select name="form_op_{field}">\n<option value="ET">ET</option>\n<option value="OU" >OU</option>\n'
            f'</select>\n<input type="hidden" name="form_req_{field}" value="{{:unittitle}}__VAL_"/>\n')


def form(name, form_id, fields):
    write(name, HEAD + (
        '<form action="" name="F_search" id="F_search" method="get">\n'
        '<input type="hidden" name="label_geogname" value="Communes, lieux" />\n'
        '<input name="form_search_geogname" type="text" value=""/>\n'
        '<input name="form_search_unitdate1" type="text" value=""/>\n'
        '<input name="form_search_unitdate2" type="text" value="" style="margin-right:0"/>\n'
        '<input name="form_search_unitdate3" type="text" value=""/>\n'
        '<input name="form_search_unitdate" id="form_search_unitdate" type="hidden" value="">\n'
        '<input name="form_search_dao" type="checkbox" class="check_noborder" value="oui"/>\n'
        + fields +
        '<input type="hidden" name="display_thesaurus" value="autocomplete"/>\n'
        '<input type="submit" name="btn_valid" class="btn_ok_right" value="Rechercher" onclick="return verif_date();"/>\n'
        '<input type="hidden" name="action" value="search"/>\n'
        f'<input type="hidden" name="id" value="{form_id}"/>\n</form>\n') + TAIL)


def item(node, date, call_number, title, context, doc=DOC):
    ariane = (f'<a class="various link_ariane" href="/?id={FORM}_detail&amp;doc={doc}">'
              'Registres paroissiaux et registres de l\'état-civil.</a>')
    for index, entry in enumerate(context):
        ariane += (f' > <!--{entry} &gt; -->\n<a class="various link_ariane" href="?id={FORM}_detail&amp;'
                   f'doc={doc}&amp;page_ref={900 + index}">{entry}</a>')
    return ('<li>\n\t<div class="caddie">\n'
            f'\t\t<a href="/?id={FORM}&doc={doc}&page_ref={node}&select_node={node}&keep=search" class="picto_caddie"></a>\n'
            '\t</div>\n\t<div class="result_cote_date">\n'
            f'\t\t<!--<div class="date">{date}\n</div>-->\n\t\t<div class="date">{date}</div>\n'
            f'\t\t<!--<div class="cote"><b>Cote : </b>{call_number}</div>-->\n'
            f'\t\t<div class="cote"><b>Cote : </b><strong>{call_number}</strong></div>\n\t</div>\n'
            '\t<div class="result">\n\t\t<div class="title">\n'
            f'\t\t\t<a class="various" href="/?id={FORM}_detail&doc={doc}&page_ref={node}" title="">\n'
            f'\t\t\t\t{title}\n\t\t\t</a>\n\t\t</div>\n\t\t<div class="source">&nbsp;</div>\n'
            f'\t\t<div class="ariane">{ariane}</div>\n\t</div>\n</li>\n')


def answer(name, total, items, pages=1, current=1):
    count = 'Aucune réponse' if total == 0 else f'{total} &nbsp;réponse{"s" if total > 1 else ""}'
    text = HEAD + (
        '<form action="" name="F_search" id="F_search" method="get">\n'
        "\t\t\t\t\t<ul class='complete_search'>\n<li class=\"li_geogname\">Exampleville</li>\n</ul>\n"
        '\t\t<div class="entete_reponses">\n\t\t\t<span class="nb_reponses">\n\n\n'
        f'{count}\n<!--\n &nbsp;&nbsp;<a href="/?id={FORM}&keep=search&print=1" class="print_reps"></a>\n-->\n\n</span>\n'
        '\t\t</div>\n')
    if items:
        text += "<div class='list'>\n\t<ul>\n\t\t" + ''.join(items) + '\t</ul>\n</div>\n'
    if pages > 1:
        text += "<div class='navigation'>\n"
        for page in range(1, pages + 1):
            text += (f'\t<b>{page}</b>\n' if page == current else
                     f"<a href='/?id={FORM}&doc=&page={page}&page_ref=' class=\"page\">{page}</a>\n")
        text += '</div>\n'
    write(name, text + '\t</form>\n' + TAIL)


def notice(name, link):
    media = (f'\t<span class="elt_title">Liens vers documents numérisés :</span>\n\t<ul><li>\n'
             f'\t<a href="{link[0]}" target="_blank" class="media_link">- Voir : {link[1]}</a><br/>\n</li></ul>\n'
             if link else '')
    write(name, HEAD + (
        '\t<div class="notice" id="notice_detail">\n'
        '\t\t\t<div class="title">Registre paroissial : mariages.</div>\n\t\t\t<div class=\'detail\'>\n'
        '\t\t\t<div class="elt_simple"><span class="elt_title">Date</span>\n1842-1860</div>\n'
        f'\t\t\t<div class="elt_simple">\n{media}</div>\n'
        '\t\t\t<div class="elt_simple"><span class="elt_title">Conditions d\'accès</span>\n'
        "<div class='ead_p plevel9'>NC Archives en ligne </div></div>\n\t\t</div>\n\t</div>\n") + TAIL)


form('form-registers.html', FORM, titled_select('v2_field_1000000000AAAAaa', 'Tables décennales et types d\'actes', [
    ('Baptêmes OU naissances', 'Naissances / baptêmes'), ('Mariages', 'Mariages / promesses de mariages'),
    ('Décès OU sépultures', 'Décès / sépultures'), ('Tables', 'Tables décennales')]))
form('form-military.html', 'recherche_guidee_registres_matricules_web',
     titled_select('v2_field_2000000000BBBBbb', 'Type de document', [
         ('Répertoire alphabétique de la classe', 'Répertoire alphabétique'),
         ('Registre matricules de la classe SAUF Répertoire', 'Registre matricules')])
     + titled_select('v2_field_3000000000CCCCcc', 'Classe', [
         (f'&quot;Classe {year}&quot;', f'Classe {year}') for year in (1899, 1900, 1901)]))

answer('answer-one.html', 1, [
    item(154001, '1842-1860', '3E 9017', 'Registre paroissial : mariages. - Exampleville.',
         ['3E - Collection des registres de catholicité', '3E 9001 à 3E 9100']),
])
answer('answer-city.html', 6, [
    # A heading over registers, without a call number.
    item(15000, '1613-1860', '', 'Baptêmes, mariages religieux, sépultures.',
         ['Sampleton.', 'Registres paroissiaux.']),
    item(18100, '1795-1860', '', 'Mariages religieux.', ['Exampleville.', 'Notre-Exemple.']),
    item(18242, '1842-1850', '4E 9362', '1842-1850.', ['Exampleville.', 'Notre-Exemple.', 'Mariages religieux.']),
    item(19579, '1850', '4E 9260', '1850.', ['Exampleville.', 'Saint-Exemple.', 'Mariages religieux.']),
    item(21268, '1775-1911', '4E 9346',
         'Table chrono-alphabétique des baptêmes, mariages religieux et sépultures (1775-1911)...',
         ['Exampleville.', 'Saint-Autre.', 'Tables.']),
    # A register of several places, which names none alone.
    item(27047, '1774-1815', '3E 9440', 'Registre paroissial : baptêmes, mariages, sépultures. - Exampleville, Sampleton,...',
         ['3E - Collection des registres de catholicité', '3E 9401 à 3E 9500']),
], pages=2)
answer('answer-city-2.html', 6, [
    item(19600, '1851', '4E 9261', '1851.', ['Exampleville.', 'Saint-Exemple.', 'Mariages religieux.']),
], pages=2, current=2)
answer('answer-census.html', 1, [
    item(8913, '1901', '6M 9150', 'Liste nominative du recensement de la population, 1901.',
         ['6M - Liste nominative du recensement de la population.', 'Exampleville. 1876-1936 (6M 9145-9156)'],
         doc='accounts%2Fmnesys_example%2Fdatas%2Fir%2FMOD%2FM%2F6M%2FFRAD999_6M_1-99%2Exml'),
])
answer('answer-military.html', 2, [
    item(6373, '1900', '1R 9142', 'Registre matricules de la classe 1900 : volume 1, n° 1 à 502.',
         ['1R - Etats signalétiques et des services.', 'Registres matricules et répertoires.'],
         doc='accounts%2Fmnesys_example%2Fdatas%2Fir%2FMOD%2FR%2F1R%2FFRAD999_1R_1-99%2Exml'),
    item(6417, '1900', '1R 9143', 'Registre matricules de la classe 1900 : volume 2, n° 503 à 1003.',
         ['1R - Etats signalétiques et des services.', 'Registres matricules et répertoires.'],
         doc='accounts%2Fmnesys_example%2Fdatas%2Fir%2FMOD%2FR%2F1R%2FFRAD999_1R_1-99%2Exml'),
])
answer('answer-none.html', 0, [])
notice('notice.html', (f'{VIEWER}/ark:/99999/0123456789abcdef', '3E 9017'))
notice('notice-without-images.html', None)
notice('notice-elsewhere.html', ('https://elsewhere.example.org/viewer?id=1', '3E 9017'))
