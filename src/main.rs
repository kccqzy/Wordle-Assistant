use bit_iter::BitIter;
use std::{
    fs::File,
    io::{BufRead, BufReader},
};

type Word = [u8; 5];
type Words = Vec<Word>;

type Result<T> = std::result::Result<T, String>;

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
enum GuessLetterResult {
    Grey,
    Yellow,
    Green,
}

type GuessWordResult = [GuessLetterResult; 5];

struct GuessState {
    letter_choices: [u32; 5],
    must_appear: u32,
}

impl Default for GuessState {
    fn default() -> Self {
        GuessState { letter_choices: [(1 << 26) - 1; 5], must_appear: 0 }
    }
}

impl GuessState {
    fn update(&mut self, guessed: Word, gwr: GuessWordResult) {
        for i in 0..5 {
            match gwr[i] {
                GuessLetterResult::Green => self.letter_choices[i] = 1 << (guessed[i] - b'a'),
                GuessLetterResult::Grey => {
                    for j in 0..5 {
                        self.letter_choices[j] &= !(1 << (guessed[i] - b'a'));
                    }
                }
                GuessLetterResult::Yellow => {
                    self.letter_choices[i] &= !(1 << (guessed[i] - b'a'));
                    self.must_appear |= 1 << (guessed[i] - b'a');
                }
            }
        }
        for c in BitIter::from(self.must_appear) {
            if let Some((0, (i, _))) = self
                .letter_choices
                .iter()
                .enumerate()
                .filter(|(_, &ch)| (ch & (1 << c)) > 0)
                .enumerate()
                .last()
            {
                self.letter_choices[i] = 1 << c;
                self.must_appear &= !(1 << c);
            }
        }
    }

    fn is_word_possible(&self, w: Word) -> bool {
        w.iter().zip(self.letter_choices.iter()).all(|(&c, &p)| (p & (1 << (c - b'a'))) != 0)
            && BitIter::from(self.must_appear).all(|c| w.iter().any(|&ch| ch == c as u8 + b'a'))
    }
}

fn load_words() -> Result<Words> {
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
        let line_bytes = line.as_bytes();
        if !line_bytes.is_ascii() {
            return Err(format!("File contains word {:?} which is not an ASCII word", line));
        }
        let mut word: Word = line_bytes.try_into().map_err(|_| {
            format!("File contains word {:?} which is not a five-letter word", line)
        })?;
        word.make_ascii_lowercase();
        if !word.iter().all(|&c| (b'a'..=b'z').contains(&c)) {
            return Err(format!(
                "File contains word {:?} which is not composed of all letters",
                line
            ));
        }
        rv.push(word);
    }
    Ok(rv)
}

// TODO use the real Display trait
fn display_word(w: Word) -> String {
    String::from(std::str::from_utf8(&w).unwrap())
}

fn process_guess(guessed: Word, actual: Word) -> GuessWordResult {
    let mut set: u32 = 0;
    let mut rv: GuessWordResult = [GuessLetterResult::Grey; 5];
    for c in actual {
        set |= 1 << (c - b'a');
    }
    for i in 0..5 {
        rv[i] = if guessed[i] == actual[i] {
            GuessLetterResult::Green
        } else if (1 << (guessed[i] - b'a')) & set != 0 {
            GuessLetterResult::Yellow
        } else {
            GuessLetterResult::Grey
        }
    }
    rv
}

// Returns the quality of a guess. A high quality guess eliminates all but one word and has a quality of 1. A low quality guess eliminates nothing and has a quality of 0.
fn initial_guess_quality(guess_state: &GuessState, words: &Words) -> f64 {
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

fn find_initial_guess(words: &Words) -> Word {
    // For each guessed word, we evaluate for each possible actual word, the guess quality.
    *words
        .iter()
        .max_by_key(|&&guessed_word| {
            let rv = words
                .iter()
                .map(|&actual_word| {
                    let mut guess_state = GuessState::default();
                    guess_state.update(guessed_word, process_guess(guessed_word, actual_word));
                    initial_guess_quality(&guess_state, words)
                })
                .sum::<f64>() as f64
                / words.len() as f64;
            println!("word = {} quality = {:.6}", display_word(guessed_word), rv);
            (rv * 1e6) as u64
        })
        .unwrap()
}

fn real_main() -> Result<()> {
    let words = load_words()?;
    eprintln!("Loaded {} words", words.len());
    eprintln!("Initial Guess: {}", display_word(find_initial_guess(&words)));
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
