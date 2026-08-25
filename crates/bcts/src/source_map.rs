use rmx::prelude::*;

use std::ops::Range;
use std::{fmt, hash, iter, mem};

use crate::input::Source;
use crate::text::{Text, SubText};
use crate::chunk::Chunk;

/// How far a comment or string extends, or `None` if one does not start here.
///
/// `Ok` is a well-formed run, `Err` an unterminated one; either way the
/// payload is its length in bytes.
pub type ScanResult = Option<Result<usize, usize>>;

/// A scanner for one lexical form, wrapped so it can sit in a salsa struct.
///
/// A bare `fn` pointer field makes the `Debug` bound salsa derives ambiguous
/// under a higher-ranked lifetime, so the impls it needs are written out here.
#[derive(Copy, Clone)]
pub struct Scanner(pub for<'a> fn(&'a str) -> ScanResult);

impl Scanner {
    pub fn scan(&self, text: &str) -> ScanResult {
        (self.0)(text)
    }
}

impl fmt::Debug for Scanner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Scanner")
    }
}

impl hash::Hash for Scanner {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        (self.0 as usize).hash(state);
    }
}

#[salsa::tracked]
pub struct Config<'db> {
    #[returns(ref)]
    comment_start_chars: Vec<char>,
    #[returns(ref)]
    string_start_chars: Vec<char>,
    // Function pointers have no meaningful equality, so opt out of the
    // backdating comparison salsa would otherwise generate.
    #[no_eq]
    #[returns(copy)]
    parse_comment: Scanner,
    #[no_eq]
    #[returns(copy)]
    parse_string: Scanner,
}

#[salsa::tracked(returns(copy))]
pub fn basic_source_map<'db>(
    db: &'db dyn crate::Db,
    source: Source,
) -> Chunk<'db> {
    source_map(
        db,
        source,
        basic_config(db),
    )
}

#[salsa::tracked(returns(copy))]
pub fn source_map<'db>(
    db: &'db dyn crate::Db,
    source: Source,
    config: Config<'db>,
) -> Chunk<'db> {
    let mut state = State {
        db,
        config,
        source,
        position: 0,
        chunk_wip: ChunkWip {
            chunk_start: 0,
            comments: vec![],
            strings: vec![],
            errors: vec![],
        },
    };

    state.map()
}

#[salsa::tracked(returns(copy))]
pub fn basic_config<'db>(
    db: &'db dyn crate::Db,
) -> Config<'db> {
    Config::new(
        db,
        vec!['/'],
        vec!['"'],
        Scanner(basic_parse_comment),
        Scanner(basic_parse_string),
    )
}

struct State<'db> {
    db: &'db dyn crate::Db,
    config: Config<'db>,
    source: Source,
    position: usize,
    chunk_wip: ChunkWip,
}

// ranges are relative to `chunk_start`
struct ChunkWip {
    chunk_start: usize,
    comments: Vec<Range<usize>>,
    strings: Vec<Range<usize>>,
    errors: Vec<Range<usize>>,
}

impl<'db> State<'db> {
    fn map(mut self) -> Chunk<'db> {
        let all_start_chars =
            self.config.comment_start_chars(self.db).iter().copied().chain(
                self.config.string_start_chars(self.db).iter().copied()
            ).collect::<Vec<_>>();

        let text_all = self.source.text(self.db);

        loop {
            let text_remaining = &text_all[self.position..];
            let mut start_char_indexes = text_remaining.match_indices(&*all_start_chars).map(|(i, s)| i);
            let next_start_char_index = start_char_indexes.next();

            match next_start_char_index {
                Some(start_char_index) => {
                    self.position = self.position.checked_add(start_char_index).X();
                    let text_remaining = &text_remaining[start_char_index..];

                    let parse_comment_res = self.parse_comment(text_remaining);
                    let parse_string_res = self.parse_string(text_remaining);

                    self.step(
                        parse_comment_res,
                        parse_string_res,
                    );
                }
                None => {
                    break;
                }
            }
        }

        Chunk::new(
            self.db,
            // fixme bad clone of full source
            Text::new(self.db, S(text_all)),
            mem::take(&mut self.chunk_wip.comments),
            mem::take(&mut self.chunk_wip.strings),
            mem::take(&mut self.chunk_wip.errors),
        )
    }

    fn step(
        &mut self,
        parse_comment: Option<Result<usize, usize>>,
        parse_string: Option<Result<usize, usize>>,
    ) {
        let chunk_offset = self.position.checked_sub(self.chunk_wip.chunk_start).X();

        match (parse_comment, parse_string) {
            (Some(Ok(comment_bytes)), None) => {
                let chunk_end = chunk_offset.checked_add(comment_bytes).X();
                self.chunk_wip.comments.push(chunk_offset..chunk_end);
                self.position = self.position.checked_add(comment_bytes).X();
            }
            (Some(Err(comment_bytes)), None) => {
                let chunk_end = chunk_offset.checked_add(comment_bytes).X();
                self.chunk_wip.errors.push(chunk_offset..chunk_end);
                self.position = self.position.checked_add(comment_bytes).X();
            }
            (None, Some(Ok(string_bytes))) => {
                let chunk_end = chunk_offset.checked_add(string_bytes).X();
                self.chunk_wip.strings.push(chunk_offset..chunk_end);
                self.position = self.position.checked_add(string_bytes).X();
            }
            (None, Some(Err(string_bytes))) => {
                let chunk_end = chunk_offset.checked_add(string_bytes).X();
                self.chunk_wip.errors.push(chunk_offset..chunk_end);
                self.position = self.position.checked_add(string_bytes).X();
            }
            (None, None) => {
                self.position = self.position.checked_add(1).X();
                let text_all = self.source.text(self.db);
                assert!(self.position <= text_all.len());
            }
            (_, _) => unreachable!(),
        }
    }

    fn parse_comment(&self, text: &str) -> Option<Result<usize, usize>> {
        let start_char = text.chars().next().X();
        if self.config.comment_start_chars(self.db).contains(&start_char) {
            self.config.parse_comment(self.db).scan(text)
        } else {
            None
        }
    }

    fn parse_string(&self, text: &str) -> Option<Result<usize, usize>> {
        let start_char = text.chars().next().X();
        if self.config.string_start_chars(self.db).contains(&start_char) {
            self.config.parse_string(self.db).scan(text)
        } else {
            None
        }
    }
}

fn basic_parse_comment(text: &str) -> Option<Result<usize, usize>> {
    let bytes = text.as_bytes();
    match *bytes {
        [b'/', b'/', ..] => {
            let newline = rmx::memchr::memchr(b'\n', bytes);
            match newline {
                Some(newline) => {
                    Some(Ok(newline))
                }
                None => {
                    Some(Ok(text.len()))
                }
            }
        },
        [b'/', b'*', ..] => {
            parse_nested_comment(text)
        }
        [b'/', ..] => None,
        _ => unreachable!(),
    }
}

fn basic_parse_string(text: &str) -> Option<Result<usize, usize>> {
    match text.as_bytes()[0] {
        b'"' => {
            let bytes = text.as_bytes();
            let mut i = 1;

            while i < bytes.len() {
                match bytes[i] {
                    b'"' => {
                        return Some(Ok(i + 1));
                    }
                    b'\\' => {
                        // Skip backslash and next character (if exists).
                        i += 1;
                        if i < bytes.len() {
                            i += 1;
                        }
                    }
                    _ => {
                        i += 1;
                    }
                }
            }

            // No closing quote found.
            Some(Err(text.len()))
        },
        _ => unreachable!(),
    }
}

fn parse_nested_comment(text: &str) -> Option<Result<usize, usize>> {

    assert!(text.starts_with("/*"));

    #[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Copy, Clone)]
    enum Kind { Open, Close }

    let opens = text.match_indices("/*").map(|(i, _)| (i, Kind::Open));
    let closes = text.match_indices("*/").map(|(i, _)| (i, Kind::Close));
    let all_braces = opens.merge(closes);
    let without_overlaps = all_braces.scan(None::<usize>, |prev_index, (index, kind)| {
        match *prev_index {
            None => {
                *prev_index = Some(index);
                Some(Some((index, kind)))
            },
            Some(pi) => {
                if Some(pi) == index.checked_sub(1) {
                    Some(None)
                } else {
                    *prev_index = Some(index);
                    Some(Some((index, kind)))
                }
            }
        }
    }).flatten();

    let mut stack = vec![];

    for (index, kind) in without_overlaps {
        match kind {
            Kind::Open => {
                stack.push((index, kind));
            }
            Kind::Close => {
                stack.pop().X();
                if stack.is_empty() {
                    return Some(Ok(index.checked_add(2).X()));
                }
            }
        }
    }

    if !stack.is_empty() {
        // Unclosed open braces
        Some(Err(text.len()))
    } else {
        // No open braces
        unreachable!()
    }
}


#[test]
fn test_source_map() {
    fn chunk<'db>(db: &'db dyn crate::Db, s: &str) -> Chunk<'db> {
        let source = Source::new(db, S(s));
        source_map(db, source, basic_config(db))
    }

    // "F"ragment
    #[derive(Debug, Copy, Clone)]
    enum F<'s> {
        T(&'s str), // text
        C(&'s str), // comment
        S(&'s str), // string
        E(&'s str), // error
    }

    fn strs<'s, 'ss>(frags: &'ss [F<'s>]) -> impl Iterator<Item = &'s str> + 'ss {
        frags.iter().map(|f| match f {
            F::T(s) | F::C(s) | F::S(s) | F::E(s) => *s,
        })
    }

    fn source(frags: &[F<'_>]) -> String {
        strs(frags).collect()
    }

    fn positions<'ss>(frags: &'ss [F<'_>]) -> impl Iterator<Item = usize> + 'ss {
        strs(frags).scan(0, |pos, s| {
            let next = *pos;
            *pos = next + s.len();
            Some(next)
        })
    }

    fn run<'s>(frags: &[F<'s>]) {
        let db = &crate::Database::default();
        let text = source(frags);
        // eprintln!("t '{text}'");
        let a_chunk = chunk(db, &text);
        let ex_chunk = frags;

        let ex_frags = ex_chunk.iter().map(|f| *f).collect::<Vec<_>>();
        let ex_text = source(&ex_frags);
        let ex_positions = positions(&ex_frags);
        let ex_chunk = ex_chunk.iter().zip(ex_positions);
        let ex_chunk = ex_chunk.collect::<Vec<_>>();

        let a_text = a_chunk.text(db).as_str(db);
        assert_eq!(&ex_text, a_text);
        //eprintln!("at '{a_text}'");

        let mut comments = a_chunk.comments(db).C();
        let mut strings = a_chunk.strings(db).C();
        let mut errors = a_chunk.errors(db).C();
        for (frag, pos) in ex_chunk.iter().rev() {
            //eprintln!("f {frag:?} {pos}");
            match frag {
                F::T(s) => {
                    assert_eq!(&a_text[*pos..][..s.len()], *s);
                }
                F::C(s) => {
                    assert_eq!(&a_text[*pos..][..s.len()], *s);
                    let range = comments.pop().X();
                    assert_eq!(range, *pos .. (*pos + s.len()));
                }
                F::S(s) => {
                    assert_eq!(&a_text[*pos..][..s.len()], *s);
                    let range = strings.pop().X();
                    assert_eq!(range, *pos .. (*pos + s.len()));
                }
                F::E(s) => {
                    assert_eq!(&a_text[*pos..][..s.len()], *s);
                    let range = errors.pop().X();
                    assert_eq!(range, *pos .. (*pos + s.len()));
                }
            }
        }

        assert!(comments.is_empty());
        assert!(strings.is_empty());
        assert!(errors.is_empty());
    }

    run(&[
        F::T("ab"),
        F::T("bdd"),
        F::C("//"),
    ]);
    run(&[
        F::T("ab"),
        F::T("bdd"),
        F::C("//"),
        F::T("\n"),
    ]);
    run(&[
        F::T("ab"),
        F::C("//a"),
        F::T("\nbdd"),
        F::C("//b"),
        F::T("\n"),
        F::C("//b"),
    ]);
    run(&[
        F::T("ab"),
        F::T("bdd"),
        F::S("\"x\""),
    ]);
    run(&[
        F::T("ab"),
        F::T("bdd"),
        F::S("\"x\""),
        F::S("\"x\""),
    ]);
    run(&[
        F::T("a"),
        F::T("b"),
        F::T("c"),
    ]);
    run(&[
        F::T("ab"),
        F::E("\"x"),
    ]);
    run(&[
        F::T("ab"),
        F::T("ab"),
        F::E("\"x"),
    ]);
    run(&[
        F::E("\"x . //"),
    ]);
    run(&[
        F::C("// \" . \""),
        F::T("\n"),
    ]);
    run(&[
        F::S("\"// . \""),
    ]);
    run(&[
        F::T("/ a"),
    ]);
    run(&[
        F::E("/* a"),
    ]);
    run(&[
        F::C("/* */"),
    ]);
    run(&[
        F::C("/*/**/*/"),
    ]);
    run(&[
        F::E("/*/**/ab"),
    ]);

    // Escape sequence tests.
    run(&[
        F::S("\"foo\\\"bar\""),
    ]);
    run(&[
        F::S("\"foo\\\\\""),
    ]);
    run(&[
        F::S("\"foo\\\\\\\"bar\""),
    ]);
    run(&[
        F::S("\"\\\\\\\\\""),
    ]);
    run(&[
        F::E("\"foo\\\"bar"),
    ]);
    run(&[
        F::E("\"foo\\\\"),
    ]);
    run(&[
        F::T("x"),
        F::S("\"a\\\"b\""),
        F::T("y"),
    ]);
}

/// A `Config` other than `basic_config` really does drive the scan.
///
/// The comment and string forms here share no syntax with the built-in ones,
/// so the same text has to chunk differently under each.
#[test]
fn test_source_map_custom_config() {
    let db = &crate::Database::default();

    // Comment and string in the custom syntax, followed by text that only the
    // built-in config would read as a comment and a string.
    let text = "a#note\nb'str'c// not a comment\n\"not a string\"";
    let source = Source::new(db, S(text));

    let at = |needle: &str| {
        let start = text.find(needle).expect("fragment is in the source");
        start..start + needle.len()
    };

    let custom = source_map(db, source, custom_config(db));
    assert_eq!(custom.comments(db), &vec![at("#note")]);
    assert_eq!(custom.strings(db), &vec![at("'str'")]);
    assert_eq!(custom.errors(db), &Vec::<Range<usize>>::new());

    // The built-in config sees the other two forms and none of the custom ones.
    let basic = source_map(db, source, basic_config(db));
    assert_eq!(basic.comments(db), &vec![at("// not a comment")]);
    assert_eq!(basic.strings(db), &vec![at("\"not a string\"")]);
    assert_eq!(basic.errors(db), &Vec::<Range<usize>>::new());
}

/// An unterminated run in a custom scanner is reported as an error range.
#[test]
fn test_source_map_custom_config_error() {
    let db = &crate::Database::default();
    let source = Source::new(db, S("ab'unterminated"));
    let chunk = source_map(db, source, strings_only_config(db));
    assert_eq!(chunk.strings(db), &Vec::<Range<usize>>::new());
    assert_eq!(chunk.errors(db), &vec![2..15]);
}

#[cfg(test)]
fn hash_comment(text: &str) -> ScanResult {
    assert!(text.starts_with('#'));
    Some(Ok(rmx::memchr::memchr(b'\n', text.as_bytes()).unwrap_or(text.len())))
}

#[cfg(test)]
fn quote_string(text: &str) -> ScanResult {
    assert!(text.starts_with('\''));
    match text[1..].find('\'') {
        Some(end) => Some(Ok(end + 2)),
        None => Some(Err(text.len())),
    }
}

#[cfg(test)]
fn never_comment(_text: &str) -> ScanResult {
    None
}

#[cfg(test)]
#[salsa::tracked(returns(copy))]
fn custom_config<'db>(db: &'db dyn crate::Db) -> Config<'db> {
    Config::new(
        db,
        vec!['#'],
        vec!['\''],
        Scanner(hash_comment),
        Scanner(quote_string),
    )
}

#[cfg(test)]
#[salsa::tracked(returns(copy))]
fn strings_only_config<'db>(db: &'db dyn crate::Db) -> Config<'db> {
    Config::new(
        db,
        vec![],
        vec!['\''],
        Scanner(never_comment),
        Scanner(quote_string),
    )
}
