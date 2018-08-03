use itertools::Itertools;
use std::collections::HashMap;
use std::str::FromStr;

use super::crf_utils::TaggingScheme;
use super::features;
use models::FeatureFactory;
use errors::*;
use nlu_utils::token::Token;
use resources::gazetteer::{get_gazetteer, HashSetGazetteer};
use resources::SharedResources;
use resources::stemmer::get_stemmer;
use resources::word_clusterer::get_word_clusterer;
use snips_nlu_ontology::{BuiltinEntityKind, Language};
use std::sync::Arc;

pub struct ProbabilisticFeatureProcessor {
    functions: Vec<FeatureFunction>,
}

impl ProbabilisticFeatureProcessor {
    // TODO add a `GazetteerProvider` to this signature
    pub fn new(
        features: &[FeatureFactory],
        shared_resources: Arc<SharedResources>
    ) -> Result<ProbabilisticFeatureProcessor> {
        let functions = features
            .iter()
            .map(|f| get_feature_function(f, shared_resources.clone()))
            .collect::<Result<Vec<Vec<_>>>>()?
            .into_iter()
            .flat_map(|fs| fs)
            .collect();

        Ok(ProbabilisticFeatureProcessor { functions })
    }
}

impl ProbabilisticFeatureProcessor {
    #[cfg_attr(rustfmt, rustfmt_skip)]
    pub fn compute_features(&self, input: &&[Token]) -> Vec<Vec<(String, String)>> {
        self.functions
            .iter()
            .fold(vec![vec![]; input.len()], |mut acc, f| {
                (0..input.len()).foreach(|i| {
                    if let Some(value) = (f.function)(input, i) {
                        f.offsets.iter().foreach(|&(offset, ref key)| {
                            if i as i32 - offset >= 0 && i as i32 - offset < input.len() as i32 {
                                acc[(i as i32 - offset) as usize].push(
                                    (key.clone(), value.clone())
                                );
                            }
                        });
                    }
                });
                acc
            })
    }
}

struct FeatureFunction {
    function: Box<Fn(&[Token], usize) -> Option<String> + Send + Sync>,
    offsets: Vec<(i32, String)>,
}

impl FeatureFunction {
    fn new<T>(key: &str, offsets: Vec<i32>, function: T) -> FeatureFunction
        where
            T: Fn(&[Token], usize) -> Option<String> + Send + Sync + 'static,
    {
        let offsets = offsets
            .into_iter()
            .map(|i| {
                (
                    i,
                    if i == 0 {
                        key.to_string()
                    } else {
                        format!("{}[{:+}]", key, i)
                    },
                )
            })
            .collect();
        FeatureFunction {
            offsets,
            function: Box::new(function),
        }
    }
}

fn get_feature_function(
    f: &FeatureFactory,
    shared_resources: Arc<SharedResources>
) -> Result<Vec<FeatureFunction>> {
    let offsets = f.offsets.clone();
    match f.factory_name.as_ref() {
        "is_digit" => Ok(vec![is_digit_feature_function(offsets)?]),
        "length" => Ok(vec![length_feature_function(offsets)?]),
        "is_first" => Ok(vec![is_first_feature_function(offsets)?]),
        "is_last" => Ok(vec![is_last_feature_function(offsets)?]),
        "ngram" => Ok(vec![ngram_feature_function(&f.args, offsets)?]),
        "shape_ngram" => Ok(vec![shape_ngram_feature_function(&f.args, offsets)?]),
        "prefix" => Ok(vec![prefix_feature_function(&f.args, offsets)?]),
        "suffix" => Ok(vec![suffix_feature_function(&f.args, offsets)?]),
        "entity_match" => entity_match_feature_function(&f.args, &offsets),
        "builtin_entity_match" => builtin_entity_match_feature_function(&f.args, &offsets, shared_resources),
        "word_cluster" => Ok(vec![word_cluster_feature_function(&f.args, offsets)?]),
        _ => bail!("Feature {} not implemented", f.factory_name),
    }
}

fn is_digit_feature_function(offsets: Vec<i32>) -> Result<FeatureFunction> {
    Ok(FeatureFunction::new("is_digit", offsets, |t, i| {
        features::is_digit(&t[i].value)
    }))
}

fn length_feature_function(offsets: Vec<i32>) -> Result<FeatureFunction> {
    Ok(FeatureFunction::new("length", offsets, |t, i| {
        features::length(&t[i].value)
    }))
}

fn is_first_feature_function(offsets: Vec<i32>) -> Result<FeatureFunction> {
    Ok(FeatureFunction::new("is_first", offsets, |_, i| {
        features::is_first(i)
    }))
}

fn is_last_feature_function(offsets: Vec<i32>) -> Result<FeatureFunction> {
    Ok(FeatureFunction::new("is_last", offsets, |t, i| {
        features::is_last(t, i)
    }))
}

fn ngram_feature_function(
    args: &HashMap<String, ::serde_json::Value>,
    offsets: Vec<i32>,
) -> Result<FeatureFunction> {
    let n = parse_as_u64(args, "n")? as usize;
    let language = Language::from_str(&parse_as_string(args, "language_code")?)?;
    let common_words_gazetteer_name = parse_as_opt_string(args, "common_words_gazetteer_name")?;
    let use_stemming = parse_as_bool(args, "use_stemming")?;
    let common_words_gazetteer: Option<Arc<_>> = if let Some(gazetteer) = common_words_gazetteer_name
        .map(|name| get_gazetteer(name, language)) {
        Some(gazetteer?)
    } else {
        None
    };
    let stemmer_opt: Option<Arc<_>> = if use_stemming {
        let stemmer = get_stemmer(language)
            .ok_or_else(|| format_err!("No stemmer found for language {:?}", language))?;
        Some(stemmer)
    } else {
        None
    };
    Ok(FeatureFunction::new(
        &format!("ngram_{}", n),
        offsets,
        move |tokens, token_index| {
            features::ngram(
                tokens,
                token_index,
                n,
                stemmer_opt.as_ref().map(|s| s.as_ref()),
                common_words_gazetteer.as_ref().map(|s| s.as_ref()),
            )
        },
    ))
}

fn shape_ngram_feature_function(
    args: &HashMap<String, ::serde_json::Value>,
    offsets: Vec<i32>,
) -> Result<FeatureFunction> {
    let n = parse_as_u64(args, "n")? as usize;
    Ok(FeatureFunction::new(
        &format!("shape_ngram_{}", n),
        offsets,
        move |t, i| features::shape(t, i, n),
    ))
}

fn prefix_feature_function(
    args: &HashMap<String, ::serde_json::Value>,
    offsets: Vec<i32>,
) -> Result<FeatureFunction> {
    let n = parse_as_u64(args, "prefix_size")? as usize;
    Ok(FeatureFunction::new(
        &format!("prefix_{}", n),
        offsets,
        move |t, i| features::prefix(&t[i].value, n),
    ))
}

fn suffix_feature_function(
    args: &HashMap<String, ::serde_json::Value>,
    offsets: Vec<i32>,
) -> Result<FeatureFunction> {
    let n = parse_as_u64(args, "suffix_size")? as usize;
    Ok(FeatureFunction::new(
        &format!("suffix_{}", n),
        offsets,
        move |t, i| features::suffix(&t[i].value, n),
    ))
}

fn entity_match_feature_function(
    args: &HashMap<String, ::serde_json::Value>,
    offsets: &[i32],
) -> Result<Vec<FeatureFunction>> {
    let collections = parse_as_vec_of_vec(args, "collections")?;
    let language = Language::from_str(&parse_as_string(args, "language_code")?)?;
    let tagging_scheme_code = parse_as_u64(args, "tagging_scheme_code")? as u8;
    let use_stemming = parse_as_bool(args, "use_stemming")?;
    let tagging_scheme = TaggingScheme::from_u8(tagging_scheme_code)?;
    let opt_stemmer: Option<Arc<_>> = if use_stemming {
        let stemmer = get_stemmer(language)
            .ok_or_else(|| format_err!("No stemmer found for language {:?}", language))?;
        Some(stemmer)
    } else {
        None
    };
    collections
        .into_iter()
        .map(|(entity_name, values)| {
            let opt_stemmer_cloned = opt_stemmer.clone();
            let entity_gazetteer = HashSetGazetteer::from(values.into_iter());
            Ok(FeatureFunction::new(
                &format!("entity_match_{}", entity_name),
                offsets.to_vec(),
                move |tokens, token_index| {
                    features::get_gazetteer_match(
                        tokens,
                        token_index,
                        &entity_gazetteer,
                        opt_stemmer_cloned.as_ref().map(|s| s.as_ref()),
                        tagging_scheme,
                    )
                },
            ))
        })
        .collect()
}

fn builtin_entity_match_feature_function(
    args: &HashMap<String, ::serde_json::Value>,
    offsets: &[i32],
    shared_resources: Arc<SharedResources>
) -> Result<Vec<FeatureFunction>> {
    let builtin_entity_labels = parse_as_vec_string(args, "entity_labels")?;
    let tagging_scheme_code = parse_as_u64(args, "tagging_scheme_code")? as u8;
    let tagging_scheme = TaggingScheme::from_u8(tagging_scheme_code)?;
    builtin_entity_labels
        .into_iter()
        .map(|label| {
            let builtin_entity_kind = BuiltinEntityKind::from_identifier(&label)?;
            let shared_resources_cloned = shared_resources.clone();
            Ok(FeatureFunction::new(
                &format!("builtin_entity_match_{}", &label),
                offsets.to_vec(),
                move |tokens, token_index|
                    features::get_builtin_entity_match(
                        tokens,
                        token_index,
                        &shared_resources_cloned.builtin_entity_parser,
                        builtin_entity_kind,
                        tagging_scheme)
            ))
        })
        .collect()
}

fn word_cluster_feature_function(
    args: &HashMap<String, ::serde_json::Value>,
    offsets: Vec<i32>,
) -> Result<FeatureFunction> {
    let cluster_name = parse_as_string(args, "cluster_name")?;
    let language = Language::from_str(&parse_as_string(args, "language_code")?)?;
    let word_clusterer = get_word_clusterer(cluster_name.clone(), language)?;
    Ok(FeatureFunction::new(
        &format!("word_cluster_{}", cluster_name),
        offsets,
        move |tokens, token_index| features::get_word_cluster(tokens, token_index, word_clusterer.as_ref()),
    ))
}

fn parse_as_string(args: &HashMap<String, ::serde_json::Value>, arg_name: &str) -> Result<String> {
    Ok(args.get(arg_name)
        .ok_or_else(|| format_err!("can't retrieve '{}' parameter", arg_name))?
        .as_str()
        .ok_or_else(|| format_err!("'{}' isn't a string", arg_name))?
        .to_string())
}

fn parse_as_opt_string(
    args: &HashMap<String, ::serde_json::Value>,
    arg_name: &str,
) -> Result<Option<String>> {
    Ok(args.get(arg_name)
        .ok_or_else(|| format_err!("can't retrieve '{}' parameter", arg_name))?
        .as_str()
        .map(|s| s.to_string()))
}

fn parse_as_vec_string(
    args: &HashMap<String, ::serde_json::Value>,
    arg_name: &str,
) -> Result<Vec<String>> {
    args.get(arg_name)
        .ok_or_else(|| format_err!("can't retrieve '{}' parameter", arg_name))?
        .as_array()
        .ok_or_else(|| format_err!("'{}' isn't an array", arg_name))?
        .iter()
        .map(|v| {
            Ok(v.as_str()
                .ok_or_else(|| format_err!("'{}' is not a string", v))?
                .to_string())
        })
        .collect()
}

fn parse_as_vec_of_vec(
    args: &HashMap<String, ::serde_json::Value>,
    arg_name: &str,
) -> Result<Vec<(String, Vec<String>)>> {
    args.get(arg_name)
        .ok_or_else(|| format_err!("can't retrieve '{}' parameter", arg_name))?
        .as_object()
        .ok_or_else(|| format_err!("'{}' isn't a map", arg_name))?
        .into_iter()
        .map(|(k, v)| {
            let values: Result<Vec<_>> = v.as_array()
                .ok_or_else(|| format_err!("'{}' is not a vec", v))?
                .into_iter()
                .map(|item| {
                    Ok(item.as_str()
                        .ok_or_else(|| format_err!("'{}' is not a string", item))?
                        .to_string())
                })
                .collect();
            Ok((k.to_string(), values?))
        })
        .collect()
}

fn parse_as_bool(args: &HashMap<String, ::serde_json::Value>, arg_name: &str) -> Result<bool> {
    Ok(args.get(arg_name)
        .ok_or_else(|| format_err!("can't retrieve '{}' parameter", arg_name))?
        .as_bool()
        .ok_or_else(|| format_err!("'{}' isn't a bool", arg_name))?)
}

fn parse_as_u64(args: &HashMap<String, ::serde_json::Value>, arg_name: &str) -> Result<u64> {
    Ok(args.get(arg_name)
        .ok_or_else(|| format_err!("can't retrieve '{}' parameter", arg_name))?
        .as_u64()
        .ok_or_else(|| format_err!("'{}' isn't a u64", arg_name))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    use nlu_utils::language::Language;
    use nlu_utils::token::tokenize;

    #[test]
    fn compute_features_works() {
        let language = Language::EN;
        let fp = ProbabilisticFeatureProcessor {
            functions: vec![
                FeatureFunction::new("Toto", vec![0], |_, i| {
                    if i == 0 {
                        None
                    } else {
                        Some("Foobar".to_string())
                    }
                }),
            ],
        };

        let computed_features =
            fp.compute_features(&tokenize("hello world how are you ?", language).as_slice());

        assert_eq!(computed_features.len(), 6);
        assert_eq!(computed_features[0], vec![]);
        for i in 1..5 {
            assert_eq!(
                computed_features[i],
                vec![("Toto".to_string(), "Foobar".to_string())]
            );
        }
    }

    #[test]
    fn offset_works() {
        let language = Language::EN;
        let fp = ProbabilisticFeatureProcessor {
            functions: vec![
                FeatureFunction::new("Toto", vec![-2, 0, 2, 4], |x, i| {
                    if i == 0 {
                        None
                    } else {
                        Some(x[i].value.clone())
                    }
                }),
                FeatureFunction::new("Tutu", vec![2], |_, i| {
                    if i != 3 {
                        None
                    } else {
                        Some("Foobar".to_string())
                    }
                }),
            ],
        };

        let computed_features =
            fp.compute_features(&tokenize("hello world how are you ?", language).as_slice());
        assert_eq!(
            computed_features,
            vec![
                vec![
                    ("Toto[+2]".to_string(), "how".to_string()),
                    ("Toto[+4]".to_string(), "you".to_string()),
                ],
                vec![
                    ("Toto".to_string(), "world".to_string()),
                    ("Toto[+2]".to_string(), "are".to_string()),
                    ("Toto[+4]".to_string(), "?".to_string()),
                    ("Tutu[+2]".to_string(), "Foobar".to_string()),
                ],
                vec![
                    ("Toto".to_string(), "how".to_string()),
                    ("Toto[+2]".to_string(), "you".to_string()),
                ],
                vec![
                    ("Toto[-2]".to_string(), "world".to_string()),
                    ("Toto".to_string(), "are".to_string()),
                    ("Toto[+2]".to_string(), "?".to_string()),
                ],
                vec![
                    ("Toto[-2]".to_string(), "how".to_string()),
                    ("Toto".to_string(), "you".to_string()),
                ],
                vec![
                    ("Toto[-2]".to_string(), "are".to_string()),
                    ("Toto".to_string(), "?".to_string()),
                ],
            ]
        );
    }
}
