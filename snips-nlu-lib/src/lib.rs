extern crate base64;
extern crate crfsuite;
extern crate csv;
extern crate dinghy_test;
#[macro_use]
extern crate failure;
extern crate itertools;
#[macro_use]
extern crate lazy_static;
extern crate lru_cache;
#[macro_use]
extern crate ndarray;
extern crate regex;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate snips_nlu_ontology;
extern crate snips_nlu_ontology_parsers;
extern crate snips_nlu_utils as nlu_utils;
extern crate tempfile;
extern crate zip;

#[cfg(test)]
#[macro_use]
extern crate maplit;

mod builtin_entity_parsing;
pub mod models;
pub mod errors;
mod intent_classifier;
mod intent_parser;
mod language;
mod nlu_engine;
mod resources;
mod slot_filler;
mod slot_utils;
#[cfg(test)]
mod testutils;
mod utils;

pub const MODEL_VERSION: &str = "0.16.0";

pub use models::*;
pub use errors::*;
pub use builtin_entity_parsing::BuiltinEntityParserFactory; // This is used by benches
pub use intent_classifier::{IntentClassifier, LogRegIntentClassifier};
pub use intent_parser::{DeterministicIntentParser, IntentParser, ProbabilisticIntentParser};
pub use nlu_engine::SnipsNluEngine;
pub use resources::loading::clear_resources; // This is used by benches
pub use slot_filler::{CRFSlotFiller, SlotFiller};
pub use nlu_utils::token::{compute_all_ngrams, tokenize_light};
pub use utils::file_path; // This is used by benches
