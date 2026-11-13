//! Choosing among files of a project: one named in full, every one a wildcard matches, or those a person picks from a
//! list.
//!
//! What any platform needs chosen this way is chosen here, so a choice reads the same everywhere: on the command line,
//! where the list is said with a number for each, and in the window, which offers the list itself with a search and a
//! box to tick for each, and answers with the names ticked.

use crate::manifest::{is_pattern, wildcard};
use crate::report::{self, Question, QuestionKind, Style, Ui};

/// The options an answer picks, in the list's order: a number of the list, several as 1,3, a name, a part of one,
/// which takes every option that has it in its name, a wildcard, or * for all. Nothing, when it picks none of them.
pub fn picked(answer: &str, options: &[String]) -> Vec<String> {
    let mut found: Vec<&String> = Vec::new();

    for word in answer
        .split(',')
        .map(str::trim)
        .filter(|word| !word.is_empty())
    {
        let lower = word.to_lowercase();
        let matched: Vec<&String> = if word == "*" {
            options.iter().collect()
        } else if let Ok(number) = word.parse::<usize>() {
            number
                .checked_sub(1)
                .and_then(|index| options.get(index))
                .into_iter()
                .collect()
        } else if is_pattern(word) {
            wildcard(word)
                .map(|pattern| {
                    options
                        .iter()
                        .filter(|option| pattern.is_match(option))
                        .collect()
                })
                .unwrap_or_default()
        } else if let Some(exact) = options.iter().find(|option| option.to_lowercase() == lower) {
            vec![exact]
        } else {
            options
                .iter()
                .filter(|option| option.to_lowercase().contains(&lower))
                .collect()
        };

        found.extend(matched);
    }

    options
        .iter()
        .filter(|option| found.contains(option))
        .cloned()
        .collect()
}

/// Ask which of `options` to take, one or several when `many`: `heading` and the list, a number for each, the ways to
/// answer, then `prompt`. The ones picked, in the list's order. None when nobody answers, or the answer picks none of
/// them, so nothing is ever taken by a guess.
pub fn ask(
    heading: &str,
    prompt: &str,
    options: &[String],
    many: bool,
    ui: &mut dyn Ui,
) -> Option<Vec<String>> {
    ui.say(report::blank());
    ui.say(report::note(heading));

    for (index, option) in options.iter().enumerate() {
        ui.say(report::plain(format!("  {:>2}  {option}", index + 1)));
    }

    ui.say(report::note(if many {
        "A number, several as 1,3, a name, a part of one, a wildcard, or * for all. Nothing takes none."
    } else {
        "A number, a name, or a part of one. Nothing takes none."
    }));

    let question = Question {
        prompt: report::line(prompt, Style::Bold),
        kind: QuestionKind::Choose {
            options: options.to_vec(),
            many,
        },
    };

    let chosen = picked(&ui.ask(&question)?, options);

    match chosen.len() {
        0 => None,
        1 => Some(chosen),
        _ if many => Some(chosen),
        // One was asked for, and the answer says several: nothing is taken rather than one of them at random.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> Vec<String> {
        [
            "app.overlay",
            "boards/nrf52840dk_nrf52840.overlay",
            "boards/nrf54l15dk_nrf54l15_cpuapp.overlay",
            "socs/nrf54l15_cpuapp.overlay",
        ]
        .iter()
        .map(ToString::to_string)
        .collect()
    }

    #[test]
    fn a_number_picks_its_line_and_several_pick_several() {
        assert_eq!(picked("2", &options()), [options()[1].clone()]);
        assert_eq!(
            picked("3, 1", &options()),
            [options()[0].clone(), options()[2].clone()]
        );
        assert!(picked("0", &options()).is_empty());
        assert!(picked("9", &options()).is_empty());
    }

    #[test]
    fn a_name_a_part_of_one_a_wildcard_and_a_star_pick_what_they_say() {
        assert_eq!(picked("APP.overlay", &options()), ["app.overlay"]);
        assert_eq!(
            picked("nrf54", &options()),
            [options()[2].clone(), options()[3].clone()]
        );
        assert_eq!(
            picked("boards/*.overlay", &options()),
            [options()[1].clone(), options()[2].clone()]
        );
        assert_eq!(picked("*", &options()), options());
    }

    #[test]
    fn nothing_or_what_matches_nothing_picks_nothing() {
        assert!(picked("", &options()).is_empty());
        assert!(picked(" , ", &options()).is_empty());
        assert!(picked("esp32", &options()).is_empty());
    }

    #[test]
    fn the_window_is_told_the_options_to_offer() {
        let kind = QuestionKind::Choose {
            options: vec!["app.overlay".to_string()],
            many: true,
        };

        assert_eq!(
            serde_json::to_value(&kind).unwrap(),
            serde_json::json!({ "type": "choose", "options": ["app.overlay"], "many": true })
        );
    }

    #[test]
    fn one_asked_for_and_several_given_takes_none() {
        let mut ui = Answers(vec!["1,2".to_string()]);

        assert_eq!(ask("Which?", "One: ", &options(), false, &mut ui), None);
    }

    struct Answers(Vec<String>);

    impl Ui for Answers {
        fn say(&mut self, _line: report::Line) {}

        fn error(&mut self, _message: &str) {}

        fn ask(&mut self, _question: &Question) -> Option<String> {
            (!self.0.is_empty()).then(|| self.0.remove(0))
        }
    }
}
