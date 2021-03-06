use rand;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use regex::Regex;

use std::collections::HashMap;
use std::iter;
use std::vec::Vec;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Atom {
    idx: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum BackwardsPred {
    Plus,
    Lt,
    Gt,
    Lte,
    Gte,
    ModNeg,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub string: Atom,
    backwards_pred: Option<BackwardsPred>,
    is_var: bool,
    is_negated: bool,
    is_side: bool,
    is_stage: bool,
    open_depth: i32,
    close_depth: i32,
}

impl PartialEq for Token {
    fn eq(&self, other: &Token) -> bool {
        self.string == other.string
            && self.is_side == other.is_side
            && self.open_depth == other.open_depth
            && self.close_depth == other.close_depth
    }
}
impl Eq for Token {}

impl Token {
    fn new(
        string: &str,
        open_depth: i32,
        close_depth: i32,
        string_cache: &mut StringCache,
    ) -> Token {
        let mut string = string;

        let mut is_negated = false;
        let mut is_side = false;
        let mut is_stage = false;
        match string.chars().next().expect("first_char") {
            '!' => {
                is_negated = true;
                string = string.get(1..).expect("get");
            }
            '^' => {
                is_side = true;
            }
            '#' => {
                is_stage = true;
            }
            _ => {}
        }

        let mut chars = string.chars();
        let first_char = chars.next();
        let is_var = first_char.expect("first_char").is_ascii_uppercase()
            && chars.all(|c| c.is_numeric() || !c.is_ascii_lowercase());

        let backwards_pred = match string {
            "+" => Some(BackwardsPred::Plus),
            "<" => Some(BackwardsPred::Lt),
            ">" => Some(BackwardsPred::Gt),
            "<=" => Some(BackwardsPred::Lte),
            ">=" => Some(BackwardsPred::Gte),
            "%%" => Some(BackwardsPred::ModNeg),

            _ => None,
        };

        let atom = string_cache.to_atom(string);

        Token {
            string: atom,
            backwards_pred,
            is_var,
            is_negated,
            is_side,
            is_stage,
            open_depth,
            close_depth,
        }
    }

    pub fn as_str<'a>(&self, string_cache: &'a StringCache) -> &'a str {
        string_cache.from_atom(self.string)
    }
}

pub type Phrase = Vec<Token>;
type Match = (Atom, Phrase);

// https://stackoverflow.com/questions/44246722/is-there-any-way-to-create-an-alias-of-a-specific-fnmut
pub trait SideInput: FnMut(&Phrase) -> Option<Phrase> {}
impl<F> SideInput for F where F: FnMut(&Phrase) -> Option<Phrase> {}

#[derive(Debug, Eq, PartialEq)]
struct Rule {
    id: i32,
    inputs: Vec<Phrase>,
    outputs: Vec<Phrase>,
}

impl Rule {
    fn new(inputs: Vec<Phrase>, outputs: Vec<Phrase>) -> Rule {
        Rule::new_with_id(0, inputs, outputs)
    }

    fn new_with_id(id: i32, inputs: Vec<Phrase>, outputs: Vec<Phrase>) -> Rule {
        Rule {
            id,
            inputs,
            outputs,
        }
    }
}

pub struct Context {
    rules: Vec<Rule>,
    pub state: Vec<Phrase>,
    pub string_cache: StringCache,
    quiescence: bool,
    rng: SmallRng,
}

pub struct StringCache {
    atom_to_string: Vec<String>,
    string_to_atom: HashMap<String, Atom>,
}

impl StringCache {
    pub fn new() -> StringCache {
        StringCache {
            atom_to_string: vec![],
            string_to_atom: HashMap::new(),
        }
    }

    pub fn to_atom(&mut self, text: &str) -> Atom {
        if let Some(atom) = self.to_existing_atom(text) {
            return atom;
        }

        let idx = self.atom_to_string.len();
        let atom = Atom { idx };

        self.atom_to_string.push(text.to_string());
        self.string_to_atom.insert(text.to_string(), atom);

        atom
    }

    pub fn to_existing_atom(&self, text: &str) -> Option<Atom> {
        self.string_to_atom.get(text).cloned()
    }

    pub fn from_atom<'a>(&'a self, atom: Atom) -> &'a str {
        &self.atom_to_string[atom.idx]
    }
}

impl Context {
    pub fn from_text(text: &str) -> Context {
        let text = text.replace("()", "qui");
        let lines = text.split("\n");

        let mut string_cache = StringCache::new();

        let parse_rule = |id: i32, string: &str, string_cache: &mut StringCache| {
            let mut r = string.split(" =");

            let (dollars, inputs): (Vec<_>, Vec<_>) = r
                .next()
                .expect("r[0]")
                .split(" . ")
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .partition(|s| s.chars().next().expect("char") == '$');

            let outputs = if let Some(r1) = r.next() { r1 } else { "" }
                .split(" . ")
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());

            let dollars: Vec<_> = dollars.iter().map(|s| s.split_at(1).1).collect();

            let inputs: Vec<_> = inputs
                .iter()
                .chain(dollars.iter())
                .cloned()
                .map(|s| tokenize(s, string_cache))
                .collect();

            let outputs = outputs
                .chain(dollars)
                .map(|s| tokenize(s, string_cache))
                .collect();

            return Rule::new_with_id(id, inputs, outputs);
        };

        let get_label = |line| {
            lazy_static! {
                static ref RE: Regex = Regex::new(r"^(#[^:]*):\s*$").unwrap();
            }

            RE.captures(line)
                .map(|caps| caps.get(1).unwrap().as_str().trim())
        };

        let get_init = |line: &String, string_cache: &mut StringCache| {
            if !line.contains(" =") && !line.is_empty() {
                return Some(
                    line.split(" . ")
                        .map(|s| tokenize(s, string_cache))
                        .collect::<Vec<_>>(),
                );
            } else {
                return None;
            }
        };

        let get_rule = |(i, line): (usize, &String), string_cache: &mut StringCache| {
            if line.contains(" =") && !line.is_empty() {
                return Some(parse_rule(i as i32, line, string_cache));
            } else {
                return None;
            }
        };

        let mut out_lines = vec![];

        let mut attach = None;

        for line in lines {
            let line = line.trim();

            if line.is_empty() {
                attach = None;
            } else {
                let label = get_label(line);

                if label.is_some() {
                    attach = label;
                } else if let Some(attach) = attach {
                    // discard the current label on quiescence
                    if line
                        .split(|c| c == '.' || c == '=')
                        .any(|s| s.trim() == "qui")
                    {
                        out_lines.push(format!("{} . {}", attach, line));
                    } else {
                        out_lines.push(format!("{} . {} . {}", attach, line, attach));
                    }
                } else {
                    out_lines.push(String::from(line));
                }
            }
        }

        let state = out_lines
            .iter()
            .map(|l| get_init(l, &mut string_cache))
            .filter(|v| v.is_some())
            .flat_map(|v| v.expect("v"))
            .collect::<Vec<_>>();

        let rules = out_lines
            .iter()
            .enumerate()
            .map(|l| get_rule(l, &mut string_cache))
            .filter(|v| v.is_some())
            .map(|v| v.expect("v"))
            .collect::<Vec<_>>();

        let seed = [
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
        ];

        let rng = SmallRng::from_seed(seed);

        Context {
            state,
            rules,
            string_cache,
            quiescence: false,
            rng,
        }
    }

    pub fn to_atom(&mut self, text: &str) -> Atom {
        self.string_cache.to_atom(text)
    }

    pub fn to_existing_atom(&self, text: &str) -> Option<Atom> {
        self.string_cache.to_existing_atom(text)
    }

    pub fn append_state(&mut self, text: &str) {
        self.state.push(tokenize(text, &mut self.string_cache));
    }

    pub fn print(&self) {
        println!("state:");
        print_state(&self.state, &self.string_cache);

        println!("\nrules:");
        let mut rules = self
            .rules
            .iter()
            .map(|r| rule_to_string(r, &self.string_cache))
            .collect::<Vec<_>>();
        rules.sort();

        for r in rules {
            println!("{}", r);
        }
    }

    pub fn find_phrase<'a>(&'a self, s1: Option<&str>) -> Option<&'a Phrase> {
        self.find_phrase2(s1, None)
    }

    pub fn find_phrase2<'a>(&'a self, s1: Option<&str>, s2: Option<&str>) -> Option<&'a Phrase> {
        self.find_phrase3(s1, s2, None)
    }

    pub fn find_phrase3<'a>(
        &'a self,
        s1: Option<&str>,
        s2: Option<&str>,
        s3: Option<&str>,
    ) -> Option<&'a Phrase> {
        self.find_phrase4(s1, s2, s3, None)
    }

    pub fn find_phrase4<'a>(
        &'a self,
        s1: Option<&str>,
        s2: Option<&str>,
        s3: Option<&str>,
        s4: Option<&str>,
    ) -> Option<&'a Phrase> {
        self.find_phrase5(s1, s2, s3, s4, None)
    }

    pub fn find_phrase5<'a>(
        &'a self,
        s1: Option<&str>,
        s2: Option<&str>,
        s3: Option<&str>,
        s4: Option<&str>,
        s5: Option<&str>,
    ) -> Option<&'a Phrase> {
        let mut atom1 = None;
        let mut atom2 = None;
        let mut atom3 = None;
        let mut atom4 = None;
        let mut atom5 = None;

        if let Some(s) = s1 {
            if let Some(atom) = self.to_existing_atom(s) {
                atom1 = Some(atom);
            } else {
                return None;
            }
        };

        if let Some(s) = s2 {
            if let Some(atom) = self.to_existing_atom(s) {
                atom2 = Some(atom);
            } else {
                return None;
            }
        };

        if let Some(s) = s3 {
            if let Some(atom) = self.to_existing_atom(s) {
                atom3 = Some(atom);
            } else {
                return None;
            }
        };

        if let Some(s) = s4 {
            if let Some(atom) = self.to_existing_atom(s) {
                atom4 = Some(atom);
            } else {
                return None;
            }
        };

        if let Some(s) = s5 {
            if let Some(atom) = self.to_existing_atom(s) {
                atom5 = Some(atom);
            } else {
                return None;
            }
        };

        for p in self.state.iter() {
            match (
                p.get(0).map(|t| &t.string),
                p.get(1).map(|t| &t.string),
                p.get(2).map(|t| &t.string),
                p.get(3).map(|t| &t.string),
                p.get(4).map(|t| &t.string),
            ) {
                (s1, s2, s3, s4, s5)
                    if (atom1.is_none() || s1 == atom1.as_ref())
                        && (atom2.is_none() || s2 == atom2.as_ref())
                        && (atom3.is_none() || s3 == atom3.as_ref())
                        && (atom4.is_none() || s4 == atom4.as_ref())
                        && (atom5.is_none() || s5 == atom5.as_ref()) =>
                {
                    return Some(p);
                }
                _ => (),
            };
        }

        return None;
    }

    pub fn find_phrases<'a>(&'a self, s1: Option<&str>) -> Vec<&'a Phrase> {
        self.find_phrases2(s1, None)
    }

    pub fn find_phrases2<'a>(&'a self, s1: Option<&str>, s2: Option<&str>) -> Vec<&'a Phrase> {
        self.find_phrases3(s1, s2, None)
    }

    pub fn find_phrases3<'a>(
        &'a self,
        s1: Option<&str>,
        s2: Option<&str>,
        s3: Option<&str>,
    ) -> Vec<&'a Phrase> {
        self.find_phrases4(s1, s2, s3, None)
    }

    pub fn find_phrases4<'a>(
        &'a self,
        s1: Option<&str>,
        s2: Option<&str>,
        s3: Option<&str>,
        s4: Option<&str>,
    ) -> Vec<&'a Phrase> {
        self.find_phrases5(s1, s2, s3, s4, None)
    }

    pub fn find_phrases5<'a>(
        &'a self,
        s1: Option<&str>,
        s2: Option<&str>,
        s3: Option<&str>,
        s4: Option<&str>,
        s5: Option<&str>,
    ) -> Vec<&'a Phrase> {
        let mut atom1 = None;
        let mut atom2 = None;
        let mut atom3 = None;
        let mut atom4 = None;
        let mut atom5 = None;

        if let Some(s) = s1 {
            if let Some(atom) = self.to_existing_atom(s) {
                atom1 = Some(atom);
            } else {
                return vec![];
            }
        };

        if let Some(s) = s2 {
            if let Some(atom) = self.to_existing_atom(s) {
                atom2 = Some(atom);
            } else {
                return vec![];
            }
        };

        if let Some(s) = s3 {
            if let Some(atom) = self.to_existing_atom(s) {
                atom3 = Some(atom);
            } else {
                return vec![];
            }
        };

        if let Some(s) = s4 {
            if let Some(atom) = self.to_existing_atom(s) {
                atom4 = Some(atom);
            } else {
                return vec![];
            }
        };

        if let Some(s) = s5 {
            if let Some(atom) = self.to_existing_atom(s) {
                atom5 = Some(atom);
            } else {
                return vec![];
            }
        };

        return self
            .state
            .iter()
            .filter(|p| {
                match (
                    p.get(0).map(|t| &t.string),
                    p.get(1).map(|t| &t.string),
                    p.get(2).map(|t| &t.string),
                    p.get(3).map(|t| &t.string),
                    p.get(4).map(|t| &t.string),
                ) {
                    (s1, s2, s3, s4, s5) => {
                        (atom1.is_none() || s1 == atom1.as_ref())
                            && (atom2.is_none() || s2 == atom2.as_ref())
                            && (atom3.is_none() || s3 == atom3.as_ref())
                            && (atom4.is_none() || s4 == atom4.as_ref())
                            && (atom5.is_none() || s5 == atom5.as_ref())
                    }
                }
            })
            .collect();
    }
}

pub fn update<F>(context: &mut Context, mut side_input: F)
where
    F: SideInput,
{
    let rules = &mut context.rules;
    let state = &mut context.state;

    let qui: Phrase = vec![Token::new("qui", 0, 0, &mut context.string_cache)];

    loop {
        let mut matching_rule = None;

        // shuffle rules so that each has an equal chance of selection.
        context.rng.shuffle(rules);

        // shuffle state so that a given rule with multiple potential
        // matches does not always match the same permutation of state.
        context.rng.shuffle(state);

        if context.quiescence {
            state.push(qui.clone());
        }

        {
            let state_stages = state.iter().filter(|p| p[0].is_stage).collect::<Vec<_>>();
            for rule in rules.iter() {
                // early exit if rule stages can't match state
                for input in rule
                    .inputs
                    .iter()
                    .filter(|p| p[0].is_stage && !p[0].is_negated)
                {
                    if !state_stages
                        .iter()
                        .any(|p| test_match_without_variables(input, p))
                    {
                        continue;
                    }
                }

                if let Some(rule) = rule_matches_state(
                    &rule,
                    &state,
                    &mut context.rng,
                    &mut side_input,
                    &mut context.string_cache,
                ) {
                    matching_rule = Some(rule);
                    break;
                }
            }
        }

        if context.quiescence {
            context.quiescence = false;

            if matching_rule.is_none() {
                assert!(
                    state
                        .iter()
                        .enumerate()
                        .filter(|&(_, p)| **p == qui[..])
                        .collect::<Vec<_>>()
                        == vec![(state.len() - 1, &qui.clone())],
                    "expected 1 * () at end of state"
                );

                let idx = state.len() - 1;
                state.remove(idx);

                return;
            }
        }

        if let Some(ref matching_rule) = matching_rule {
            let inputs = &matching_rule.inputs;
            let outputs = &matching_rule.outputs;

            for input in inputs.iter() {
                let remove_idx = state.iter().position(|v| v == input);
                state.swap_remove(remove_idx.expect("remove_idx"));
            }

            for output in outputs.iter() {
                state.push(output.clone());
            }
        } else {
            context.quiescence = true;
        }
    }
}

// Checks whether the rule's forward and backward predicates match the state.
// Returns a new rule with all variables resolved, with backwards/side
// predicates removed.
fn rule_matches_state<R, F>(
    r: &Rule,
    state: &Vec<Phrase>,
    rng: &mut R,
    side_input: &mut F,
    string_cache: &mut StringCache,
) -> Option<Rule>
where
    R: Rng,
    F: SideInput,
{
    let inputs = &r.inputs;
    let outputs = &r.outputs;

    let mut permutation_count = 1;

    // per input, a list of states that match the input.
    // indexed by input using start index and counts in the following vectors.
    let mut input_state_matches = Vec::with_capacity(inputs.len() * state.len());

    let mut input_state_match_start_indices = Vec::with_capacity(inputs.len());
    let mut input_state_match_counts = Vec::with_capacity(inputs.len());

    let mut backwards_pred = vec![];
    let mut side_pred = vec![];
    let mut negated_pred = vec![];

    for (i_i, input) in inputs.iter().enumerate() {
        let mut count = 0;
        if is_backwards_pred(input) {
            backwards_pred.push(i_i);
        } else if is_side_pred(input) {
            // TODO: exit early if we already know that side predicate won't match
            side_pred.push(i_i);
        } else if is_negated_pred(input) {
            negated_pred.push(i_i);
        } else {
            for (s_i, p) in state.iter().enumerate() {
                if test_match_without_variables(input, p) {
                    input_state_matches.push(s_i);
                    count += 1;
                }
            }

            if count == 0 {
                return None;
            }

            permutation_count *= count;
        }

        input_state_match_start_indices.push(input_state_matches.len() - count);
        input_state_match_counts.push(count);
    }

    // precompute values required for deriving branch indices.
    let mut input_rev_permutation_counts = vec![1; inputs.len()];
    let mut acc = 1;
    for (i, count) in input_state_match_counts.iter().enumerate().rev() {
        if *count > 0 {
            acc = acc * count;
        }

        if i > 0 {
            input_rev_permutation_counts[i - 1] = acc;
        }
    }

    let mut variables_matched = vec![];

    let mut states_matched_bool = vec![false; state.len()];
    let mut states_matched = vec![];

    'outer: for p_i in 0..permutation_count {
        variables_matched.clear();

        for s_i in states_matched.drain(..) {
            states_matched_bool[s_i] = false;
        }

        assert!(states_matched_bool.iter().all(|b| !b));

        // iterate across the graph of permutations from root to leaf, where each
        // level of the tree is an input, and each branch is a match against a state.
        for (i_i, input) in inputs.iter().enumerate() {
            let match_count = input_state_match_counts[i_i];

            if match_count == 0 {
                continue;
            }

            let branch_idx = (p_i / input_rev_permutation_counts[i_i]) % match_count;

            let input_state_match_idx = input_state_match_start_indices[i_i] + branch_idx;
            let s_i = input_state_matches[input_state_match_idx];

            // a previous input in this permutation has already matched the state being checked
            if states_matched_bool[s_i] {
                continue 'outer;
            } else {
                states_matched.push(s_i);
                states_matched_bool[s_i] = true;
            }

            if let Some(ref mut result) =
                match_variables_with_existing(input, &state[s_i], &variables_matched)
            {
                variables_matched.append(result);
            } else {
                continue 'outer;
            }
        }

        for input in backwards_pred.iter().map(|&i| &inputs[i]) {
            let mut extra_matches =
                match_backwards_variables(input, &variables_matched, string_cache);

            if let Some(ref mut extra_matches) = extra_matches {
                variables_matched.append(extra_matches);
            } else {
                continue 'outer;
            }
        }

        for input in side_pred.iter().map(|&i| &inputs[i]) {
            let mut extra_matches = match_side_variables(input, &variables_matched, side_input);

            if let Some(ref mut extra_matches) = extra_matches {
                variables_matched.append(extra_matches);
            } else {
                continue 'outer;
            }
        }

        for input in negated_pred.iter().map(|&i| &inputs[i]) {
            // check negated predicates last, so that we know about all variables
            // from the backwards and side predicates
            if state
                .iter()
                .enumerate()
                .filter(|&(s_i, _)| !states_matched_bool[s_i])
                .any(|(_, s)| match_variables_with_existing(input, s, &variables_matched).is_some())
            {
                continue 'outer;
            }
        }

        let mut forward_concrete = vec![];
        let mut outputs_concrete = vec![];

        for v in inputs.iter() {
            if !is_backwards_pred(v) && !is_side_pred(v) && !is_negated_pred(v) {
                forward_concrete.push(assign_vars(v, &variables_matched));
            }
        }

        for v in outputs.iter() {
            if is_side_pred(v) {
                let pred = assign_vars(v, &variables_matched);

                evaluate_side_pred(&pred, side_input);
            } else {
                outputs_concrete.push(assign_vars(v, &variables_matched));
            }
        }

        return Some(Rule::new_with_id(r.id, forward_concrete, outputs_concrete));
    }

    return None;
}

fn match_backwards_variables(
    pred: &Phrase,
    existing_matches: &Vec<Match>,
    string_cache: &mut StringCache,
) -> Option<Vec<Match>> {
    let pred = assign_vars(pred, existing_matches);

    evaluate_backwards_pred(&pred, string_cache).and_then(|eval_result| {
        match_variables_with_existing(&pred, &eval_result, existing_matches)
    })
}

fn match_side_variables<F>(
    pred: &Phrase,
    existing_matches: &Vec<Match>,
    side_input: &mut F,
) -> Option<Vec<Match>>
where
    F: SideInput,
{
    let pred = assign_vars(pred, existing_matches);

    evaluate_side_pred(&pred, side_input).and_then(|eval_result| {
        match_variables_with_existing(&pred, &eval_result, existing_matches)
    })
}

fn assign_vars(tokens: &Phrase, matches: &Vec<Match>) -> Phrase {
    let mut result: Phrase = vec![];

    for token in tokens.iter() {
        if token.is_var {
            if let Some(&(_, ref tokens)) = matches.iter().find(|&&(ref s, _)| *s == token.string) {
                let mut tokens = tokens.clone();
                let len = tokens.len();

                if len == 1 {
                    tokens[0].is_negated = token.is_negated;
                    tokens[0].open_depth = token.open_depth;
                    tokens[len - 1].close_depth = token.close_depth;
                } else {
                    tokens[0].is_negated = token.is_negated;
                    if token.open_depth > 0 {
                        tokens[0].open_depth += token.open_depth
                    }
                    if token.close_depth > 0 {
                        tokens[len - 1].close_depth += token.close_depth
                    }
                }

                result.append(&mut tokens);

                continue;
            }
        }

        result.push(token.clone());
    }

    return result;
}

fn is_backwards_pred(tokens: &Phrase) -> bool {
    if tokens.len() == 0 {
        return false;
    }

    return tokens[0].backwards_pred.is_some();
}

fn is_side_pred(tokens: &Phrase) -> bool {
    if tokens.len() == 0 {
        return false;
    }

    return tokens[0].is_side;
}

fn is_negated_pred(tokens: &Phrase) -> bool {
    if tokens.len() == 0 {
        return false;
    }

    return tokens[0].is_negated;
}

fn evaluate_backwards_pred(tokens: &Phrase, string_cache: &mut StringCache) -> Option<Phrase> {
    match tokens[0].backwards_pred {
        Some(BackwardsPred::Plus) => {
            use std::str::FromStr;

            let n1 = f32::from_str(tokens[1].as_str(string_cache));
            let n2 = f32::from_str(tokens[2].as_str(string_cache));
            let n3 = f32::from_str(tokens[3].as_str(string_cache));

            return match (n1, n2, n3) {
                (Ok(v1), Ok(v2), Err(_)) => Some(vec![
                    tokens[0].clone(),
                    tokens[1].clone(),
                    tokens[2].clone(),
                    Token::new(&(v1 + v2).to_string(), 0, 1, string_cache),
                ]),
                (Ok(v1), Err(_), Ok(v3)) => Some(vec![
                    tokens[0].clone(),
                    tokens[1].clone(),
                    Token::new(&(v3 - v1).to_string(), 0, 0, string_cache),
                    tokens[3].clone(),
                ]),
                (Err(_), Ok(v2), Ok(v3)) => Some(vec![
                    tokens[0].clone(),
                    Token::new(&(v3 - v2).to_string(), 0, 0, string_cache),
                    tokens[2].clone(),
                    tokens[3].clone(),
                ]),
                (Ok(v1), Ok(v2), Ok(v3)) if v1 + v2 == v3 => Some(tokens.clone()),
                _ => None,
            };
        }
        Some(BackwardsPred::Lt) => {
            use std::str::FromStr;

            let n1 = f32::from_str(tokens[1].as_str(string_cache));
            let n2 = f32::from_str(tokens[2].as_str(string_cache));

            return match (n1, n2) {
                (Ok(v1), Ok(v2)) if v1 < v2 => Some(tokens.clone()),
                _ => None,
            };
        }
        Some(BackwardsPred::Gt) => {
            use std::str::FromStr;

            let n1 = f32::from_str(tokens[1].as_str(string_cache));
            let n2 = f32::from_str(tokens[2].as_str(string_cache));

            return match (n1, n2) {
                (Ok(v1), Ok(v2)) if v1 > v2 => Some(tokens.clone()),
                _ => None,
            };
        }
        Some(BackwardsPred::Lte) => {
            use std::str::FromStr;

            let n1 = f32::from_str(tokens[1].as_str(string_cache));
            let n2 = f32::from_str(tokens[2].as_str(string_cache));

            return match (n1, n2) {
                (Ok(v1), Ok(v2)) if v1 <= v2 => Some(tokens.clone()),
                _ => None,
            };
        }
        Some(BackwardsPred::Gte) => {
            use std::str::FromStr;

            let n1 = f32::from_str(tokens[1].as_str(string_cache));
            let n2 = f32::from_str(tokens[2].as_str(string_cache));

            return match (n1, n2) {
                (Ok(v1), Ok(v2)) if v1 >= v2 => Some(tokens.clone()),
                _ => None,
            };
        }
        Some(BackwardsPred::ModNeg) => {
            use std::str::FromStr;

            let n1 = f32::from_str(tokens[1].as_str(string_cache));
            let n2 = f32::from_str(tokens[2].as_str(string_cache));
            let n3 = f32::from_str(tokens[3].as_str(string_cache));

            let mod_neg = |x: f32, n: f32| x - n * (x / n).floor();

            return match (n1, n2, n3) {
                (Ok(v1), Ok(v2), Err(_)) => Some(vec![
                    tokens[0].clone(),
                    tokens[1].clone(),
                    tokens[2].clone(),
                    Token::new(&mod_neg(v1, v2).to_string(), 0, 1, string_cache),
                ]),
                _ => None,
            };
        }
        _ => None,
    }
}

fn evaluate_side_pred<F>(tokens: &Phrase, side_input: &mut F) -> Option<Phrase>
where
    F: SideInput,
{
    side_input(tokens)
}

fn test_match_without_variables(input_tokens: &Phrase, pred_tokens: &Phrase) -> bool {
    let mut pred_token_iter = pred_tokens.iter();

    let mut input_depth = 0;
    let mut pred_depth = 0;

    for token in input_tokens.iter() {
        let pred_token = pred_token_iter.next();

        input_depth += token.open_depth;

        if let Some(pred_token) = pred_token {
            pred_depth += pred_token.open_depth;

            if !token.is_var {
                if token != pred_token || input_depth != pred_depth {
                    return false;
                }
            } else {
                while input_depth < pred_depth {
                    if let Some(pred_token) = pred_token_iter.next() {
                        pred_depth += pred_token.open_depth;
                        pred_depth -= pred_token.close_depth;
                    } else {
                        return false;
                    }
                }
            }

            pred_depth -= pred_token.close_depth;
        } else {
            return false;
        }

        input_depth -= token.close_depth;
    }

    if let Some(_) = pred_token_iter.next() {
        return false;
    }

    return true;
}

fn match_variables(input_tokens: &Phrase, pred_tokens: &Phrase) -> Option<Vec<Match>> {
    match_variables_with_existing(input_tokens, pred_tokens, &vec![])
}

fn match_variables_with_existing(
    input_tokens: &Phrase,
    pred_tokens: &Phrase,
    existing_matches: &Vec<Match>,
) -> Option<Vec<Match>> {
    let mut pred_token_iter = pred_tokens.iter();
    let mut result = vec![];

    let mut input_depth = 0;
    let mut pred_depth = 0;

    for token in input_tokens.iter() {
        let pred_token = pred_token_iter.next();

        input_depth += token.open_depth;

        if let Some(pred_token) = pred_token {
            pred_depth += pred_token.open_depth;

            if !token.is_var {
                if token != pred_token || input_depth != pred_depth {
                    return None;
                }
            } else {
                let mut matches = vec![pred_token.clone()];

                while input_depth < pred_depth {
                    if let Some(pred_token) = pred_token_iter.next() {
                        pred_depth += pred_token.open_depth;
                        pred_depth -= pred_token.close_depth;

                        matches.push(pred_token.clone());
                    } else {
                        return None;
                    }
                }

                let len = matches.len();
                if len == 1 {
                    matches[0].open_depth = 0;
                    matches[0].close_depth = 0;
                } else {
                    matches[0].open_depth -= token.open_depth;
                    matches[len - 1].close_depth -= token.close_depth;
                }

                let has_existing_matches = if let Some(&(_, ref existing_matches)) = result
                    .iter()
                    .chain(existing_matches.iter())
                    .find(|&&(ref t, _)| *t == token.string)
                {
                    if *existing_matches != matches {
                        return None;
                    }

                    true
                } else {
                    false
                };

                if !has_existing_matches {
                    result.push((token.string.clone(), matches));
                }
            }

            pred_depth -= pred_token.close_depth;
        } else {
            return None;
        }

        input_depth -= token.close_depth;
    }

    if let Some(_) = pred_token_iter.next() {
        return None;
    }

    return Some(result);
}

fn tokenize(string: &str, string_cache: &mut StringCache) -> Phrase {
    let mut string = format!("({})", string);

    lazy_static! {
        static ref RE1: Regex = Regex::new(r"\(\s*(\S+)\s*\)").unwrap();
    }

    loop {
        // remove instances of brackets surrounding single atoms
        let string1 = string.clone();
        let string2 = RE1.replace_all(&string1, "$1");

        if string1 == string2 {
            break;
        } else {
            string = string2.into_owned();
        }
    }

    lazy_static! {
        static ref RE2: Regex = Regex::new(r"\(|\)|\s+|[^\(\)\s]+").unwrap();
    }

    let tokens = RE2
        .find_iter(&string)
        .map(|m| m.as_str())
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>();

    let mut result = vec![];

    let mut open_depth = 0;
    let mut close_depth = 0;

    for (i, token) in tokens.iter().enumerate() {
        if *token == "(" {
            open_depth += 1;
            continue;
        }

        for j in i + 1..tokens.len() {
            if tokens[j] == ")" {
                close_depth += 1;
            } else {
                break;
            }
        }

        if *token == ")" {
            continue;
        }

        result.push(Token::new(token, open_depth, close_depth, string_cache));
        open_depth = 0;
        close_depth = 0;
    }

    return result;
}

fn random_prime<R: Rng>(rng: &mut R) -> usize {
    #[cfg_attr(rustfmt, rustfmt_skip)]
  let primes = [
    2,    3,    5,    7,    11,   13,   17,   19,   23,   29,   31,   37,
    41,   43,   47,   53,   59,   61,   67,   71,   73,   79,   83,   89,
    97,   101,  103,  107,  109,  113,  127,  131,  137,  139,  149,  151,
    157,  163,  167,  173,  179,  181,  191,  193,  197,  199,  211,  223,
    227,  229,  233,  239,  241,  251,  257,  263,  269,  271,  277,  281,
    283,  293,  307,  311,  313,  317,  331,  337,  347,  349,  353,  359,
    367,  373,  379,  383,  389,  397,  401,  409,  419,  421,  431,  433,
    439,  443,  449,  457,  461,  463,  467,  479,  487,  491,  499,  503,
    509,  521,  523,  541,  547,  557,  563,  569,  571,  577,  587,  593,
    599,  601,  607,  613,  617,  619,  631,  641,  643,  647,  653,  659,
    661,  673,  677,  683,  691,  701,  709,  719,  727,  733,  739,  743,
    751,  757,  761,  769,  773,  787,  797,  809,  811,  821,  823,  827,
    829,  839,  853,  857,  859,  863,  877,  881,  883,  887,  907,  911,
    919,  929,  937,  941,  947,  953,  967,  971,  977,  983,  991,  997,
    1009, 1013, 1019, 1021, 1031, 1033, 1039, 1049, 1051, 1061, 1063, 1069,
    1087, 1091, 1093, 1097, 1103, 1109, 1117, 1123, 1129, 1151, 1153, 1163,
    1171, 1181, 1187, 1193, 1201, 1213, 1217, 1223, 1229, 1231, 1237, 1249,
    1259, 1277, 1279, 1283, 1289, 1291, 1297, 1301, 1303, 1307, 1319, 1321,
    1327, 1361, 1367, 1373, 1381, 1399, 1409, 1423, 1427, 1429, 1433, 1439,
    1447, 1451, 1453, 1459, 1471, 1481, 1483, 1487, 1489, 1493, 1499, 1511,
    1523, 1531, 1543, 1549, 1553, 1559, 1567, 1571, 1579, 1583, 1597, 1601,
    1607, 1609, 1613, 1619, 1621, 1627, 1637, 1657, 1663, 1667, 1669, 1693,
    1697, 1699, 1709, 1721, 1723, 1733, 1741, 1747, 1753, 1759, 1777, 1783,
    1787, 1789, 1801, 1811, 1823, 1831, 1847, 1861, 1867, 1871, 1873, 1877,
    1879, 1889, 1901, 1907, 1913, 1931, 1933, 1949, 1951, 1973, 1979, 1987,
    1993, 1997, 1999, 2003, 2011, 2017, 2027, 2029, 2039, 2053, 2063, 2069,
    2081, 2083, 2087, 2089, 2099, 2111, 2113, 2129, 2131, 2137, 2141, 2143,
    2153, 2161, 2179, 2203, 2207, 2213, 2221, 2237, 2239, 2243, 2251, 2267,
    2269, 2273, 2281, 2287, 2293, 2297, 2309, 2311, 2333, 2339, 2341, 2347,
    2351, 2357, 2371, 2377, 2381, 2383, 2389, 2393, 2399, 2411, 2417, 2423,
    2437, 2441, 2447, 2459, 2467, 2473, 2477, 2503, 2521, 2531, 2539, 2543,
    2549, 2551, 2557, 2579, 2591, 2593, 2609, 2617, 2621, 2633, 2647, 2657,
    2659, 2663, 2671, 2677, 2683, 2687, 2689, 2693, 2699, 2707, 2711, 2713,
    2719, 2729, 2731, 2741, 2749, 2753, 2767, 2777, 2789, 2791, 2797, 2801,
    2803, 2819, 2833, 2837, 2843, 2851, 2857, 2861, 2879, 2887, 2897, 2903,
    2909, 2917, 2927, 2939, 2953, 2957, 2963, 2969, 2971, 2999, 3001
  ];

    primes[rng.gen_range(0, primes.len())]
}

trait IterRand {
    type Item;

    fn iter_rand<R: Rng>(&self, rng: &mut R) -> IterRandState<Self::Item>;
}

impl<T> IterRand for [T] {
    type Item = T;

    fn iter_rand<R: Rng>(&self, rng: &mut R) -> IterRandState<T> {
        let length = self.len();
        let mut rand = 1;

        if length != 1 {
            while length % rand == 0 || rand % length == 0 {
                rand = random_prime(rng);
            }
        }

        let modulo = rand % length;

        IterRandState {
            rand,
            idx: 0,
            length,
            modulo,
            _slice: self,
        }
    }
}

impl<'a, T> iter::Iterator for IterRandState<'a, T> {
    type Item = (usize, &'a T);

    fn next(&mut self) -> Option<(usize, &'a T)> {
        if self.idx == self.length {
            return None;
        } else {
            let result = Some((self.modulo, &self._slice[self.modulo]));

            self.idx += 1;
            self.modulo = (self.modulo + self.rand) % self.length;

            return result;
        }
    }
}

struct IterRandState<'a, T: 'a> {
    rand: usize,
    idx: usize,
    length: usize,
    modulo: usize,
    _slice: &'a [T],
}

fn build_phrase(phrase: &Phrase, string_cache: &StringCache) -> String {
    let mut tokens = vec![];

    for t in phrase.iter() {
        let string = t.as_str(string_cache);

        tokens.push(format!(
            "{}{}{}{}",
            String::from("(").repeat(t.open_depth as usize),
            if t.is_negated { "!" } else { "" },
            string,
            String::from(")").repeat(t.close_depth as usize)
        ));
    }

    return tokens.join(" ");
}

fn print_state(state: &Vec<Phrase>, string_cache: &StringCache) {
    for s in state
        .iter()
        .map(|p| build_phrase(p, string_cache))
        .collect::<Vec<_>>()
    {
        println!("{}", s);
    }
}

fn rule_to_string(rule: &Rule, string_cache: &StringCache) -> String {
    let inputs = rule
        .inputs
        .iter()
        .map(|p| build_phrase(p, string_cache))
        .collect::<Vec<_>>()
        .join(" . ");

    let outputs = rule
        .outputs
        .iter()
        .map(|p| build_phrase(p, string_cache))
        .collect::<Vec<_>>()
        .join(" . ");

    format!("{:5}: {} = {}", rule.id, inputs, outputs)
}
