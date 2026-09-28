//! The word tables, each language's numbers and dates, and reading back.

use super::*;

fn ymd(year: i32, month: u8, day: u8) -> Ymd {
    Ymd {
        year,
        month: Some(month),
        day: Some(day),
    }
}

#[test]
fn english_numbers() {
    let cases = [
        (1, "one"),
        (11, "eleven"),
        (19, "nineteen"),
        (20, "twenty"),
        (21, "twenty-one"),
        (90, "ninety"),
        (100, "one hundred"),
        (101, "one hundred and one"),
        (999, "nine hundred and ninety-nine"),
        (1000, "one thousand"),
        (1650, "one thousand six hundred and fifty"),
        (2024, "two thousand and twenty-four"),
    ];
    for (n, words) in cases {
        assert_eq!(en_cardinal(n), words, "{n}");
    }
    assert_eq!(en_ordinal(1), "first");
    assert_eq!(en_ordinal(12), "twelfth");
    assert_eq!(en_ordinal(20), "twentieth");
    assert_eq!(en_ordinal(31), "thirty-first");
}

#[test]
fn french_numbers() {
    let cases = [
        (1, "un"),
        (11, "onze"),
        (16, "seize"),
        (17, "dix-sept"),
        (21, "vingt et un"),
        (30, "trente"),
        (71, "soixante et onze"),
        (72, "soixante-douze"),
        (80, "quatre-vingts"),
        (81, "quatre-vingt-un"),
        (91, "quatre-vingt-onze"),
        (200, "deux cents"),
        (201, "deux cent un"),
        (999, "neuf cent quatre-vingt-dix-neuf"),
        (1000, "mille"),
        (1650, "mille six cent cinquante"),
        (2000, "deux mille"),
    ];
    for (n, words) in cases {
        assert_eq!(fr_cardinal(n), words, "{n}");
    }
}

#[test]
fn german_numbers() {
    let cases = [
        (1, "eins"),
        (11, "elf"),
        (17, "siebzehn"),
        (21, "einundzwanzig"),
        (30, "dreißig"),
        (101, "einhunderteins"),
        (999, "neunhundertneunundneunzig"),
        (1000, "eintausend"),
        (2001, "zweitausendeins"),
    ];
    for (n, words) in cases {
        assert_eq!(de_cardinal(n), words, "{n}");
    }
    assert_eq!(de_year(1650), "sechzehnhundertfünfzig");
    assert_eq!(de_year(1901), "neunzehnhunderteins");
    assert_eq!(de_year(2024), "zweitausendvierundzwanzig");
    assert_eq!(de_year(987), "neunhundertsiebenundachtzig");
    let ordinals = [
        (1, "ersten"),
        (2, "zweiten"),
        (3, "dritten"),
        (7, "siebten"),
        (8, "achten"),
        (16, "sechzehnten"),
        (20, "zwanzigsten"),
        (31, "einunddreißigsten"),
    ];
    for (n, words) in ordinals {
        assert_eq!(de_ordinal(n), words, "{n}");
    }
}

#[test]
fn spanish_numbers() {
    let cases = [
        (1, "uno"),
        (15, "quince"),
        (16, "dieciséis"),
        (20, "veinte"),
        (21, "veintiuno"),
        (22, "veintidós"),
        (31, "treinta y uno"),
        (100, "cien"),
        (101, "ciento uno"),
        (500, "quinientos"),
        (1000, "mil"),
        (1650, "mil seiscientos cincuenta"),
        (2000, "dos mil"),
    ];
    for (n, words) in cases {
        assert_eq!(es_cardinal(n), words, "{n}");
    }
}

#[test]
fn italian_numbers() {
    let cases = [
        (1, "uno"),
        (3, "tre"),
        (13, "tredici"),
        (17, "diciassette"),
        (21, "ventuno"),
        (23, "ventitré"),
        (28, "ventotto"),
        (30, "trenta"),
        (100, "cento"),
        (108, "centotto"),
        (180, "centottanta"),
        (999, "novecentonovantanove"),
        (1000, "mille"),
        (1650, "milleseicentocinquanta"),
        (1653, "milleseicentocinquantatré"),
        (2001, "duemilauno"),
    ];
    for (n, words) in cases {
        assert_eq!(it_cardinal(n), words, "{n}");
    }
}

#[test]
fn dutch_numbers() {
    let cases = [
        (1, "een"),
        (12, "twaalf"),
        (21, "eenentwintig"),
        (22, "tweeëntwintig"),
        (80, "tachtig"),
        (100, "honderd"),
        (1000, "duizend"),
        (2024, "tweeduizendvierentwintig"),
    ];
    for (n, words) in cases {
        assert_eq!(nl_cardinal(n), words, "{n}");
    }
    assert_eq!(nl_year(1650), "zestienhonderdvijftig");
    assert_eq!(nl_year(999), "negenhonderdnegenennegentig");
    let ordinals = [
        (1, "eerste"),
        (2, "tweede"),
        (3, "derde"),
        (8, "achtste"),
        (11, "elfde"),
        (20, "twintigste"),
        (31, "eenendertigste"),
    ];
    for (n, words) in ordinals {
        assert_eq!(nl_ordinal(n), words, "{n}");
    }
}

#[test]
fn polish_ordinals_in_the_genitive() {
    let cases = [
        (1, "pierwszego"),
        (2, "drugiego"),
        (11, "jedenastego"),
        (20, "dwudziestego"),
        (21, "dwudziestego pierwszego"),
        (31, "trzydziestego pierwszego"),
        (999, "dziewięćset dziewięćdziesiątego dziewiątego"),
        (1000, "tysięcznego"),
        (1600, "tysiąc sześćsetnego"),
        (1650, "tysiąc sześćset pięćdziesiątego"),
        (1912, "tysiąc dziewięćset dwunastego"),
        (2000, "dwutysięcznego"),
        (2024, "dwa tysiące dwudziestego czwartego"),
    ];
    for (n, words) in cases {
        assert_eq!(pl_ordinal(n), words, "{n}");
    }
    assert_eq!(
        written(Language::Pl, ymd(1650, 2, 2), Form::Long).unwrap(),
        "drugiego lutego tysiąc sześćset pięćdziesiątego roku"
    );
}

#[test]
fn portuguese_numbers() {
    let cases = [
        (1, "um"),
        (16, "dezasseis"),
        (22, "vinte e dois"),
        (100, "cem"),
        (101, "cento e um"),
        (999, "novecentos e noventa e nove"),
        (1000, "mil"),
        (1001, "mil e um"),
        (1500, "mil e quinhentos"),
        (1650, "mil seiscentos e cinquenta"),
        (1982, "mil novecentos e oitenta e dois"),
        (2000, "dois mil"),
    ];
    for (n, words) in cases {
        assert_eq!(pt_cardinal(n), words, "{n}");
    }
}

#[test]
fn roman_numerals() {
    let cases = [
        (1, "I"),
        (4, "IV"),
        (9, "IX"),
        (14, "XIV"),
        (40, "XL"),
        (90, "XC"),
        (400, "CD"),
        (1650, "MDCL"),
        (1999, "MCMXCIX"),
        (3999, "MMMCMXCIX"),
    ];
    for (n, numeral) in cases {
        assert_eq!(roman(n), numeral);
        assert_eq!(from_roman(numeral), Some(n));
    }
    // Not the canonical way: not read.
    assert_eq!(from_roman("IIII"), None);
    assert_eq!(from_roman("MIL"), None);
}

/// The reference example: 2 February 1650.
#[test]
fn latin_writes_the_reference_example() {
    let date = ymd(1650, 2, 2);
    assert_eq!(
        latin(date, Form::Long).unwrap(),
        "die secunda mensis Februarii anno Domini millesimo sexcentesimo quinquagesimo"
    );
    assert_eq!(latin(date, Form::Short).unwrap(), "II Februarius MDCL");
    assert_eq!(
        latin_roman_reckoning(Calendar::Gregorian, date).unwrap(),
        "ante diem IV Nonas Februarii"
    );
    let parts = latin_parts(date).unwrap();
    assert_eq!(
        parts.day,
        Some((2, "secunda".to_string(), "II".to_string()))
    );
    assert_eq!(
        parts.month,
        Some(("Februarii".to_string(), "II".to_string()))
    );
    assert_eq!(
        parts.year,
        (
            "millesimo sexcentesimo quinquagesimo".to_string(),
            "MDCL".to_string()
        )
    );
}

#[test]
fn latin_days_and_years() {
    assert_eq!(la_day(13), "decima tertia");
    assert_eq!(la_day(20), "vicesima");
    assert_eq!(la_day(31), "tricesima prima");
    assert_eq!(la_year(1702), "millesimo septingentesimo secundo");
    assert_eq!(la_year(2024), "bis millesimo vicesimo quarto");
    assert_eq!(la_year(999), "nongentesimo nonagesimo nono");
}

#[test]
fn the_roman_reckoning_counts_to_kalends_nones_and_ides() {
    let r = |y, m, d| latin_roman_reckoning(Calendar::Gregorian, ymd(y, m, d)).unwrap();
    assert_eq!(r(1650, 3, 1), "Kalendis Martii");
    assert_eq!(r(1650, 3, 6), "pridie Nonas Martii");
    assert_eq!(r(1650, 3, 7), "Nonis Martii");
    assert_eq!(r(1650, 3, 15), "Idibus Martii");
    assert_eq!(r(1650, 2, 14), "ante diem XVI Kalendas Martii");
    assert_eq!(r(1650, 12, 31), "pridie Kalendas Januarii");
    // A leap February doubles the sixth day before the Kalends.
    assert_eq!(r(1652, 2, 24), "ante diem bis VI Kalendas Martii");
    assert_eq!(r(1652, 2, 25), "ante diem VI Kalendas Martii");
    assert_eq!(r(1652, 2, 29), "pridie Kalendas Martii");
}

#[test]
fn every_language_writes_its_dates() {
    let date = ymd(1650, 2, 2);
    let long = |l| written(l, date, Form::Long).unwrap();
    assert_eq!(
        long(Language::En),
        "the second of February, one thousand six hundred and fifty"
    );
    assert_eq!(
        long(Language::Fr),
        "le deux février mille six cent cinquante"
    );
    assert_eq!(
        long(Language::De),
        "am zweiten Februar sechzehnhundertfünfzig"
    );
    assert_eq!(
        long(Language::Es),
        "dos de febrero de mil seiscientos cincuenta"
    );
    assert_eq!(long(Language::It), "due febbraio milleseicentocinquanta");
    assert_eq!(
        long(Language::Nl),
        "de tweede februari zestienhonderdvijftig"
    );
    assert_eq!(
        long(Language::Pt),
        "dois de fevereiro de mil seiscentos e cinquenta"
    );
    assert_eq!(
        written(Language::Fr, ymd(1650, 2, 1), Form::Long).unwrap(),
        "le premier février mille six cent cinquante"
    );
    assert_eq!(
        written(Language::De, ymd(1650, 2, 2), Form::Short).unwrap(),
        "2. Februar 1650"
    );
    assert_eq!(
        written(Language::Pl, ymd(1650, 2, 2), Form::Short).unwrap(),
        "2 lutego 1650"
    );
    assert_eq!(
        written(
            Language::Pl,
            Ymd {
                year: 1650,
                month: Some(2),
                day: None
            },
            Form::Short
        )
        .unwrap(),
        "luty 1650"
    );
    assert!(written(Language::En, ymd(4000, 1, 1), Form::Long).is_none());
}

#[test]
fn the_annunciation_style_numbers_early_months_with_the_previous_year() {
    let style = |m, d| ymd(1650, m, d).in_style(YearStart::Annunciation).year;
    assert_eq!(style(2, 2), 1649);
    assert_eq!(style(3, 24), 1649);
    assert_eq!(style(3, 25), 1650);
    assert_eq!(style(12, 31), 1650);
    assert_eq!(ymd(1650, 2, 2).in_style(YearStart::January).year, 1650);
}

/// Whatever is written reads back as the same date, in every language and
/// both forms, including the edge days and years.
#[test]
fn written_dates_read_back() {
    let years = [1, 13, 999, 1000, 1100, 1492, 1650, 1901, 2024, 3999];
    let days = [1, 2, 3, 8, 11, 13, 17, 20, 21, 29, 31];
    for year in years {
        for month in 1..=12u8 {
            for day in days {
                if day > days_in_month(Calendar::Gregorian, year, month) {
                    continue;
                }
                let date = ymd(year, month, day);
                for language in Language::ALL {
                    for form in [Form::Long, Form::Short] {
                        let text = written(language, date, form).unwrap();
                        assert_eq!(read(&text), Ok(date), "{language:?} {text}");
                    }
                }
                for form in [Form::Long, Form::Short] {
                    let text = latin(date, form).unwrap();
                    assert_eq!(read(&text), Ok(date), "{text}");
                }
                let text = format!(
                    "{} anno Domini {}",
                    latin_roman_reckoning(Calendar::Gregorian, date).unwrap(),
                    roman(year as u32)
                );
                // The Kalends of January name the next month of the year.
                assert_eq!(read(&text), Ok(date), "{text}");
            }
            let month_only = Ymd {
                year,
                month: Some(month),
                day: None,
            };
            for language in Language::ALL {
                let text = written(language, month_only, Form::Long).unwrap();
                assert_eq!(read(&text), Ok(month_only), "{language:?} {text}");
            }
        }
    }
}

#[test]
fn other_ways_of_writing_a_date_are_read() {
    let feb = ymd(1650, 2, 2);
    for text in [
        "le 2 février 1650",
        "February 2nd, 1650",
        "2 FEB 1650",
        "2/2/1650",
        "1650-02-02",
        "dos de febrero del año mil seiscientos cincuenta",
        "die secundo mensis februarii anno domini millesimo sexcentesimo quinquagesimo",
        "ante diem quartum Nonas Februarias MDCL",
        "a.d. IV Non. Feb. 1650",
    ] {
        assert_eq!(read(text), Ok(feb), "{text}");
    }
    assert_eq!(read("le 1er mars 1650"), Ok(ymd(1650, 3, 1)));
    assert_eq!(read("pridie Idus Martias 1650"), Ok(ymd(1650, 3, 14)));
    assert_eq!(
        read("quatre-vingt-dix"),
        Ok(Ymd {
            year: 90,
            month: None,
            day: None
        })
    );
    assert_eq!(
        read("MDCL"),
        Ok(Ymd {
            year: 1650,
            month: None,
            day: None
        })
    );
}

#[test]
fn what_cannot_be_read_says_why() {
    assert_eq!(read("  "), Err(ReadError::Empty));
    assert_eq!(read("février"), Err(ReadError::NoYear));
    assert_eq!(read("some words"), Err(ReadError::NoYear));
    assert_eq!(read("31 February 1650"), Err(ReadError::NoSuchDay));
}
