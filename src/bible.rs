//! The story bible: what the writer knows that the script does not say.
//!
//! A screenplay is the tip of a much larger thing. The world has rules the
//! audience never hears stated; a character wants something they would never
//! admit to. None of that is printed, but all of it decides what goes on the
//! page — so the tool keeps it beside the screenplay rather than in a separate
//! file that gets lost.
//!
//! Two parts. [`Bible::world`] is free text: the setting, its rules, the shape
//! of the story. [`Bible::profiles`] is one [`Profile`] per character, keyed by
//! the cue name they speak under, so a profile and the dialogue in the script
//! are talking about the same person.
//!
//! It is all optional, and all plain text. It is saved inside the `.sct`
//! header (see [`crate::document`]), which is why every field here is a
//! `String` rather than something structured: a story bible that refuses a
//! half-finished thought is no use to anybody drafting.

/// What is known about one character.
///
/// Everything except the name may be empty; a profile that is only a name is
/// still worth keeping, because it is somewhere to put the next thought.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    /// The cue name, upper case — the same identity the parser gives dialogue.
    pub name: String,
    /// Who they are in the story: "Salt-runner. The younger sister."
    pub role: String,
    /// Age, however the writer wants to put it: "19", "late fifties", "ageless".
    pub age: String,
    /// What they are after. The engine of every scene they are in.
    pub want: String,
    /// How they talk — the thing a suggested line has to match to be usable.
    pub voice: String,
    /// Background, contradictions, anything else. May run to several lines.
    pub notes: String,
}

impl Profile {
    /// A new, empty profile under a cue name.
    pub fn new(name: impl AsRef<str>) -> Self {
        Profile { name: normalise(name.as_ref()), ..Default::default() }
    }

    /// Has anything been written down beyond the name?
    pub fn is_bare(&self) -> bool {
        [&self.role, &self.age, &self.want, &self.voice, &self.notes]
            .iter()
            .all(|field| field.trim().is_empty())
    }

    /// The profile as a short briefing, for a reader who needs to be told who
    /// this is in as few words as possible.
    ///
    /// Empty fields are left out rather than printed empty: a heading with
    /// nothing under it reads as "this is unknown", which is not the same as
    /// "this does not matter".
    pub fn brief(&self) -> String {
        let mut out = self.name.clone();
        if !self.age.trim().is_empty() {
            out.push_str(&format!(" ({})", self.age.trim()));
        }
        out.push('\n');
        for (label, value) in [
            ("Role", &self.role),
            ("Wants", &self.want),
            ("Voice", &self.voice),
            ("Notes", &self.notes),
        ] {
            let value = value.trim();
            if !value.is_empty() {
                out.push_str(&format!("  {label}: {value}\n"));
            }
        }
        out
    }
}

/// The world and the people in it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bible {
    /// The setting and its rules, in the writer's own words.
    pub world: String,
    /// Character profiles, in the order the writer made them.
    pub profiles: Vec<Profile>,
}

impl Bible {
    /// Nothing written down yet.
    pub fn is_empty(&self) -> bool {
        self.world.trim().is_empty() && self.profiles.is_empty()
    }

    /// Find a profile by cue name, however it was typed.
    pub fn get(&self, name: &str) -> Option<&Profile> {
        let name = normalise(name);
        self.profiles.iter().find(|p| p.name == name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Profile> {
        let name = normalise(name);
        self.profiles.iter_mut().find(|p| p.name == name)
    }

    /// The index of a profile by cue name.
    pub fn position(&self, name: &str) -> Option<usize> {
        let name = normalise(name);
        self.profiles.iter().position(|p| p.name == name)
    }

    /// Add a profile for a character, or return the one already there.
    ///
    /// Adding a character who is already in the bible must not wipe what was
    /// written about them, which is why this returns the existing profile
    /// rather than replacing it.
    pub fn ensure(&mut self, name: &str) -> &mut Profile {
        let name = normalise(name);
        match self.profiles.iter().position(|p| p.name == name) {
            Some(i) => &mut self.profiles[i],
            None => {
                self.profiles.push(Profile::new(&name));
                self.profiles.last_mut().expect("just pushed")
            }
        }
    }

    /// Remove a profile. Returns whether there was one.
    pub fn remove(&mut self, name: &str) -> bool {
        let name = normalise(name);
        match self.profiles.iter().position(|p| p.name == name) {
            Some(i) => {
                self.profiles.remove(i);
                true
            }
            None => false,
        }
    }

    /// Every profile as a briefing, for handing to a reader who knows nothing.
    pub fn brief(&self) -> String {
        self.profiles.iter().map(Profile::brief).collect::<Vec<_>>().join("\n")
    }

    /// Names in the script that have no profile yet.
    ///
    /// The bible is meant to be filled in from the screenplay rather than
    /// typed twice, so the interesting question is always "who is speaking that
    /// I have not thought about".
    pub fn unprofiled<'a>(&self, speaking: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        let mut out: Vec<String> = speaking
            .into_iter()
            .map(normalise)
            .filter(|name| !name.is_empty() && self.get(name).is_none())
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

/// A cue name as the parser would see it: trimmed and upper case, so that
/// "Maya", "MAYA" and " maya " are one person.
pub fn normalise(name: &str) -> String {
    name.trim().to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_character_is_the_same_character_however_the_name_is_typed() {
        let mut bible = Bible::default();
        bible.ensure("Maya").want = "Out".to_string();
        assert_eq!(bible.get("MAYA").map(|p| p.want.as_str()), Some("Out"));
        assert_eq!(bible.get(" maya ").map(|p| p.want.as_str()), Some("Out"));
        assert_eq!(bible.profiles.len(), 1);
    }

    #[test]
    fn adding_a_character_twice_does_not_lose_what_was_written() {
        let mut bible = Bible::default();
        bible.ensure("MAYA").voice = "Clipped".to_string();
        bible.ensure("MAYA");
        assert_eq!(bible.profiles.len(), 1);
        assert_eq!(bible.get("MAYA").unwrap().voice, "Clipped");
    }

    #[test]
    fn removing_says_whether_there_was_anything_to_remove() {
        let mut bible = Bible::default();
        bible.ensure("DEV");
        assert!(bible.remove("dev"));
        assert!(!bible.remove("dev"));
        assert!(bible.profiles.is_empty());
    }

    #[test]
    fn a_briefing_leaves_out_what_has_not_been_decided() {
        let profile = Profile {
            name: "MAYA".to_string(),
            want: "To buy her brother out".to_string(),
            ..Profile::new("MAYA")
        };
        let brief = profile.brief();
        assert!(brief.contains("MAYA"));
        assert!(brief.contains("Wants: To buy her brother out"));
        assert!(!brief.contains("Voice:"));
        assert!(!brief.contains("()"));
    }

    #[test]
    fn the_unprofiled_are_the_ones_worth_asking_about() {
        let mut bible = Bible::default();
        bible.ensure("MAYA");
        let missing = bible.unprofiled(["MAYA", "DEV", "dev", "", "  "]);
        assert_eq!(missing, vec!["DEV".to_string()]);
    }

    #[test]
    fn a_profile_with_only_a_name_knows_it_is_bare() {
        assert!(Profile::new("MAYA").is_bare());
        let mut profile = Profile::new("MAYA");
        profile.notes = "Cannot swim.".to_string();
        assert!(!profile.is_bare());
    }
}
