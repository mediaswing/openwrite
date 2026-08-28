//! `--self-check`: does the program that is about to be published work?
//!
//! This exists for the release workflow. A window cannot be opened on a
//! headless runner, so without this there would be nothing between a broken
//! build and a download — `cargo test` proves the *code* was sound when it was
//! compiled, not that the *executable* being uploaded starts and does its job.
//! The two come apart more easily than they sound: a missing runtime library, a
//! bad link, a stripped binary that faults on the first allocation.
//!
//! So this drives the whole engine — parser, layout, all three renderers — over
//! a screenplay compiled into the binary, and checks the results rather than
//! merely not crashing. It is not a substitute for the test suite and does not
//! try to be; it is the last thing that runs before a release is packaged.
//!
//! It reports through its exit code, because on Windows this is a windowed
//! executable with no console to print to.

use openwrite::layout::{self, LineKind, Options};
use openwrite::render;
use std::process::ExitCode;

/// The sample screenplay, compiled in, so the check depends on nothing sitting
/// next to the binary.
const SAMPLE: &str = include_str!("../examples/sample.fountain");

/// And the sample audio drama, for the same reason.
#[cfg(feature = "drama")]
const DRAMA: &str = include_str!("../examples/sample-drama.xml");

pub fn run() -> ExitCode {
    let mut failures = Vec::new();
    let mut checked = 0;

    let mut check = |name: &str, result: Result<String, String>| {
        checked += 1;
        match result {
            Ok(detail) => println!("ok    {name}: {detail}"),
            Err(why) => {
                println!("FAIL  {name}: {why}");
                failures.push(name.to_string());
            }
        }
    };

    let doc = openwrite::parse(SAMPLE);
    let opts = Options::default();
    let pages = layout::paginate(&doc, &opts);

    check("parse", {
        let scenes = doc
            .elements
            .iter()
            .filter(|e| matches!(e, openwrite::Element::SceneHeading { .. }))
            .count();
        if doc.has_title_page() && scenes == 3 {
            Ok(format!("title page and {scenes} scenes"))
        } else {
            Err(format!("title page {}, {scenes} scenes, expected 3", doc.has_title_page()))
        }
    });

    check("paginate", {
        let script: Vec<_> = pages.iter().filter(|p| !p.is_title_page).collect();
        let over = script.iter().any(|p| p.lines.len() > opts.lines_per_page);
        let blank_first = script.iter().any(|p| p.lines.first().is_some_and(|l| l.is_blank()));
        if script.len() >= 2 && !over && !blank_first {
            Ok(format!("{} pages, none over {} lines", script.len(), opts.lines_per_page))
        } else {
            Err(format!(
                "{} pages, over budget {over}, starts blank {blank_first}",
                script.len()
            ))
        }
    });

    let text = render::text::render(&pages, &opts, false);
    check("format to text", {
        // The columns are the whole point of the program: a cue at 20, dialogue
        // at 10, a transition flush right.
        let cue = text.lines().find(|l| l.trim_start().starts_with("GEORGE"));
        let indent = cue.map(|l| l.len() - l.trim_start().len());
        let transition = text.lines().find(|l| l.contains("CUT TO:"));
        match (indent, transition) {
            (Some(20), Some(t)) if t.len() == opts.width => {
                Ok(format!("{} characters, cues at column 20", text.len()))
            }
            _ => Err(format!("cue indent {indent:?}, transition {transition:?}")),
        }
    });

    check("scene headings survive pagination", {
        let scenes = layout::scene_pages(&pages);
        if scenes.len() == 3 && scenes.iter().all(|(heading, _)| !heading.is_empty()) {
            Ok(format!("{} scenes located", scenes.len()))
        } else {
            Err(format!("{scenes:?}"))
        }
    });

    check("no orphaned scene heading", {
        // The rule the layout engine exists to keep: a slug is never the last
        // thing a reader sees before turning over.
        let mut orphan = None;
        for page in &pages {
            for (i, line) in page.lines.iter().enumerate() {
                if line.kind != LineKind::SceneHeading {
                    continue;
                }
                if page.lines.get(i + 1).map(|l| l.kind) == Some(LineKind::SceneHeading) {
                    continue;
                }
                let following = page.lines[i + 1..].iter().filter(|l| !l.is_blank()).count();
                if following < 2 {
                    orphan = Some(line.to_text());
                }
            }
        }
        match orphan {
            None => Ok("every slug has its scene under it".into()),
            Some(slug) => Err(format!("{slug:?} is stranded at the foot of a page")),
        }
    });

    let html = render::html::render(&doc, &opts);
    check("format to HTML", {
        let landmarks = ["<main id=\"screenplay\"", "class=\"skip-link\"", "role=\"status\""];
        match landmarks.iter().find(|l| !html.contains(**l)) {
            None => Ok(format!("{} bytes, landmarks present", html.len())),
            Some(missing) => Err(format!("missing {missing}")),
        }
    });

    let fdx = render::fdx::render(&doc);
    check("format to Final Draft", {
        let opens = fdx.matches("<Paragraph").count();
        let closes = fdx.matches("</Paragraph>").count();
        if fdx.starts_with("<?xml") && opens == closes && opens > 0 {
            Ok(format!("{opens} paragraphs, balanced"))
        } else {
            Err(format!("{opens} open, {closes} closed"))
        }
    });

    check("the .sct round trip", {
        let saved = openwrite::document::Document {
            source: SAMPLE.to_string(),
            working: openwrite::document::Working { caret: Some(42), scene: Some(1) },
            ..Default::default()
        };
        let reopened = openwrite::document::read(&openwrite::document::write(&saved));
        if reopened.working.caret == Some(42) && reopened.working.scene == Some(1) {
            Ok("a draft reopens where it was left".into())
        } else {
            Err(format!("{:?}", reopened.working))
        }
    });

    check("the story bible round trip", {
        let mut bible = openwrite::bible::Bible {
            world: "A salt lake.\nThe Guild rules by writ.".to_string(),
            ..Default::default()
        };
        let maya = bible.ensure("MAYA");
        maya.want = "Out of this town: it costs more than she has".to_string();
        maya.voice = "Short sentences. Never says what she means.".to_string();

        let saved = openwrite::document::Document {
            source: SAMPLE.to_string(),
            working: openwrite::document::Working { caret: Some(42), scene: Some(1) },
            bible,
        };
        let written = openwrite::document::write(&saved);
        let reopened = openwrite::document::read(&written);
        if reopened == saved {
            Ok("the world and the people come back with the draft".into())
        } else if written.lines().any(|l| l.starts_with("notes:") || l.starts_with("want:")) {
            Err("the header was written but did not read back".into())
        } else {
            Err("the bible did not survive being written".into())
        }
    });

    #[cfg(feature = "ai")]
    check("the model briefing", {
        // No network: this only checks that what would be sent has the world,
        // the character and the page in it.
        let mut bible = openwrite::bible::Bible {
            world: "A salt lake, and a Guild that rules by writ.".to_string(),
            ..Default::default()
        };
        bible.ensure("MAYA").voice = "Short sentences.".to_string();

        let ask = openwrite::ai::prompt::Ask::Say("MAYA".to_string());
        let context = openwrite::ai::Context::gather(SAMPLE, None, &bible, &ask);
        let prompt = context.prompt(&ask);
        let missing: Vec<&str> = [
            ("the world", "salt lake"),
            ("the voice notes", "Short sentences."),
            ("the script", "MAYA"),
        ]
        .iter()
        .filter(|(_, needle)| !prompt.contains(needle))
        .map(|(what, _)| *what)
        .collect();
        if missing.is_empty() {
            Ok(format!("{} characters of briefing", prompt.len()))
        } else {
            Err(format!("the briefing is missing {}", missing.join(", ")))
        }
    });

    // The whole audio path, short of the one part that costs money: read a
    // story, work out what each line asks for, do the arithmetic to some
    // samples and write a `.wav`. Nothing here touches the network — the
    // recording is made here rather than fetched — so this runs on a headless
    // runner like everything else, and catches a build in which the pitch
    // shifting or the mixing has stopped working before it is uploaded.
    #[cfg(feature = "drama")]
    check("the audio drama", {
        use openwrite::drama::{audio, story};

        let drama = story::parse(DRAMA);
        let scared = drama.lines.iter().find(|line| line.state == story::State::Scared);
        let Some(scared) = scared else {
            return finish(checked, &failures, "the sample drama has no frightened line");
        };

        // Half a second of a buzz, standing in for a voice.
        let samples: Vec<f32> = (0..audio::SAMPLE_RATE / 2)
            .map(|i| {
                let t = i as f32 / audio::SAMPLE_RATE as f32;
                (std::f32::consts::TAU * 140.0 * t).sin() * 0.4
            })
            .collect();
        let treatment = audio::Treatment::for_line(scared, 1.0);
        let (left, right) = treatment.apply(&audio::Mono { samples }, 1);
        let piece = audio::Piece { left, right, speaker: "ben".to_string() };
        let mixed = audio::stitch(std::slice::from_ref(&piece));
        let wav = audio::wav(&mixed, audio::SAMPLE_RATE, 2);

        let lifted = treatment.semitones > 3.0;
        let trembles = treatment.wobble > 0.0;
        let panned = piece.left.iter().map(|s| s * s).sum::<f32>()
            > piece.right.iter().map(|s| s * s).sum::<f32>() * 4.0;
        let sound = wav.starts_with(b"RIFF") && wav.len() > 44 && mixed.iter().all(|s| s.is_finite());

        if drama.lines.len() == 5 && lifted && trembles && panned && sound {
            Ok(format!(
                "{} lines, {:+.1} semitones and trembling, {} bytes of audio",
                drama.lines.len(),
                treatment.semitones,
                wav.len()
            ))
        } else {
            Err(format!(
                "{} lines, lifted {lifted}, trembles {trembles}, panned {panned}, sound {sound}",
                drama.lines.len()
            ))
        }
    });

    check("statistics", {
        let stats = openwrite::stats::compute(&doc, &pages);
        if stats.scenes == 3 && stats.words > 100 && stats.characters.contains_key("MAYA") {
            Ok(format!("{} words, {} speakers", stats.words, stats.characters.len()))
        } else {
            Err(format!("{} scenes, {} words", stats.scenes, stats.words))
        }
    });

    println!();
    if failures.is_empty() {
        println!("{checked} checks passed");
        ExitCode::SUCCESS
    } else {
        println!("{} of {checked} checks failed: {}", failures.len(), failures.join(", "));
        ExitCode::FAILURE
    }
}

/// Give up early, having already run some checks. Only reached when a sample
/// compiled into the binary is not what it should be, which is a broken build
/// rather than a failing check.
#[cfg(feature = "drama")]
fn finish(checked: usize, failures: &[String], why: &str) -> ExitCode {
    println!("FAIL  {why}");
    println!();
    println!("{} of {checked} checks failed: {}", failures.len() + 1, failures.join(", "));
    ExitCode::FAILURE
}
