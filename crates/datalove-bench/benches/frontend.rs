//! Lexing, bracing and parsing the standard library.
//!
//! The front end is the largest phase of a datalove invocation: of the ~94ms
//! spent compiling `sys/std` on every start, roughly 39ms is parsing, which is
//! more than typechecking. These cases take it apart.
//!
//! The phases nest, and each is salsa-memoized on the one below, so the timings
//! are cumulative: `brace` includes `lex`, and `parse` includes both. Subtract to
//! get a phase on its own.
//!
//! A fresh database and fresh `Source` inputs are built per iteration in
//! `with_inputs`, which divan does not time. Reusing them would measure a memo
//! hit and nothing else.

use divan::Bencher;

use bct::input::Source;
use datalove_datafun as datafun;

fn main() {
    divan::main();
}

/// A database and one `Source` per module of the embedded standard library.
struct Corpus {
    db: datafun::Database,
    sources: Vec<Source>,
}

impl Corpus {
    fn new() -> Self {
        let db = datafun::Database::default();
        let sys = datalove_sys_packages::system_library();
        let sources = module_texts(&sys)
            .into_iter()
            .map(|text| Source::new(&db, text))
            .collect();
        Self { db, sources }
    }

    /// How much source there is, so a rate can be worked out from a time.
    fn bytes(&self) -> usize {
        let sys = datalove_sys_packages::system_library();
        module_texts(&sys).iter().map(|t| t.len()).sum()
    }
}

/// Every module source in the system library.
fn module_texts(sys: &datafun::pipeline::SystemLibrary) -> Vec<String> {
    sys.library
        .packages
        .values()
        .flat_map(|package| package.modules.values())
        .map(|module| module.source.to_string())
        .collect()
}

#[divan::bench]
fn lex(bencher: Bencher) {
    bencher.with_inputs(Corpus::new).bench_local_values(|corpus| {
        for source in &corpus.sources {
            let chunk = bct::source_map::basic_source_map(&corpus.db, *source);
            divan::black_box(bct::lexer::lex_chunk(&corpus.db, chunk));
        }
    });
}

#[divan::bench]
fn brace(bencher: Bencher) {
    bencher.with_inputs(Corpus::new).bench_local_values(|corpus| {
        for source in &corpus.sources {
            let chunk = bct::source_map::basic_source_map(&corpus.db, *source);
            let lexed = bct::lexer::lex_chunk(&corpus.db, chunk);
            divan::black_box(bct::bracer::bracer(&corpus.db, lexed));
        }
    });
}

#[divan::bench]
fn parse(bencher: Bencher) {
    bencher.with_inputs(Corpus::new).bench_local_values(|corpus| {
        for source in &corpus.sources {
            divan::black_box(datalove_datafun_parser::parse(&corpus.db, *source));
        }
    });
}

/// What the corpus is, printed once so the numbers above have a denominator.
#[divan::bench(sample_count = 1, sample_size = 1)]
fn corpus_size() {
    let corpus = Corpus::new();
    eprintln!(
        "\ncorpus: {} modules, {} bytes\n",
        corpus.sources.len(),
        corpus.bytes()
    );
}
