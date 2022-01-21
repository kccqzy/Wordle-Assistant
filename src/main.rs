use arrayvec::ArrayVec;
use bit_iter::BitIter;
use std::cmp::Ordering;
use std::fmt::Display;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;

#[derive(Debug, Clone, Copy)]
struct Word([u8; 5]);

impl Display for Word {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = std::str::from_utf8(&self.0).unwrap();
        s.fmt(f)
    }
}

impl TryFrom<&str> for Word {
    type Error = &'static str;
    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        let bytes = value.as_bytes();
        if !bytes.is_ascii() {
            return Err("not an ASCII word");
        }
        let mut word: [u8; 5] = bytes.try_into().map_err(|_| "not a five-letter word")?;
        word.make_ascii_lowercase();
        if !word.iter().all(|&c| (b'a'..=b'z').contains(&c)) {
            return Err("not all ASCII letters");
        }
        Ok(Word(word))
    }
}

type Result<T> = std::result::Result<T, String>;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuessLetterResult {
    Green,
    Yellow,
    Black,
}

type GuessWordResult = [GuessLetterResult; 5];

#[derive(Debug, Clone, PartialEq, Eq)]
struct LetterCount(std::ops::Range<u8>); // We purposefully use a half-open range. It makes the below code simpler.

impl Default for LetterCount {
    fn default() -> Self {
        // By default, we have no information about how many letters there are. So the range is [0, 6).
        LetterCount(0u8..6)
    }
}

#[derive(Clone)]
struct GuessState {
    letter_choices: [u32; 5],
    letter_counts: [LetterCount; 26],
}

impl Default for GuessState {
    fn default() -> Self {
        GuessState { letter_choices: [(1 << 26) - 1; 5], letter_counts: Default::default() }
    }
}

impl std::fmt::Debug for GuessState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn set_to_str(s: u32) -> String {
            BitIter::from(s).map(|c| (c as u8 + b'a') as char).collect()
        }

        struct LetterCountsFormatter<'a>(&'a [LetterCount; 26]);

        impl<'a> std::fmt::Debug for LetterCountsFormatter<'a> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_map()
                    .entries(
                        self.0
                            .iter()
                            .enumerate()
                            .filter(|(_, lc)| **lc != LetterCount::default())
                            .map(|(i, lc)| ((i as u8 + b'a') as char, lc)),
                    )
                    .finish()
            }
        }

        f.debug_struct("GuessState")
            .field(
                "letter_choices",
                &self.letter_choices.iter().copied().map(set_to_str).collect::<Vec<String>>(),
            )
            .field("letter_counts", &LetterCountsFormatter(&self.letter_counts))
            .finish()
    }
}

impl GuessState {
    fn update(&mut self, guessed: Word, gwr: GuessWordResult) {
        // The interpretation of letter_result is surprisingly tricky when it
        // comes to words with repeated letters. It is possible for a repeated
        // letter to first get a Black and then a Green in the same guess. This
        // means the Green was right, and the Black indicated that there are no
        // more positions with this letter. It's also possible for a repeated
        // letter to get a Black and Yellow.

        let mut letters_seen = 0u32;
        for (i, glr) in gwr.iter().enumerate() {
            let letter = (guessed.0[i] - b'a') as usize;

            // First, process the letter_choices state. This is straightforward: for
            // Green, we eliminate all other choices; for Yellow and Black, we
            // eliminate itself.
            match glr {
                GuessLetterResult::Green => self.letter_choices[i] = 1 << letter,
                _ => self.letter_choices[i] &= !(1 << letter),
            }

            // Next, process the letter_counts. If a letter gets Green and Yellow,
            // the lower bound of the count must be the number of times this letter
            // occurs here with Green and Yellow. For Black, the upper bound of the
            // count must be the one more than the number of Green and Yellow.
            let yellow_or_green_count = guessed
                .0
                .iter()
                .zip(gwr.iter())
                .filter(|(&c, &l)| (c - b'a') as usize == letter && l != GuessLetterResult::Black)
                .count() as u8;
            match glr {
                GuessLetterResult::Black =>
                    self.letter_counts[letter].0.end = 1 + yellow_or_green_count,
                _ => self.letter_counts[letter].0.start = yellow_or_green_count,
            }
            letters_seen |= 1 << letter;
        }

        // Now try to combine the information from the two fields.
        for letter in BitIter::from(letters_seen) {
            let current_letter_count = &mut self.letter_counts[letter];

            let implied_letter_count =
                (self.letter_choices.iter().filter(|set| **set == 1 << letter).count() as u8)
                    ..(self.letter_choices.iter().filter(|set| *set & (1 << letter) != 0).count()
                        as u8
                        + 1);
            current_letter_count.0.start =
                u8::max(current_letter_count.0.start, implied_letter_count.start);
            current_letter_count.0.end =
                u8::min(current_letter_count.0.end, implied_letter_count.end);

            assert!(
                !current_letter_count.0.is_empty(),
                "Current letter_count must not be empty but it is. State: {:?}",
                self
            );

            // Pass 1: remove if LetterCount(0..1)
            if current_letter_count == &LetterCount(0..1) {
                for set in self.letter_choices.iter_mut() {
                    *set &= !(1 << letter)
                }
            }
            // Pass 2: if a single count
            else if current_letter_count.0.end - current_letter_count.0.start == 1
                && self.letter_choices.iter().filter(|&lc| lc & (1 << letter) != 0).count()
                    == current_letter_count.0.start as usize
            {
                for set in self.letter_choices.iter_mut() {
                    if *set & (1 << letter) != 0 {
                        *set = 1 << letter
                    }
                }
            }
        }
    }

    fn then(&self, guessed: Word, gwr: GuessWordResult) -> Self {
        let mut new = self.clone();
        new.update(guessed, gwr);
        new
    }

    fn is_word_possible(&self, w: Word) -> bool {
        w.0.iter().zip(self.letter_choices.iter()).all(|(&c, &p)| (p & (1 << (c - b'a'))) != 0)
            && self.letter_counts.iter().enumerate().all(|(i, lc)| {
                *lc == LetterCount::default()
                    || lc.0.contains(&(w.0.iter().filter(|&c| (c - b'a') == i as u8).count() as u8))
            })
    }

    fn filter_word_list<'a>(&self, words: &'a mut [Word]) -> &'a mut [Word] {
        partition::partition(words, |&w| self.is_word_possible(w)).0
    }
}

fn load_words() -> Result<Vec<Word>> {
    let mut rv = vec![];
    let f = File::open("/Users/qzy/Desktop/Wordle/five_letter_words.txt")
        .map_err(|fe| format!("{:?}", fe))?;
    let reader = BufReader::new(f);
    for line in reader.lines().map(|l| l.unwrap()) {
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let line: &str = &line;
        let word: Word = line
            .try_into()
            .map_err(|e| format!("File contains word {:?} which is invalid: {}", line, e))?;
        rv.push(word);
    }
    Ok(rv)
}

fn process_guess(guessed: Word, actual: Word) -> GuessWordResult {
    let mut rv: GuessWordResult = [GuessLetterResult::Black; 5];
    let mut remaining_guess = ArrayVec::<_, 5>::new();
    let mut remaining_actual = ArrayVec::<_, 5>::new();
    // Step 1: Green.
    for (i, result) in rv.iter_mut().enumerate() {
        if guessed.0[i] == actual.0[i] {
            *result = GuessLetterResult::Green;
        } else {
            remaining_guess.push((guessed.0[i], i));
            remaining_actual.push(actual.0[i]);
        }
    }
    // Step 2: Yellow. We need to look at the remaining letters after the
    // yellows. If the intersection is not empty, we assign them yellows.
    remaining_guess.sort_unstable();
    remaining_actual.sort_unstable();
    let (mut i, mut j) = (0usize, 0usize);
    while i < remaining_guess.len() && j < remaining_actual.len() {
        match remaining_guess[i].0.cmp(&remaining_actual[j]) {
            Ordering::Less => i += 1,
            Ordering::Greater => j += 1,
            Ordering::Equal => {
                rv[remaining_guess[i].1] = GuessLetterResult::Yellow;
                i += 1;
                j += 1;
            }
        }
    }
    rv
}

#[test]
fn test_process_guess() {
    let b = GuessLetterResult::Black;
    let y = GuessLetterResult::Yellow;
    let g = GuessLetterResult::Green;
    assert_eq!(process_guess("abcde".try_into().unwrap(), "fghij".try_into().unwrap()), [
        b, b, b, b, b
    ]);
    assert_eq!(process_guess("aaaaa".try_into().unwrap(), "abcde".try_into().unwrap()), [
        g, b, b, b, b
    ]);
    assert_eq!(process_guess("baaaa".try_into().unwrap(), "abcde".try_into().unwrap()), [
        y, y, b, b, b
    ]);
    assert_eq!(process_guess("brood".try_into().unwrap(), "proxy".try_into().unwrap()), [
        b, g, g, b, b
    ]);
    assert_eq!(process_guess("dippy".try_into().unwrap(), "proxy".try_into().unwrap()), [
        b, b, y, b, g
    ]);
    assert_eq!(process_guess("yucky".try_into().unwrap(), "proxy".try_into().unwrap()), [
        b, b, b, b, g
    ]);
}

// Returns the quality of a guess state. A high quality guess eliminates all but one word and has a quality of 1. A low quality guess eliminates nothing and has a quality of 0.
fn quality(guess_state: &GuessState, words: &[Word]) -> f64 {
    let total = words.len() as f64;
    let remaining = words.iter().filter(|&&w| guess_state.is_word_possible(w)).count() as f64;

    // We want:
    //   when r == t, q == 0;
    //   when r == 1, q == 1;
    // with linear scaling.
    // So define q(r, t) = ar+t+c.
    // Solving, q(r, t) = (t-r)/(t-1)
    (total - remaining) / (total - 1.0)
}

fn guess_quality(s: &GuessState, guessed_word: Word, words: &[Word]) -> f64 {
    words
        .iter()
        .map(|&actual_word| {
            let new_state = s.then(guessed_word, process_guess(guessed_word, actual_word));
            assert!(new_state.is_word_possible(actual_word));
            quality(&new_state, words)
        })
        .sum::<f64>()
        / words.len() as f64
}

fn guess_quality_lower_bound(
    s: &GuessState, guessed_word: Word, words: &[Word], lower_bound: f64,
) -> Option<f64> {
    let total = words.len() as f64;
    let minimum_quality = lower_bound * total;
    words
        .iter()
        .enumerate()
        .try_fold(0.0f64, |cur_quality, (i, &actual_word)| {
            let new_state = s.then(guessed_word, process_guess(guessed_word, actual_word));
            assert!(new_state.is_word_possible(actual_word), "original state = {:?}\nguessed_word = {}\nactual_word = {}\nguess result= {:?}\nnew state = {:?}", s, guessed_word, actual_word, process_guess(guessed_word, actual_word), new_state);
            let this_quality = quality(&new_state, words);
            let new_quality = cur_quality + this_quality;
            if new_quality + ((words.len() - i - 1) as f64) < minimum_quality {
                eprintln!("word = {} early reject after {}", guessed_word, i);
                debug_assert!(guess_quality(s, guessed_word, words) < minimum_quality);
                None
            } else {
                Some(new_quality)
            }
        })
        .map(|raw_quality| {
            let rv = raw_quality / total;
            // This uses == on double values. It is safe because in both
            // implementations we use the same sequence of operations to arrive
            // at the result.
            debug_assert_eq!(guess_quality(s, guessed_word, words) , rv);
            rv
        })
}

fn find_best_guess(s: &GuessState, words: &mut &mut [Word]) -> Result<Word> {
    let all_words: &mut [Word] = std::mem::take(words);
    let remaining_words = s.filter_word_list(all_words);
    if remaining_words.is_empty() {
        return Err("No more words remaining in word list".into());
    }
    *words = remaining_words;
    Ok(if words.len() == 1 {
        words[0]
    } else {
        let mut best_quality: f64 = 0.0;
        // We use a filter_map with a mutable closure here. The filter_map essentially yields an increasing subsequence.
        words
            .iter()
            .filter_map(|&guessed_word| {
                guess_quality_lower_bound(s, guessed_word, words, best_quality).map(
                    |better_quality| {
                        eprintln!("word = {} quality = {:.6}", guessed_word, better_quality);
                        best_quality = better_quality;
                        guessed_word
                    },
                )
            })
            .last()
            .unwrap()
    })
}

fn presort_words_by_heuristic(words: &mut [Word]) {
    let mut histogram = [[0isize; 26]; 5];
    for w in words.iter() {
        for (i, letter) in w.0.iter().enumerate() {
            histogram[i][(letter - b'a') as usize] += 1;
        }
    }
    words.sort_unstable_by_key(|w| {
        -w.0.iter()
            .enumerate()
            .map(|(i, letter)| histogram[i][(letter - b'a') as usize])
            .sum::<isize>()
    });
}

fn real_main() -> Result<()> {
    let mut words = load_words()?;
    presort_words_by_heuristic(&mut words);
    eprintln!("Loaded {} words", words.len());
    eprintln!("Initial Guess: {}", {
        let mut words: &mut [Word] = &mut words;
        find_best_guess(&GuessState::default(), &mut words)
    }?);
    let traces: &[Vec<(Word, GuessWordResult)>] = {
        let b = GuessLetterResult::Black;
        let y = GuessLetterResult::Yellow;
        let g = GuessLetterResult::Green;
        &[
            vec![
                ("RAISE".try_into().unwrap(), [b, g, b, b, b]),
                ("BACON".try_into().unwrap(), [b, g, b, b, y]),
                ("VAUNT".try_into().unwrap(), [b, g, b, y, y]),
                ("TAWNY".try_into().unwrap(), [g, g, b, y, g]),
            ],
            vec![
                ("rates".try_into().unwrap(), [b, g, b, b, b]),
                ("manly".try_into().unwrap(), [b, g, g, b, b]),
                ("danio".try_into().unwrap(), [b, g, g, g, b]),
            ],
            vec![
                ("tares".try_into().unwrap(), [b, y, y, b, y]),
                ("snark".try_into().unwrap(), [g, b, y, y, b]),
            ],
            vec![
                ("tares".try_into().unwrap(), [b, b, y, y, y]),
                ("prose".try_into().unwrap(), [b, y, b, y, g]),
            ],
            vec![
                ("saner".try_into().unwrap(), [b, b, b, b, y]),
                ("court".try_into().unwrap(), [b, y, b, y, b]),
                ("brood".try_into().unwrap(), [b, g, g, b, b]),
            ],
            vec![
                ("sales".try_into().unwrap(), [b, b, b, b, b]),
                ("count".try_into().unwrap(), [b, g, b, g, g]),
            ],
            vec![
                ("raise".try_into().unwrap(), [g, b, b, b, b]),
                ("rotor".try_into().unwrap(), [g, g, y, g, b]),
            ],
        ]
    };

    for trace in traces.iter() {
        let mut state = GuessState::default();
        let mut words: &mut [Word] = &mut words;
        for (i, &(guessed_word, result)) in trace.iter().enumerate() {
            state.update(guessed_word, result);
            eprintln!("State after round {}: {:?}", 1 + i, state);
            println!("Recommended Guess: {}", find_best_guess(&state, &mut words)?);
            println!("Remaining words: {}", words.len());
        }
    }
    Ok(())
}

fn main() {
    match real_main() {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
