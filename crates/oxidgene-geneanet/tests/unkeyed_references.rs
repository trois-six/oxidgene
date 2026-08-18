//! Asks whether a media reference that carries no GeneWeb key could be joined
//! by the name it *does* carry.
//!
//! Geneanet's `/media/api/references` usually hands back
//! `reference_extra_geneweb.ref` — the `surname|firstname|occ` triple the whole
//! join rests on. Sometimes it hands back nothing, while still naming the
//! person in `lastname`/`firstname`. On the reference account that is 35 of
//! 550 links, and their owner is certain those people are in the tree.
//!
//! If folding the reference's own name lands on exactly one person, those links
//! are recoverable and the wizard is discarding them for no reason. If it lands
//! on several, they are not — attaching would be a coin toss, which this
//! project does not do. This measures which.
//!
//! `#[ignore]`d and self-skipping: it needs a real `.gw` and a real dump of
//! `/media/api/references`, neither of which is committed.
//!
//! ```text
//! OXIDGENE_GW=/path/tree.gw OXIDGENE_REFERENCES=/path/references \
//!   cargo test -p oxidgene-geneanet --test unkeyed_references -- --ignored --nocapture
//! ```

use std::collections::HashMap;

use oxidgene_geneanet::key::geneanet_key;

#[test]
#[ignore = "needs a real .gw and a references dump; see the module docs"]
fn unkeyed_references_can_be_joined_by_the_name_they_carry() {
    let (Ok(gw), Ok(dir)) = (
        std::env::var("OXIDGENE_GW"),
        std::env::var("OXIDGENE_REFERENCES"),
    ) else {
        eprintln!("OXIDGENE_GW / OXIDGENE_REFERENCES unset — skipping");
        return;
    };

    let bytes = std::fs::read(&gw).expect("reads the .gw");
    let (database, _) = oxidgene_geneanet::parse_gw(&bytes, "tree.gw").expect("parses");

    // Two indexes: the exact triple the join uses, and the name alone across
    // every occurrence — the second is what says whether a namesake exists.
    let mut by_key: HashMap<String, usize> = HashMap::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();
    for person in &database.persons {
        *by_key
            .entry(geneanet_key(
                &person.surname,
                &person.first_name,
                person.occ,
            ))
            .or_default() += 1;
        let name = geneanet_key(&person.surname, &person.first_name, 0);
        *by_name.entry(name).or_default() += 1;
    }
    println!(
        "{} persons, {} distinct keys",
        database.persons.len(),
        by_key.len()
    );

    let mut keyed = 0usize;
    let mut unkeyed = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("reads the references directory") {
        let Ok(entry) = entry else { continue };
        let Ok(body) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(items) = serde_json::from_str::<Vec<serde_json::Value>>(&body) else {
            continue;
        };
        for item in items {
            let has_key = item
                .get("reference_extra_geneweb")
                .and_then(|g| g.get("ref"))
                .and_then(|r| r.as_str())
                .is_some_and(|r| !r.is_empty());
            if has_key {
                keyed += 1;
            } else {
                unkeyed.push((
                    item.get("lastname")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    item.get("firstname")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                ));
            }
        }
    }

    println!("{keyed} references carry a key, {} do not", unkeyed.len());

    let (mut one, mut several, mut none) = (0usize, 0usize, 0usize);
    let mut unmatched = Vec::new();
    for (lastname, firstname) in &unkeyed {
        // Occurrence 0: the reference carries none, and 0 is what a person
        // with no namesake has.
        let key = geneanet_key(lastname, firstname, 0);
        // Unique across *all* occurrences, not just occurrence zero: a
        // reference carries no occurrence, so a namesake makes it a guess.
        match by_name.get(&key).copied().unwrap_or(0) {
            0 => {
                none += 1;
                unmatched.push(key);
            }
            1 => {
                one += 1;
                println!("  RECOVERABLE: {key}");
            }
            _ => several += 1,
        }
    }

    let _ = &by_key;
    println!("folding the name the reference carries (unique across all occurrences):");
    println!("  exactly one person  {one}   <- recoverable");
    println!("  several persons     {several}   <- must stay unattached");
    println!("  no person           {none}");
    if !unmatched.is_empty() {
        println!("  keys that found nobody, e.g.:");
        for key in &unmatched {
            println!("    {key}");
        }
    }
}
