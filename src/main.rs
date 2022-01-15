use bit_iter::BitIter;
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

#[derive(Clone)]
struct GuessState {
    letter_choices: [u32; 5],
    must_appear: u32,
}

impl Default for GuessState {
    fn default() -> Self {
        GuessState { letter_choices: [(1 << 26) - 1; 5], must_appear: 0 }
    }
}

impl std::fmt::Debug for GuessState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn set_to_str(s: u32) -> String {
            BitIter::from(s).map(|c| (c as u8 + b'a') as char).collect()
        }
        f.debug_struct("GuessState")
            .field(
                "letter_choices",
                &self.letter_choices.iter().copied().map(set_to_str).collect::<Vec<String>>(),
            )
            .field("must_appear", &set_to_str(self.must_appear))
            .finish()
    }
}

impl GuessState {
    fn update(&mut self, guessed: Word, gwr: GuessWordResult) {
        for i in 0..5 {
            match gwr[i] {
                GuessLetterResult::Green => self.letter_choices[i] = 1 << (guessed.0[i] - b'a'),
                GuessLetterResult::Grey =>
                    for j in 0..5 {
                        self.letter_choices[j] &= !(1 << (guessed.0[i] - b'a'));
                    },
                GuessLetterResult::Yellow => {
                    self.letter_choices[i] &= !(1 << (guessed.0[i] - b'a'));
                    self.must_appear |= 1 << (guessed.0[i] - b'a');
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

    fn then(&self, guessed: Word, gwr: GuessWordResult) -> Self {
        let mut new = self.clone();
        new.update(guessed, gwr);
        new
    }

    fn is_word_possible(&self, w: Word) -> bool {
        w.0.iter().zip(self.letter_choices.iter()).all(|(&c, &p)| (p & (1 << (c - b'a'))) != 0)
            && BitIter::from(self.must_appear).all(|c| w.0.iter().any(|&ch| ch == c as u8 + b'a'))
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
        let line: &str = &line;
        let word: Word = line
            .try_into()
            .map_err(|e| format!("File contains word {:?} which is invalid: {}", line, e))?;
        rv.push(word);
    }
    Ok(rv)
}

fn process_guess(guessed: Word, actual: Word) -> GuessWordResult {
    let mut set: u32 = 0;
    let mut rv: GuessWordResult = [GuessLetterResult::Grey; 5];
    for c in actual.0 {
        set |= 1 << (c - b'a');
    }
    for i in 0..5 {
        rv[i] = if guessed.0[i] == actual.0[i] {
            GuessLetterResult::Green
        } else if (1 << (guessed.0[i] - b'a')) & set != 0 {
            GuessLetterResult::Yellow
        } else {
            GuessLetterResult::Grey
        }
    }
    rv
}

// Returns the quality of a guess state. A high quality guess eliminates all but one word and has a quality of 1. A low quality guess eliminates nothing and has a quality of 0.
fn quality(guess_state: &GuessState, words: &Words) -> f64 {
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

fn guess_quality(s: &GuessState, guessed_word: Word, words: &Words) -> f64 {
    words
        .iter()
        .map(|&actual_word| {
            quality(&s.then(guessed_word, process_guess(guessed_word, actual_word)), words)
        })
        .sum::<f64>()
        / words.len() as f64
}

fn find_best_guess(s: &GuessState, words: &Words) -> Word {
    // For each guessed word, we evaluate for each possible actual word, the guess quality.
    let words = &words
        .iter()
        .filter(|&&actual_word| s.is_word_possible(actual_word))
        .cloned()
        .collect::<Words>();
    *words
        .iter()
        .max_by_key(|&&guessed_word| {
            let rv = guess_quality(s, guessed_word, words);
            println!("word = {} quality = {:.6}", guessed_word, rv);
            (rv * 1e6) as u64
        })
        .unwrap()
}

fn real_main() -> Result<()> {
    let words = load_words()?;
    eprintln!("Loaded {} words", words.len());
    // eprintln!("Initial Guess: {}", find_best_guess(&GuessState::default(), &words));
    let example_trace: &[(Word, GuessWordResult)] = &[("RAISE".try_into().unwrap(), [
        GuessLetterResult::Grey,
        GuessLetterResult::Green,
        GuessLetterResult::Grey,
        GuessLetterResult::Grey,
        GuessLetterResult::Grey,
    ])];
    let example_state = &GuessState::default().then(example_trace[0].0, example_trace[0].1);
    eprintln!("State after first trial: {:?}", example_state);
    eprintln!("First trial: {}", find_best_guess(&example_state, &words));
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
