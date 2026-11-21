//! A fictitious population of any size, written as GEDCOM: the tree the
//! large-tree runs import.
//!
//! Unlike [`super::family_blocks_gedcom`], whose blocks are unrelated, this
//! tree is one connected population over [`GENERATIONS`] generations: the
//! children of one generation marry each other, a man with a woman born a
//! third of the generation away, so a late person's ancestry widens
//! generation after generation until it meets itself (implex), and an early
//! couple's descendants multiply through the whole population — the two
//! shapes whose cost grows with a real tree. Names, places, dates, notes and sources are invented
//! and drawn from small pools.

use std::io::{self, Write};

/// Generations of the population, founders included.
pub const GENERATIONS: usize = 16;

/// The given name of the first founder: the root of the widest descent.
pub const FOUNDER: &str = "Anchor";
/// The given name of the last person born: the root of the deepest ancestry.
pub const PROBAND: &str = "Zenith";
/// The given name of a person of the last generation born far from
/// [`PROBAND`]: a distant cousin.
pub const COUSIN: &str = "Yarrow";
/// The given name of a founder of another couple than [`FOUNDER`]'s: no
/// blood relative of his, linked to him through their descendants' unions.
pub const STRANGER: &str = "Xanthe";

/// The place [`HUB_USES`] persons are born in.
pub const HUB_PLACE: &str = "Hubminster, Shire";
/// The place [`SMALL_HUB_USES`] persons are born in.
pub const SMALL_HUB_PLACE: &str = "Sixtyford, Shire";
/// Births in [`HUB_PLACE`], in a population of 10,000 persons or more.
pub const HUB_USES: usize = 2_000;
/// Births in [`SMALL_HUB_PLACE`].
pub const SMALL_HUB_USES: usize = 60;

const SURNAME_HEADS: [&str; 16] = [
    "Ash", "Birch", "Cold", "Dun", "Elm", "Fern", "Grey", "Hazel", "Ivy", "Kings", "Lark", "Marsh",
    "North", "Oak", "Pine", "Reed",
];
const SURNAME_TAILS: [&str; 16] = [
    "down", "ley", "well", "more", "worth", "hill", "wood", "ford", "field", "brook", "mere",
    "stead", "croft", "combe", "wick", "holt",
];
const MEN: [&str; 16] = [
    "Arthur", "Bernard", "Cyril", "Dorian", "Edmund", "Felix", "Gareth", "Horace", "Ivor",
    "Jasper", "Kenneth", "Lionel", "Magnus", "Nigel", "Osric", "Piers",
];
const WOMEN: [&str; 16] = [
    "Alice", "Beatrice", "Clara", "Delia", "Edith", "Flora", "Greta", "Hester", "Iris", "Joan",
    "Kitty", "Lorna", "Mabel", "Nora", "Olive", "Prudence",
];
const OCCUPATIONS: [&str; 6] = [
    "Weaver",
    "Cooper",
    "Miller",
    "Thatcher",
    "Wheelwright",
    "Chandler",
];
const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

/// One person of the generation being written.
struct Member {
    id: usize,
    male: bool,
    /// Index of the surname, inherited from the father.
    surname: usize,
    /// The family this person is a child of, none for a founder.
    parents: Option<usize>,
}

/// One couple of a generation.
struct Couple {
    family: usize,
    husband: usize,
    wife: usize,
    surname: usize,
}

/// What [`write_gedcom`] wrote.
#[derive(Debug, Clone, Copy)]
pub struct Population {
    pub persons: usize,
    pub families: usize,
    pub bytes: u64,
}

struct Writer<W: Write> {
    out: W,
    bytes: u64,
    /// Persons to write in all.
    target: usize,
    persons: usize,
    families: usize,
}

impl<W: Write> Writer<W> {
    fn line(&mut self, text: &str) -> io::Result<()> {
        self.out.write_all(text.as_bytes())?;
        self.out.write_all(b"\n")?;
        self.bytes += text.len() as u64 + 1;
        Ok(())
    }

    fn place(&self, id: usize) -> String {
        if id.is_multiple_of(5) && id / 5 < HUB_USES {
            return HUB_PLACE.to_owned();
        }
        if id % 5 == 1 && id / 5 < SMALL_HUB_USES {
            return SMALL_HUB_PLACE.to_owned();
        }
        let places = (self.target / 40).max(5);
        format!("Village {}, Shire", (id / 20) % places)
    }

    /// The given name of person `id`: one of the named four, or a common
    /// one of their sex.
    fn given(&self, member: &Member) -> &'static str {
        let id = member.id;
        match id {
            0 => FOUNDER,
            3 => STRANGER,
            _ if id + 1 == self.target => PROBAND,
            _ if id == self.target - self.target / 20 - 1 => COUSIN,
            _ if member.male => MEN[(id / 3) % MEN.len()],
            _ => WOMEN[(id / 3) % WOMEN.len()],
        }
    }

    fn person(
        &mut self,
        member: &Member,
        generation: usize,
        spouse_of: Option<usize>,
    ) -> io::Result<()> {
        let id = member.id;
        let given = self.given(member);
        let surname = format!(
            "{}{}",
            SURNAME_HEADS[member.surname % 16],
            SURNAME_TAILS[(member.surname / 16) % 16]
        );
        let year = 1500 + 25 * generation + id % 20;
        let place = self.place(id);
        let mut lines = vec![
            format!("0 @I{id}@ INDI"),
            format!("1 NAME {given} /{surname}/"),
            format!("1 SEX {}", if member.male { 'M' } else { 'F' }),
            "1 BIRT".to_owned(),
            format!(
                "2 DATE {} {} {year}",
                1 + id % 28,
                MONTHS[id % MONTHS.len()]
            ),
            format!("2 PLAC {place}"),
        ];
        if id.is_multiple_of(3) {
            lines.push(format!("2 SOUR @S{}@", id / 100));
            lines.push(format!("3 PAGE folio {id}"));
        }
        if year < 1880 {
            lines.push("1 DEAT".to_owned());
            lines.push(format!("2 DATE {}", year + 55 + id % 25));
            lines.push(format!("2 PLAC {place}"));
        }
        if id % 5 == 2 {
            lines.push(format!("1 OCCU {}", OCCUPATIONS[id % OCCUPATIONS.len()]));
        }
        if id % 4 == 1 {
            lines.push(format!(
                "1 NOTE Entry {id} of the invented register: read from a fictitious \
                 transcription, kept here to give the notes of a generated tree the \
                 weight of the ones a researcher writes."
            ));
            lines.push("2 CONT The second line of the note says nothing either.".to_owned());
        }
        lines.extend(member.parents.map(|family| format!("1 FAMC @F{family}@")));
        lines.extend(spouse_of.map(|family| format!("1 FAMS @F{family}@")));
        lines.iter().try_for_each(|line| self.line(line))
    }

    fn family(&mut self, couple: &Couple, generation: usize, children: &[usize]) -> io::Result<()> {
        let family = couple.family;
        self.line(&format!("0 @F{family}@ FAM"))?;
        self.line(&format!("1 HUSB @I{}@", couple.husband))?;
        self.line(&format!("1 WIFE @I{}@", couple.wife))?;
        for child in children {
            self.line(&format!("1 CHIL @I{child}@"))?;
        }
        self.line("1 MARR")?;
        self.line(&format!("2 DATE {}", 1522 + 25 * generation + family % 5))?;
        let place = self.place(couple.wife);
        self.line(&format!("2 PLAC {place}"))
    }

    /// Pair the men and the women of a generation, a man with the woman
    /// born a third of the generation later — never a sibling, and far
    /// enough for the lineages to mix — and write every member with the
    /// family they are a spouse of.
    fn marry(&mut self, members: &[Member], generation: usize) -> io::Result<Vec<Couple>> {
        let men: Vec<&Member> = members.iter().filter(|m| m.male).collect();
        let women: Vec<&Member> = members.iter().filter(|m| !m.male).collect();
        let pairs = men.len().min(women.len());
        let mut spouse_of = vec![None; members.len()];
        let first = members.first().map_or(0, |m| m.id);
        let mut couples = Vec::with_capacity(pairs);
        let shift = pairs / 3 + 5;
        for (at, husband) in men.iter().take(pairs).enumerate() {
            let wife = women[(at + shift) % pairs];
            let family = self.families;
            self.families += 1;
            spouse_of[husband.id - first] = Some(family);
            spouse_of[wife.id - first] = Some(family);
            couples.push(Couple {
                family,
                husband: husband.id,
                wife: wife.id,
                surname: husband.surname,
            });
        }
        for member in members {
            self.person(member, generation, spouse_of[member.id - first])?;
        }
        Ok(couples)
    }
}

/// Write the GEDCOM of a population of `persons` persons to `out`.
///
/// The first generation is as many founder couples as the size asks for;
/// each couple has two or three children, who marry within their
/// generation. The first founder is named [`FOUNDER`], a founder of
/// another couple [`STRANGER`], the last person born [`PROBAND`] and a
/// person of the last generation born far from them [`COUSIN`]. One person in five is born in [`HUB_PLACE`] up to
/// [`HUB_USES`], and [`SMALL_HUB_USES`] in [`SMALL_HUB_PLACE`].
pub fn write_gedcom(out: impl Write, persons: usize) -> io::Result<Population> {
    let mut writer = Writer {
        out,
        bytes: 0,
        target: persons,
        persons: 0,
        families: 0,
    };
    writer.line("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n2 FORM LINEAGE-LINKED\n1 CHAR UTF-8")?;
    for source in 0..=persons / 100 {
        writer.line(&format!("0 @S{source}@ SOUR"))?;
        writer.line(&format!("1 TITL Invented register {source}"))?;
    }

    // Each generation is a quarter larger than the one before: the founders
    // are what leaves `persons` after GENERATIONS of that growth.
    let growth: f64 = (0..GENERATIONS - 1).map(|g| 1.25f64.powi(g as i32)).sum();
    let founders = ((persons as f64 / (2.0 + 2.5 * growth)).ceil() as usize).max(4);
    let mut members: Vec<Member> = (0..(2 * founders).min(persons))
        .map(|id| Member {
            id,
            male: id % 2 == 0,
            surname: id / 2,
            parents: None,
        })
        .collect();
    writer.persons = members.len();

    for generation in 0..GENERATIONS {
        let couples = writer.marry(&members, generation)?;
        let mut children = Vec::new();
        for (at, couple) in couples.iter().enumerate() {
            let wanted = 2 + (at + generation) % 2;
            let born: Vec<usize> = (0..wanted)
                .map_while(|_| {
                    (writer.persons < persons).then(|| {
                        let id = writer.persons;
                        writer.persons += 1;
                        children.push(Member {
                            id,
                            male: id % 2 == 0,
                            surname: couple.surname,
                            parents: Some(couple.family),
                        });
                        id
                    })
                })
                .collect();
            writer.family(couple, generation, &born)?;
        }
        members = children;
        if members.is_empty() {
            break;
        }
    }
    // The last generation born marries nobody.
    for member in &members {
        writer.person(member, GENERATIONS, None)?;
    }
    writer.line("0 TRLR")?;
    writer.out.flush()?;
    Ok(Population {
        persons: writer.persons,
        families: writer.families,
        bytes: writer.bytes,
    })
}
