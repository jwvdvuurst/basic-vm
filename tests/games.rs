//! Integration tests for the text games in `examples/games/`.
//!
//! The games are interactive, so every test spawns the built `basic-vm`
//! binary with a fixed seed argument and answers each prompt (a transcript
//! ending in `"> "`) from a script or from a small strategy closure. A
//! watchdog kills the child (by handle, never by name) after 20 seconds, so a
//! hanging game fails the test instead of blocking the suite.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(20);

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

struct Outcome {
    stdout: String,
    stderr: String,
    success: bool,
}

impl Outcome {
    /// The run exited with status 0 and wrote nothing to stderr; the game did
    /// not leak an interpreter error such as "unexpected end of input".
    fn assert_clean(&self) {
        assert!(
            self.success,
            "non-zero exit.\nstderr: {}\nstdout:\n{}",
            self.stderr, self.stdout
        );
        assert!(self.stderr.is_empty(), "stderr: {}", self.stderr);
        for bad in ["unexpected end of input", "panicked", "basic-vm:"] {
            assert!(
                !self.stdout.contains(bad),
                "stdout mentions {bad:?}:\n{}",
                self.stdout
            );
        }
    }

    fn assert_contains(&self, needle: &str) {
        assert!(
            self.stdout.contains(needle),
            "missing {needle:?} in:\n{}",
            self.stdout
        );
    }

    fn assert_lacks(&self, needle: &str) {
        assert!(
            !self.stdout.contains(needle),
            "unexpected {needle:?} in:\n{}",
            self.stdout
        );
    }

    fn count(&self, needle: &str) -> usize {
        self.stdout.matches(needle).count()
    }
}

fn games_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("games")
}

/// Every `*.bas` file in `examples/games`, sorted by name.
fn game_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(games_dir())
        .expect("examples/games exists")
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "bas"))
        .collect();
    files.sort();
    files
}

/// Runs a game. Whenever the transcript ends in a prompt (`"> "`) the
/// responder sees the whole transcript and returns the line to type, or
/// `None` to close stdin (end of input).
fn play(game: &str, args: &[&str], mut responder: impl FnMut(&str) -> Option<String>) -> Outcome {
    let path = games_dir().join(game);
    let mut child = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg(&path)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn basic-vm");
    let mut stdin = child.stdin.take();
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");

    let (sender, receiver) = mpsc::channel::<Vec<u8>>();
    let stdout_reader = thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        loop {
            match stdout.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if sender.send(buffer[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let stderr_reader = thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });

    let deadline = Instant::now() + TIMEOUT;
    let mut transcript = String::new();
    let mut answered = 0;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{game} timed out after {TIMEOUT:?}; transcript so far:\n{transcript}");
        }
        match receiver.recv_timeout(remaining.min(Duration::from_millis(50))) {
            Ok(chunk) => transcript.push_str(&String::from_utf8_lossy(&chunk)),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if transcript.len() > answered && transcript.ends_with("> ") {
            answered = transcript.len();
            match responder(&transcript) {
                Some(line) => {
                    if let Some(pipe) = stdin.as_mut() {
                        // The game may already be gone; that is checked below.
                        let _ = writeln!(pipe, "{line}");
                        let _ = pipe.flush();
                    }
                }
                None => stdin = None,
            }
        }
    }
    drop(stdin);
    let status = child.wait().expect("wait for basic-vm");
    stdout_reader.join().expect("stdout reader");
    let stderr = stderr_reader.join().expect("stderr reader");
    Outcome {
        stdout: transcript,
        stderr,
        success: status.success(),
    }
}

/// A responder that types the given lines in order, then ends the input.
fn script(lines: &[&str]) -> impl FnMut(&str) -> Option<String> {
    let mut queue: VecDeque<String> = lines.iter().map(|line| line.to_string()).collect();
    move |_| queue.pop_front()
}

/// The integer written right after the last occurrence of `label`.
fn number_after(text: &str, label: &str) -> Option<i64> {
    let at = text.rfind(label)?;
    let rest = text[at + label.len()..].trim_start();
    let digits: String = rest
        .chars()
        .enumerate()
        .take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && *c == '-'))
        .map(|(_, c)| c)
        .collect();
    digits.parse().ok()
}

/// The word that follows the last occurrence of `label`, up to a `.`.
fn word_after(text: &str, label: &str) -> String {
    let at = text
        .rfind(label)
        .unwrap_or_else(|| panic!("{label:?} not found in:\n{text}"));
    text[at + label.len()..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect()
}

fn all_numbers(text: &str) -> Vec<usize> {
    text.split(|c: char| !c.is_ascii_digit())
        .filter(|piece| !piece.is_empty())
        .map(|piece| piece.parse().expect("digits"))
        .collect()
}

// ---------------------------------------------------------------------------
// Every game: unit tests, graceful end of input
// ---------------------------------------------------------------------------

#[test]
fn all_five_games_are_present() {
    let names: Vec<String> = game_files()
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    for expected in [
        "hamurabi.bas",
        "hangman.bas",
        "lander.bas",
        "mastermind.bas",
        "wumpus.bas",
    ] {
        assert!(names.iter().any(|name| name == expected), "{expected}");
    }
}

#[test]
fn every_game_passes_its_own_test_suite() {
    for path in game_files() {
        let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
            .arg("--test")
            .arg(&path)
            .stdin(Stdio::null())
            .output()
            .expect("run basic-vm --test");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{path:?}:\n{stdout}\n{stderr}");
        assert!(
            stdout.contains(" 0 failed, 0 errors"),
            "{path:?}:\n{stdout}"
        );
        let summary = stdout.lines().last().unwrap_or("");
        let count = all_numbers(summary).first().copied().unwrap_or(0);
        assert!(count >= 6, "{path:?} has only {count} tests:\n{stdout}");
        assert_eq!(stdout.matches("FAIL").count(), 0, "{path:?}:\n{stdout}");
    }
}

#[test]
fn every_game_ends_gracefully_when_input_ends_immediately() {
    for path in game_files() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let run = play(&name, &["7"], |_| None);
        run.assert_clean();
        assert!(
            run.stdout.trim_end().ends_with(|c: char| c != '>'),
            "{name} should finish with a closing message:\n{}",
            run.stdout
        );
        assert!(run.stdout.contains("> "), "{name} never prompted");
    }
}

#[test]
fn games_without_a_seed_argument_print_the_seed_they_chose() {
    for name in [
        "hamurabi.bas",
        "wumpus.bas",
        "lander.bas",
        "hangman.bas",
        "mastermind.bas",
    ] {
        let run = play(name, &[], |_| None);
        run.assert_clean();
        run.assert_contains("(Seed ");
        let seeded = play(name, &["7"], |_| None);
        seeded.assert_lacks("(Seed ");
    }
}

#[test]
fn the_same_seed_gives_the_same_game() {
    for name in [
        "hamurabi.bas",
        "wumpus.bas",
        "lander.bas",
        "hangman.bas",
        "mastermind.bas",
    ] {
        let first = play(name, &["12345"], |_| None);
        let second = play(name, &["12345"], |_| None);
        assert_eq!(first.stdout, second.stdout, "{name}");
    }
}

// ---------------------------------------------------------------------------
// Hamurabi
// ---------------------------------------------------------------------------

/// Feeds everybody (selling land when the granary is too small to do so),
/// plants as much as people and seed allow.
fn careful_ruler() -> impl FnMut(&str) -> Option<String> {
    let mut trade = 0i64;
    move |transcript| {
        let people = number_after(transcript, "Population:")?;
        let price = number_after(transcript, "Land costs ")?;
        let acres = number_after(transcript, "Land:")?;
        let bushels = number_after(transcript, "Granary:")?;
        if transcript.ends_with("none)> ") {
            let shortfall = (people * 20 - bushels).max(0);
            let to_sell = (shortfall + price - 1) / price;
            assert!(to_sell < acres, "not enough land to sell:\n{transcript}");
            trade = -to_sell;
            Some(trade.to_string())
        } else if transcript.ends_with("Bushels to feed your people> ") {
            Some((people * 20).to_string())
        } else if transcript.ends_with("Acres to plant> ") {
            let seed = bushels - trade * price - people * 20;
            Some((acres + trade).min(people * 10).min(seed).to_string())
        } else {
            None
        }
    }
}

#[test]
fn hamurabi_ten_careful_years_reach_the_final_rating() {
    let run = play("hamurabi.bas", &["1"], careful_ruler());
    run.assert_clean();
    assert_eq!(run.count("=== Year "), 10, "{}", run.stdout);
    run.assert_contains("=== Year 10 of 10 ===");
    run.assert_contains("Your ten years are over.");
    run.assert_lacks("council throws you out");
    let verdicts = ["Disaster.", "Poor.", "Fair.", "Good.", "Excellent."];
    assert!(
        verdicts.iter().any(|verdict| run.stdout.contains(verdict)),
        "no rating in:\n{}",
        run.stdout
    );
}

#[test]
fn hamurabi_rejects_amounts_the_player_does_not_have() {
    // Start: 2800 bushels, 1000 acres, 100 people.
    let run = play(
        "hamurabi.bas",
        &["1"],
        script(&[
            "999999", // trade: far more land than the grain can buy
            "-5000",  // trade: more acres than owned
            "zebra",  // trade: not a number
            "0",      // trade nothing
            "99999",  // feed: more than the granary
            "-1",     // feed: negative
            "2000",   // feed: everybody
            "5000",   // plant: more acres than owned
            "-3",     // plant: negative
            "900",    // plant: more seed than the 800 bushels left
            "800",    // plant: fine
        ]),
    );
    run.assert_clean();
    run.assert_contains("You only have 2800 bushels");
    run.assert_contains("only own 1000 acres");
    run.assert_contains("Please enter a whole number.");
    run.assert_contains("cannot plant a negative number of acres");
    run.assert_contains("You only have 800 bushels of seed grain left.");
    run.assert_contains("=== Year 2 of 10 ===");
}

#[test]
fn hamurabi_starving_everyone_gets_the_ruler_impeached() {
    let run = play("hamurabi.bas", &["1"], |_| Some("0".into()));
    run.assert_clean();
    run.assert_contains("The council throws you out of office");
    run.assert_lacks("Your ten years are over.");
    assert_eq!(run.count("=== Year "), 1);
}

#[test]
fn hamurabi_input_ending_mid_reign_is_handled() {
    let run = play("hamurabi.bas", &["1"], script(&["0", "2000"]));
    run.assert_clean();
    run.assert_contains("The input ended; the reign is abandoned.");
}

// ---------------------------------------------------------------------------
// Wumpus
// ---------------------------------------------------------------------------

/// The dodecahedron, built independently of the BASIC program: five rooms in
/// each of four rings (1-5, 6-10, 11-15, 16-20).
fn cave_neighbours(room: usize) -> Vec<usize> {
    let ring_start = |base: usize, i: i64| base + (i + 5).rem_euclid(5) as usize;
    let ring = (room - 1) / 5;
    let i = ((room - 1) % 5) as i64;
    let mut near = match ring {
        0 => vec![ring_start(1, i - 1), ring_start(1, i + 1), ring_start(6, i)],
        1 => vec![ring_start(1, i), ring_start(11, i), ring_start(11, i - 1)],
        2 => vec![ring_start(6, i), ring_start(6, i + 1), ring_start(16, i)],
        _ => vec![
            ring_start(16, i - 1),
            ring_start(16, i + 1),
            ring_start(11, i),
        ],
    };
    near.sort_unstable();
    near
}

#[test]
fn the_reference_cave_is_a_symmetric_three_regular_graph() {
    for room in 1..=20 {
        let near = cave_neighbours(room);
        assert_eq!(near.len(), 3);
        assert!(near.windows(2).all(|pair| pair[0] < pair[1]), "{near:?}");
        for other in near {
            assert!(cave_neighbours(other).contains(&room), "{room} <-> {other}");
        }
    }
}

/// Shortest path from `from` to `to` that never enters a `blocked` room
/// (other than `to` itself), excluding the starting room.
fn route(from: usize, to: usize, blocked: &[usize]) -> Option<Vec<usize>> {
    if from == to {
        return Some(Vec::new());
    }
    let mut previous = [0usize; 21];
    let mut seen = [false; 21];
    let mut queue = VecDeque::from([from]);
    seen[from] = true;
    while let Some(room) = queue.pop_front() {
        if room == to {
            let mut path = vec![room];
            let mut at = room;
            while previous[at] != from {
                at = previous[at];
                path.push(at);
            }
            path.reverse();
            return Some(path);
        }
        for next in cave_neighbours(room) {
            if !seen[next] && (next == to || !blocked.contains(&next)) {
                seen[next] = true;
                previous[next] = room;
                queue.push_back(next);
            }
        }
    }
    None
}

#[derive(Clone, Copy)]
enum Goal {
    Pit,
    Bats,
    SlayWumpus,
}

/// Reads the player's room and (thanks to the `reveal` argument) the hazard
/// rooms from the transcript and walks towards the goal.
fn hunter(goal: Goal) -> impl FnMut(&str) -> Option<String> {
    move |transcript| {
        let here = number_after(transcript, "You are in room ")? as usize;
        let reveal = transcript
            .lines()
            .rev()
            .find(|line| line.starts_with("[reveal]"))?;
        let facts = all_numbers(reveal);
        let (wumpus, pits, bats) = (facts[0], &facts[1..3], &facts[3..5]);
        let hazards: Vec<usize> = std::iter::once(wumpus)
            .chain(pits.iter().copied())
            .chain(bats.iter().copied())
            .collect();
        let candidates: Vec<usize> = match goal {
            Goal::Pit => pits.to_vec(),
            Goal::Bats => {
                if transcript.contains("Giant bats seize you") {
                    return None;
                }
                bats.to_vec()
            }
            Goal::SlayWumpus => {
                if cave_neighbours(here).contains(&wumpus) {
                    return Some(format!("S {wumpus}"));
                }
                cave_neighbours(wumpus)
                    .into_iter()
                    .filter(|room| !hazards.contains(room))
                    .collect()
            }
        };
        let best = candidates
            .iter()
            .filter_map(|&target| route(here, target, &hazards))
            .min_by_key(Vec::len)
            .expect("a hazard-free route to the goal exists for this seed");
        Some(format!("M {}", best[0]))
    }
}

#[test]
fn wumpus_tunnels_match_the_dodecahedron() {
    let run = play("wumpus.bas", &["3", "reveal"], hunter(Goal::SlayWumpus));
    run.assert_clean();
    let mut room = 0;
    let mut checked = 0;
    for line in run.stdout.lines() {
        if let Some(rest) = line.strip_prefix("You are in room ") {
            room = all_numbers(rest)[0];
        } else if line.starts_with("Tunnels lead to rooms") {
            assert_eq!(all_numbers(line), cave_neighbours(room), "room {room}");
            checked += 1;
        }
    }
    assert!(checked >= 2, "{}", run.stdout);
}

#[test]
fn wumpus_walking_into_a_pit_loses() {
    for seed in ["1", "2", "3", "4"] {
        let run = play("wumpus.bas", &[seed, "reveal"], hunter(Goal::Pit));
        run.assert_clean();
        run.assert_contains("bottomless pit");
        run.assert_contains("You lose.");
        run.assert_lacks("You win!");
        run.assert_contains("cold draught"); // the pit was announced on the way
    }
}

#[test]
fn wumpus_flying_into_bats_relocates_the_hunter() {
    for seed in ["1", "2", "3", "4"] {
        let run = play("wumpus.bas", &[seed, "reveal"], hunter(Goal::Bats));
        run.assert_clean();
        run.assert_contains("Giant bats seize you and drop you in room");
        run.assert_contains("leathery wings"); // warned beforehand
    }
}

#[test]
fn wumpus_shooting_the_wumpus_from_next_door_wins() {
    for seed in ["1", "2", "3", "4"] {
        let run = play("wumpus.bas", &[seed, "reveal"], hunter(Goal::SlayWumpus));
        run.assert_clean();
        run.assert_contains("foul stench"); // the wumpus was smelled on the way
        run.assert_contains("Your arrow finds the wumpus");
        run.assert_contains("You win!");
        run.assert_lacks("You lose.");
    }
}

#[test]
fn wumpus_rejects_bad_commands_and_unconnected_rooms() {
    let mut step = 0;
    let run = play("wumpus.bas", &["3"], move |transcript| {
        step += 1;
        let here = number_after(transcript, "You are in room ")? as usize;
        let far = (1..=20)
            .find(|room| *room != here && !cave_neighbours(here).contains(room))
            .unwrap();
        match step {
            1 => Some("M 99".into()),
            2 => Some("dance".into()),
            3 => Some("S 1 2 3 4 5 6".into()),
            4 => Some(format!("M {far}")),
            5 => Some("H".into()),
            6 => Some("".into()),
            7 => Some("Q".into()),
            _ => None,
        }
    });
    run.assert_clean();
    run.assert_contains("'99' is not a room number from 1 to 20.");
    run.assert_contains("I do not understand 'DANCE'");
    run.assert_contains("An arrow can only travel through 5 rooms.");
    run.assert_contains("is not connected to room");
    run.assert_contains("walk to a connected room");
    run.assert_contains("Type M and a room");
    run.assert_contains("You abandon the hunt. The wumpus lurked in room");
}

#[test]
fn wumpus_missing_arrows_use_up_the_quiver() {
    // Shooting at a room that is never next to the hunter makes arrows fly
    // randomly; five misses (or an unlucky self-hit) end the game.
    let run = play("wumpus.bas", &["3"], |_| Some("S 1 1 1 1 1".into()));
    run.assert_clean();
    assert!(
        run.stdout.contains("You lose.") || run.stdout.contains("You win!"),
        "{}",
        run.stdout
    );
    assert!(run.count("Arrows left:") <= 5, "{}", run.stdout);
}

// ---------------------------------------------------------------------------
// Lunar lander (seed 3: start at 1600 m, falling at 32 m/s, 400 kg of fuel)
// ---------------------------------------------------------------------------

fn lander_pilot(burns: &'static [&'static str]) -> impl FnMut(&str) -> Option<String> {
    let mut queue: VecDeque<&str> = burns.iter().copied().collect();
    move |transcript| {
        if transcript.ends_with("Fly again? (y/n)> ") {
            Some("n".into())
        } else {
            queue.pop_front().map(str::to_string)
        }
    }
}

#[test]
fn lander_zero_burn_crashes() {
    let run = play("lander.bas", &["3"], |t| {
        Some(
            if t.ends_with("Fly again? (y/n)> ") {
                "n"
            } else {
                "0"
            }
            .into(),
        )
    });
    run.assert_clean();
    run.assert_contains("Altitude 1600 m");
    run.assert_contains("CRASH.");
    run.assert_lacks("Perfect landing");
    run.assert_contains("with 400 kg of fuel left.");
    run.assert_contains("Thanks for flying. Rounds played: 1");
}

#[test]
fn lander_full_burn_runs_dry_and_crashes() {
    // Always ask for the largest burn the prompt allows.
    let run = play("lander.bas", &["3"], |t| {
        if t.ends_with("Fly again? (y/n)> ") {
            return Some("n".into());
        }
        let start = t.rfind("(0 to ")? + "(0 to ".len();
        let end = start + t[start..].find(')')?;
        Some(t[start..end].to_string())
    });
    run.assert_clean();
    run.assert_contains("Out of fuel - the engine is dead");
    run.assert_contains("CRASH.");
    run.assert_contains("with 0 kg of fuel left.");
}

#[test]
fn lander_a_planned_burn_schedule_lands_softly() {
    let run = play(
        "lander.bas",
        &["3"],
        lander_pilot(&["0", "0", "10", "10", "1.8", "4.0", "n"]),
    );
    run.assert_clean();
    run.assert_contains("Touchdown after 58.2 s at 0.89 m/s with 149.3 kg of fuel left.");
    run.assert_contains("Perfect landing!");
    run.assert_lacks("CRASH.");
}

#[test]
fn lander_validates_burn_rates() {
    let run = play(
        "lander.bas",
        &["3"],
        lander_pilot(&["abc", "11", "-1", "0"]),
    );
    run.assert_clean();
    run.assert_contains("Please enter a number.");
    run.assert_contains("The engine cannot burn more than 10 kg/s.");
    run.assert_contains("The engine cannot burn a negative amount.");
    run.assert_contains("The input ended; the mission is abandoned.");
    // The three bad entries did not use up a turn.
    run.assert_lacks("Time 20 s");
}

#[test]
fn lander_can_be_flown_again() {
    let mut answers = VecDeque::from(["y".to_string()]);
    let run = play("lander.bas", &["3"], |t| {
        if t.ends_with("Fly again? (y/n)> ") {
            answers.pop_front()
        } else {
            Some("0".into())
        }
    });
    run.assert_clean();
    assert_eq!(run.count("CRASH."), 2, "{}", run.stdout);
    run.assert_contains("Rounds played: 2");
}

// ---------------------------------------------------------------------------
// Hangman (seed 4)
// ---------------------------------------------------------------------------

fn alphabet_guesser() -> impl FnMut(&str) -> Option<String> {
    let mut letters = ('A'..='Z').map(|c| c.to_string());
    move |t| {
        if t.ends_with("Play again? (y/n)> ") {
            Some("n".into())
        } else {
            letters.next()
        }
    }
}

#[test]
fn hangman_guessing_the_alphabet_in_order_loses_and_reveals_the_word() {
    let run = play("hangman.bas", &["4"], alphabet_guesser());
    run.assert_clean();
    run.assert_contains("You lose. The word was ");
    run.assert_contains("Wrong guesses: 6 of 6");
    run.assert_contains(" / \\  |"); // the finished picture
    run.assert_contains("Games won: 0 of 1");
    run.assert_lacks("You win!");
}

#[test]
fn hangman_guessing_the_right_letters_wins_and_handles_bad_guesses() {
    let word = word_after(
        &play("hangman.bas", &["4"], alphabet_guesser()).stdout,
        "The word was ",
    );
    let mut letters: Vec<char> = Vec::new();
    for letter in word.chars() {
        if !letters.contains(&letter) {
            letters.push(letter);
        }
    }
    let mut queue: VecDeque<String> = VecDeque::new();
    queue.push_back("7".into()); // not a letter
    queue.push_back(letters[0].to_string());
    queue.push_back(letters[0].to_string()); // repeated guess
    queue.extend(letters[1..].iter().map(|c| c.to_string()));
    queue.push_back("n".into());
    let run = play("hangman.bas", &["4"], move |_| queue.pop_front());
    run.assert_clean();
    run.assert_contains("Only the letters A-Z are allowed.");
    run.assert_contains(&format!("You already tried {}.", letters[0]));
    run.assert_contains(&format!("You win! The word is {word}."));
    run.assert_contains("Wrong guesses: 0 of 6");
    run.assert_contains("Games won: 1 of 1");
    run.assert_lacks("You lose.");
}

#[test]
fn hangman_a_whole_word_guess_and_a_second_round() {
    let word = word_after(
        &play("hangman.bas", &["4"], alphabet_guesser()).stdout,
        "The word was ",
    );
    let mut queue: VecDeque<String> = VecDeque::from([word.to_lowercase(), "y".to_string()]);
    let run = play("hangman.bas", &["4"], move |_| queue.pop_front());
    run.assert_clean();
    run.assert_contains(&format!("You win! The word is {word}."));
    assert_eq!(run.count("I have chosen a word of"), 2, "{}", run.stdout);
    run.assert_contains("The input ended; the game is abandoned.");
    run.assert_contains("Games won: 1 of 2");
}

// ---------------------------------------------------------------------------
// Mastermind (seed 5)
// ---------------------------------------------------------------------------

/// Reference scoring, written independently of the BASIC program.
fn score(secret: &str, guess: &str) -> (usize, usize) {
    let secret: Vec<char> = secret.chars().collect();
    let guess: Vec<char> = guess.chars().collect();
    let exact = secret.iter().zip(&guess).filter(|(a, b)| a == b).count();
    let common: usize = "ABCDEF"
        .chars()
        .map(|letter| {
            let in_secret = secret.iter().filter(|&&c| c == letter).count();
            let in_guess = guess.iter().filter(|&&c| c == letter).count();
            in_secret.min(in_guess)
        })
        .sum();
    (exact, common - exact)
}

fn reveal_secret(args: &[&str]) -> String {
    let run = play("mastermind.bas", args, script(&["q", "n"]));
    run.assert_clean();
    run.assert_contains("You give up. The code was ");
    let code = word_after(&run.stdout, "The code was ");
    assert_eq!(code.len(), 4, "{code}");
    code
}

#[test]
fn mastermind_feedback_matches_a_reference_scorer() {
    let secret = reveal_secret(&["5"]);
    let guesses = ["AABB", "abcd", "c c d d", "FFFF", "ABCA", "DDDD", "EFAB"];
    assert!(
        !guesses
            .iter()
            .any(|g| g.replace(' ', "").to_uppercase() == secret)
    );
    let mut lines: Vec<&str> = guesses.to_vec();
    lines.push("q");
    lines.push("n");
    let run = play("mastermind.bas", &["5"], script(&lines));
    run.assert_clean();
    for guess in guesses {
        let normal = guess.replace(' ', "").to_uppercase();
        let (exact, misplaced) = score(&secret, &normal);
        run.assert_contains(&format!("{normal}: {exact} exact, {misplaced} misplaced"));
    }
    run.assert_contains("Attempt 8 of 10> You give up.");
}

#[test]
fn mastermind_cracking_the_code_wins() {
    let secret = reveal_secret(&["5"]);
    let run = play("mastermind.bas", &["5"], script(&[&secret, "n"]));
    run.assert_clean();
    run.assert_contains(&format!("{secret}: 4 exact, 0 misplaced"));
    run.assert_contains("You cracked the code in 1 attempts!");
    run.assert_contains("Codes cracked: 1 of 1");
}

#[test]
fn mastermind_runs_out_of_attempts_and_reveals_the_code() {
    let secret = reveal_secret(&["5"]);
    // Invalid lines must not use up attempts; ten valid wrong guesses do.
    let wrong: Vec<String> = ["AAAA", "BBBB", "CCCC", "DDDD", "EEEE"]
        .iter()
        .flat_map(|code| [code.to_string(), format!("{}A", &code[..3])])
        .filter(|code| *code != secret)
        .collect();
    assert!(wrong.len() >= 10, "{wrong:?}");
    let mut lines: Vec<&str> = vec!["ZZZZ", "abc", ""];
    lines.extend(wrong.iter().take(10).map(String::as_str));
    lines.push("n");
    let run = play("mastermind.bas", &["5"], script(&lines));
    run.assert_clean();
    run.assert_contains("'Z' is not one of the letters ABCDEF.");
    run.assert_contains("A guess is exactly 4 letters from A to F.");
    run.assert_contains("Attempt 10 of 10> ");
    run.assert_contains(&format!("Out of attempts. The code was {secret}."));
    run.assert_contains("Codes cracked: 0 of 1");
    run.assert_lacks("Attempt 11");
}

#[test]
fn mastermind_distinct_mode_has_no_repeated_letters() {
    for seed in ["1", "2", "3", "4", "5", "6"] {
        let secret = reveal_secret(&[seed, "distinct"]);
        let mut sorted: Vec<char> = secret.chars().collect();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 4, "seed {seed}: {secret}");
    }
    let run = play("mastermind.bas", &["5", "distinct"], script(&["AABB"]));
    run.assert_clean();
    run.assert_contains("In this game every letter appears only once.");
    run.assert_contains("No letter appears twice in the code.");
}

#[test]
fn mastermind_input_ending_mid_game_reveals_the_code() {
    let secret = reveal_secret(&["5"]);
    let run = play("mastermind.bas", &["5"], script(&["AAAA"]));
    run.assert_clean();
    run.assert_contains(&format!(
        "The input ended; the game is abandoned. The code was {secret}."
    ));
}
