use crate::error::CanonicalizationError;
use crate::nquads::SerializeNQuads;
use crate::rdf::{BlankNode, Graph, Object, Quad, Subject, Term};
use base16ct::lower::encode_str;
use indexmap::IndexMap;
use itertools::Itertools;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use tracing::{debug, debug_span};

/// **4.3 Canonicalization State**
struct CanonicalizationState {
    /// **blank node to quads map**
    ///   A map that relates a blank node identifier to the quads
    ///   in which they appear in the input dataset.
    blank_node_to_quads_map: BTreeMap<String, Vec<Quad>>,

    /// **hash to blank nodes map**
    ///   A map that relates a hash to a list of blank node identifiers.
    hash_to_blank_node_map: BTreeMap<String, Vec<String>>,

    /// **canonical issuer**
    ///   An identifier issuer, initialized with the prefix c14n, for
    ///   issuing canonical blank node identifiers.
    canonical_issuer: IdentifierIssuer,
}

impl CanonicalizationState {
    const DEFAULT_CANONICAL_IDENTIFER_PREFIX: &str = "c14n";

    fn new() -> CanonicalizationState {
        CanonicalizationState {
            blank_node_to_quads_map: BTreeMap::<String, Vec<Quad>>::new(),
            hash_to_blank_node_map: BTreeMap::<String, Vec<String>>::new(),
            canonical_issuer: IdentifierIssuer::new(Self::DEFAULT_CANONICAL_IDENTIFER_PREFIX),
        }
    }

    fn update_blank_node_to_quads_map(&mut self, dataset: &[Quad]) {
        // **4.5.3 Algorithm**
        // 2) For every quad Q in input dataset:
        for quad in dataset.iter() {
            // 2.1) For each blank node that is a component of Q, add a reference to Q from the map
            // entry for the blank node identifier identifier in the blank node to quads map,
            // creating a new entry if necessary.
            if let Subject::BlankNode(n) = &quad.subject {
                self.blank_node_to_quads_map
                    .entry(n.value().clone())
                    .or_insert_with(Vec::<Quad>::new)
                    .push(quad.clone());
            }
            // 2.1) For each blank node that is a component of Q, add a reference to Q from the map
            // entry for the blank node identifier identifier in the blank node to quads map,
            // creating a new entry if necessary.
            if let Object::BlankNode(n) = &quad.object {
                self.blank_node_to_quads_map
                    .entry(n.value().clone())
                    .or_insert_with(Vec::<Quad>::new)
                    .push(quad.clone());
            }
            // 2.1) For each blank node that is a component of Q, add a reference to Q from the map
            // entry for the blank node identifier identifier in the blank node to quads map,
            // creating a new entry if necessary.
            if let Graph::BlankNode(n) = &quad.graph {
                self.blank_node_to_quads_map
                    .entry(n.value().clone())
                    .or_insert_with(Vec::<Quad>::new)
                    .push(quad.clone());
            }
        }
    }

    fn get_quads_for_blank_node(&self, identifier: &String) -> Option<&Vec<Quad>> {
        self.blank_node_to_quads_map.get(identifier)
    }

    fn serialize_blank_node_to_quads_map(&self) -> BTreeMap<String, Vec<String>> {
        self.blank_node_to_quads_map
            .iter()
            .map(|(k, v)| (k.clone(), v.iter().map(|q| q.serialize()).collect()))
            .collect()
    }
}

/// **4.4 Blank Node Identifier Issuer State**
/// During the canonicalization algorithm, it is sometimes necessary to issue new identifiers to blank nodes. The Issue Identifier algorithm uses an identifier issuer to accomplish this task. The information an identifier issuer needs to keep track of is described below.
#[derive(PartialEq, Eq, Clone, Debug)]
struct IdentifierIssuer {
    /// **identifier prefix**
    ///   The identifier prefix is a string that is used at the
    ///   beginning of an blank node identifier. It should be initialized
    ///   to a string that is specified by the canonicalization algorithm.
    ///   When generating a new blank node identifier, the prefix is
    ///   concatenated with a identifier counter. For example, c14n is a
    ///   proper initial value for the identifier prefix that would produce
    ///   blank node identifiers like c14n1.
    identifier_prefix: String,

    /// **identifier counter**
    ///   A counter that is appended to the identifier prefix to create an
    ///   blank node identifier. It is initialized to 0.
    identifier_counter: usize,

    /// **issued identifiers map**
    ///   An ordered map that relates existing identifiers to issued
    ///   identifiers, to prevent issuance of more than one new identifier
    ///   per existing identifier, and to allow blank nodes to be
    ///   reassigned identifiers some time after issuance.
    issued_identifiers_map: IndexMap<String, String>,
}

impl IdentifierIssuer {
    fn new(identifier_prefix: &str) -> IdentifierIssuer {
        let issued_identifiers_map = IndexMap::<String, String>::new();
        IdentifierIssuer {
            identifier_prefix: identifier_prefix.to_string(),
            identifier_counter: 0,
            issued_identifiers_map,
        }
    }

    fn increment(&mut self) {
        self.identifier_counter += 1
    }

    fn get(&self, existing_identifier: &String) -> Option<String> {
        self.issued_identifiers_map
            .get(existing_identifier)
            .cloned()
    }

    /// **4.6 Issue Identifier Algorithm**
    ///   This algorithm issues a new blank node identifier for a given existing
    ///   blank node identifier. It also updates state information that tracks
    ///   the order in which new blank node identifiers were issued. The order
    ///   of issuance is important for canonically labeling blank nodes that are
    ///   isomorphic to others in the dataset.
    /// **4.6.2 Algorithm**
    ///   The algorithm takes an identifier issuer I and an existing identifier as
    ///   inputs. The output is a new issued identifier.
    fn issue(&mut self, existing_identifier: &String) -> String {
        // 1) If there is a map entry for existing identifier in issued identifiers
        // map of I, return it.
        if let Some(issued_identifier) = self.get(existing_identifier) {
            return issued_identifier;
        }

        // 2) Generate issued identifier by concatenating identifier prefix with
        // the string value of identifier counter.
        let issued_identifier = format!("{}{}", self.identifier_prefix, self.identifier_counter);

        // 3) Add an entry mapping existing identifier to issued identifier to
        // the issued identifiers map of I.
        self.issued_identifiers_map
            .insert(existing_identifier.clone(), issued_identifier.clone());

        // 4) Increment identifier counter.
        self.increment();

        // 5) Return issued identifier.
        issued_identifier
    }

    fn serialize_issued_identifiers_map(&self) -> String {
        format!(
            "{{{}}}",
            self.issued_identifiers_map
                .iter()
                .map(|(k, v)| format!("{}: {}", k, v))
                .join(", ")
        )
    }
}

/// **hash**
///   The lowercase, hexadecimal representation of a message digest.
/// **hash algorithm**
///   The hash algorithm used by URDNA2015, namely, SHA-256.
fn hash(data: impl AsRef<[u8]>) -> Result<String, CanonicalizationError> {
    const HASH_LEN: usize = 32;
    const HASH_BUF_LEN: usize = HASH_LEN * 2;

    let hash = Sha256::digest(data);
    let mut buf = [0u8; HASH_BUF_LEN];
    let hex_hash = encode_str(&hash, &mut buf);
    match hex_hash {
        Ok(h) => Ok(h.to_string()),
        Err(e) => Err(CanonicalizationError::Base16EncodingFailed(e)),
    }
}

impl Quad {
    fn canonicalize(&self, issuer: &IdentifierIssuer) -> Result<Quad, CanonicalizationError> {
        Ok(Quad::new(
            &self.subject.canonicalize(issuer)?,
            &self.predicate,
            &self.object.canonicalize(issuer)?,
            &self.graph.canonicalize(issuer)?,
        ))
    }
}

impl Subject {
    fn canonicalize(&self, issuer: &IdentifierIssuer) -> Result<Subject, CanonicalizationError> {
        match self {
            Self::BlankNode(blank_node) => match blank_node.canonicalize(issuer) {
                Ok(canonicalized_blank_node) => Ok(Self::BlankNode(canonicalized_blank_node)),
                Err(e) => Err(e),
            },
            _ => Ok(self.clone()),
        }
    }
}

impl Object {
    fn canonicalize(&self, issuer: &IdentifierIssuer) -> Result<Object, CanonicalizationError> {
        match self {
            Self::BlankNode(blank_node) => match blank_node.canonicalize(issuer) {
                Ok(canonicalized_blank_node) => Ok(Self::BlankNode(canonicalized_blank_node)),
                Err(e) => Err(e),
            },
            _ => Ok(self.clone()),
        }
    }
}

impl Graph {
    fn canonicalize(&self, issuer: &IdentifierIssuer) -> Result<Graph, CanonicalizationError> {
        match self {
            Self::BlankNode(blank_node) => match blank_node.canonicalize(issuer) {
                Ok(canonicalized_blank_node) => Ok(Self::BlankNode(canonicalized_blank_node)),
                Err(e) => Err(e),
            },
            _ => Ok(self.clone()),
        }
    }
}

impl BlankNode {
    fn canonicalize(&self, issuer: &IdentifierIssuer) -> Result<BlankNode, CanonicalizationError> {
        let canonical_identifier = issuer.get(&self.value());
        match canonical_identifier {
            Some(id) => Ok(BlankNode::new(Some(&id))),
            None => Err(CanonicalizationError::CanonicalIdentifierNotExist),
        }
    }
}

/// **4.5 Canonicalization Algorithm**
///   The canonicalization algorithm converts an input dataset into a normalized dataset.
///   This algorithm will assign deterministic identifiers to any blank nodes in the input dataset.
pub fn canonicalize(input_dataset: &[Quad]) -> Result<Vec<Quad>, CanonicalizationError> {
    // * log * //
    let _span_ca = debug_span!(
        "ca",
        message = "log point: Entering the canonicalization function (4.5.3)."
    )
    .entered();
    // * log * //

    // 1) Create the canonicalization state.
    let mut state = CanonicalizationState::new();

    // 2) For every quad Q in input dataset:
    // * log * //
    let span_ca_2 = debug_span!(
        "ca.2",
        message = "log point: Extract quads for each bnode (4.5.3 (2))."
    )
    .entered();
    // * log * //

    // 2.1) For each blank node that is a component of Q, add a reference to Q from the map
    // entry for the blank node identifier identifier in the blank node to quads map,
    // creating a new entry if necessary.
    state.update_blank_node_to_quads_map(input_dataset);

    // * log * //
    debug!("Bnode to quads:");
    for (bnode_id, quads) in state.serialize_blank_node_to_quads_map().iter() {
        debug!(indent = 1, "{}:", bnode_id);
        for quad in quads.iter() {
            debug!(indent = 2, "- {}", quad.trim_end());
        }
    }
    span_ca_2.exit();
    // * log * //

    // 3) For each key n in the blank node to quads map:
    // * log * //
    let span_ca_3 = debug_span!(
        "ca.3",
        message = "log point: Calculated first degree hashes (4.5.3 (3))."
    )
    .entered();
    debug!("with:");
    // * log * //

    for (n, _quads) in state.blank_node_to_quads_map.iter() {
        // * log * //
        debug!(indent = 1, "- identifier: {}", n);
        // * log * //

        // 3.1) Create a hash, h_f(n), for n according to the Hash First Degree Quads algorithm.
        // * log * //
        let span_ca_3_1 = debug_span!("", indent = 1).entered();
        // * log * //

        let hash = hash_first_degree_quads(&state, n).unwrap();

        // * log * //
        span_ca_3_1.exit();
        // * log * //

        // 3.2) Add h_f(n) and n to hash to blank nodes map, including repetitions, creating a new entry if necessary.
        state
            .hash_to_blank_node_map
            .entry(hash)
            .or_insert_with(Vec::<String>::new)
            .push(n.clone());
    }
    // * log * //
    span_ca_3.exit();
    // * log * //

    // 4) For each hash to identifier list map entry in hash to blank nodes map, code point ordered by hash:
    // TODO: check if `sort()` here is actually sorting in **Unicode code point order**
    // * log * //
    let span_ca_4 = debug_span!(
        "ca.4",
        message = "log point: Create canonical replacements for hashes mapping to a single node (4.5.3 (4))."
    )
    .entered();
    debug!("with:");
    // * log * //

    let mut new_hash_to_blank_node_map = state.hash_to_blank_node_map.clone();
    for (hash, identifier_list) in state.hash_to_blank_node_map.iter() {
        // 4.1) If identifier list has more than one entry, continue to the next mapping.
        if identifier_list.len() > 1 {
            continue;
        }
        let identifier = &identifier_list[0];

        // * log * //
        debug!(indent = 1, "- identifier: {}", identifier);
        debug!("hash: {}", hash);
        // * log * //

        // 4.2) Use the Issue Identifier algorithm, passing canonical issuer and the single blank node identifier,
        // identifier in identifier list to issue a canonical replacement identifier for identifier.
        let _canonical_identifier = state.canonical_issuer.issue(identifier);

        // * log * //
        debug!("canonical label: {}", _canonical_identifier);
        // * log * //

        // 4.3) Remove the map entry for hash from the hash to blank nodes map.
        new_hash_to_blank_node_map.remove(hash);
    }
    state.hash_to_blank_node_map = new_hash_to_blank_node_map;

    // * log * //
    span_ca_4.exit();
    // * log * //

    // 5) For each hash to identifier list map entry in hash to blank nodes map, code point ordered by hash:
    // * log * //
    let span_ca_5 = debug_span!(
        "ca.5",
        message = "log point: Calculate hashes for identifiers with shared hashes (4.5.3 (5))."
    )
    .entered();
    debug!("with:");
    // * log * //

    for (_hash, identifier_list) in state.hash_to_blank_node_map.iter() {
        // * log * //
        debug!(indent = 1, "- hash: {}", _hash);
        debug!(indent = 2, "identifier list: {:?}", identifier_list);
        // * log * //

        // 5.1) Create hash path list where each item will be a result of running the Hash N-Degree Quads algorithm.
        let mut hash_path_list = Vec::<HashNDegreeQuadsResult>::new();

        // 5.2) For each blank node identifier n in identifier list:
        // * log * //
        let span_ca_5_2 = debug_span!(
            "ca.5.2",
            message =
                "log point: Calculate hashes for identifiers with shared hashes (4.5.3 (5.2)).",
            indent = 2
        )
        .entered();
        debug!("with:");
        // * log * //

        for n in identifier_list {
            // * log * //
            debug!(indent = 1, "- identifier: {}", n);
            // * log * //

            // 5.2.1) If a canonical identifier has already been issued for n, continue to the next blank node
            // identifier.
            if state.canonical_issuer.get(n).is_some() {
                continue;
            }

            // 5.2.2) Create temporary issuer, an identifier issuer initialized with the prefix b.
            let mut temporary_issuer = IdentifierIssuer::new("b");

            // 5.2.3) Use the Issue Identifier algorithm, passing temporary issuer and n, to issue a new temporary
            // blank node identifier b_n to n.
            temporary_issuer.issue(n);

            // 5.2.4) Run the Hash N-Degree Quads algorithm, passing the canonicalization state, n for identifier,
            // and temporary issuer, appending the result to the hash path list.
            // * log * //
            let span_ca_5_2_4 = debug_span!("", indent = 1).entered();
            // * log * //

            let result = hash_n_degree_quads(&state, n.clone(), &temporary_issuer).unwrap();

            // * log * //
            span_ca_5_2_4.exit();
            // * log * //

            hash_path_list.push(result);
        }

        // * log * //
        span_ca_5_2.exit();
        // * log * //

        // 5.3) For each result in the hash path list, code point ordered by the hash in result:

        // * log * //
        let span_ca_5_3 = debug_span!(
            "ca.5.3",
            message = "log point: Canonical identifiers for temporary identifiers (4.5.3 (5.3)).",
            indent = 2
        )
        .entered();
        if !hash_path_list.is_empty() {
            debug!("with:");
        }
        // * log * //

        hash_path_list.sort();
        for result in hash_path_list.iter() {
            // * log * //
            debug!(indent = 1, "- result: {}", result.hash);
            debug!(
                indent = 2,
                "issuer: {}",
                result.issuer.serialize_issued_identifiers_map()
            );
            // * log * //

            // 5.3.1) For each blank node identifier, existing identifier, that was issued a temporary identifier
            // by identifier issuer in result, issue a canonical identifier, in the same order, using the Issue
            // Identifier algorithm, passing canonical issuer and existing identifier.

            // * log * //
            let span_ca_5_3_1 = debug_span!("ca.5.3.1", indent = 2).entered();
            // * log * //

            for (existing_identifier, _temporary_identifier) in
                result.issuer.issued_identifiers_map.iter()
            {
                // * log * //
                debug!("- existing identifier: {}", existing_identifier);
                // * log * //

                let _canonical_identifier = state.canonical_issuer.issue(existing_identifier);

                // * log * //
                debug!(indent = 1, "cid: {}", _canonical_identifier);
                // * log * //
            }

            // * log * //
            span_ca_5_3_1.exit();
            // * log * //
        }

        // * log * //
        span_ca_5_3.exit();
        // * log * //
    }

    // * log * //
    span_ca_5.exit();
    // * log * //

    // 6) For each quad, q, in input dataset:

    // * log * //
    let span_ca_6 = debug_span!(
        "ca.6",
        message = "log point: Replace original with canonical labels (4.5.3 (6))."
    )
    .entered();
    debug!(
        "canonical issuer: {}",
        state.canonical_issuer.serialize_issued_identifiers_map()
    );
    // * log * //

    // 6.1) Create a copy, quad copy, of q and replace any existing blank node identifier n using the
    // canonical identifiers previously issued by canonical issuer.
    // 6.2) Add quad copy to the normalized dataset.
    let normalized_dataset: Result<Vec<Quad>, CanonicalizationError> = input_dataset
        .iter()
        .map(|q| q.canonicalize(&state.canonical_issuer))
        .collect();

    // * log * //
    span_ca_6.exit();
    // * log * //

    // 7) Return the normalized dataset.
    normalized_dataset
}

/// **4.7 Hash First Degree Quads**
///   This algorithm calculates a hash for a given blank node across the
///   quads in a dataset in which that blank node is a component. If the
///   hash uniquely identifies that blank node, no further examination is
///   necessary. Otherwise, a hash will be created for the blank node using
///   the algorithm in 4.9 Hash N-Degree Quads invoked via
///   4.5 Canonicalization Algorithm.
/// **4.7.3 Algorithm**
///   This algorithm takes the canonicalization state and a reference blank node
///   identifier as inputs.
fn hash_first_degree_quads(
    canonicalization_state: &CanonicalizationState,
    reference_blank_node_identifier: &String,
) -> Result<String, CanonicalizationError> {
    let _span_h1dq = debug_span!(
        "h1dq",
        message = "log point: Hash First Degree Quads function (4.7.3)."
    )
    .entered();

    // 1) Initialize nquads to an empty list. It will be used to store
    // quads in canonical n-quads form.
    // let nquads: Vec<String> = Vec::new();

    // 2) Get the list of quads quads from the map entry for reference
    // blank node identifier in the blank node to quads map.
    let quads =
        match canonicalization_state.get_quads_for_blank_node(reference_blank_node_identifier) {
            Some(q) => q,
            None => return Err(CanonicalizationError::QuadsNotExist),
        };

    // 3) For each quad quad in quads:
    let mut nquads = quads
        .iter()
        .map(|quad| {
            // 3.1) Serialize the quad in canonical n-quads form with the following special rule:
            // 3.1.1) If any component in quad is an blank node, then serialize it using a special
            // identifier as follows:
            let subject = match &quad.subject {
                Subject::BlankNode(bnode) => {
                    Subject::BlankNode(replace_bnid(bnode, reference_blank_node_identifier))
                }
                s => s.clone(),
            };
            // 3.1.1) If any component in quad is an blank node, then serialize it using a special
            // identifier as follows:
            let object = match &quad.object {
                Object::BlankNode(bnode) => {
                    Object::BlankNode(replace_bnid(bnode, reference_blank_node_identifier))
                }
                s => s.clone(),
            };
            // 3.1.1) If any component in quad is an blank node, then serialize it using a special
            // identifier as follows:
            let graph = match &quad.graph {
                Graph::BlankNode(bnode) => {
                    Graph::BlankNode(replace_bnid(bnode, reference_blank_node_identifier))
                }
                s => s.clone(),
            };
            let predicate = quad.predicate.clone();

            Quad::new(&subject, &predicate, &object, &graph).serialize()
        })
        .collect::<Vec<String>>();

    // 3.1.1.1) If the blank node's existing blank node identifier matches the reference
    // blank node identifier then use the blank node identifier a, otherwise, use the blank
    // node identifier z.
    fn replace_bnid(bnode: &BlankNode, reference_blank_node_identifier: &String) -> BlankNode {
        if bnode.value() == *reference_blank_node_identifier {
            BlankNode::new(Some("a"))
        } else {
            BlankNode::new(Some("z"))
        }
    }

    // * log * //
    debug!("nquads:");
    for nquad in nquads.iter() {
        debug!(indent = 1, "- {}", nquad.trim_end());
    }
    // * log * //

    // 4) Sort nquads in Unicode code point order.
    // TODO: check if `sort()` here is actually sorting in **Unicode code point order**
    nquads.sort();

    // 5) Return the hash that results from passing the sorted and concatenated
    // nquads through the hash algorithm.
    let hashed_nquads = hash(nquads.join(""));
    debug!("hash: {}", hashed_nquads.clone().unwrap_or_default());
    hashed_nquads
}

enum HashRelatedBlankNodePosition {
    Subject,
    Object,
    Graph,
}
impl HashRelatedBlankNodePosition {
    fn serialize(&self) -> &str {
        match self {
            Self::Subject => "s",
            Self::Object => "o",
            Self::Graph => "g",
        }
    }
}

/// **4.8 Hash Related Blank Node**
///   This algorithm generates a hash for some blank node component of a quad, considering
///   its position within that quad. This is used as part of the Hash N-Degree Quads
///   algorithm to characterize the blank nodes related to some particular blank node within
///   their mention sets.
fn hash_related_blank_node(
    state: &CanonicalizationState,
    related: &String,
    quad: &Quad,
    issuer: &IdentifierIssuer,
    position: HashRelatedBlankNodePosition,
) -> Result<String, CanonicalizationError> {
    // * log * //
    debug!("- position: {}", position.serialize());
    debug!(indent = 1, "related: {}", related);
    // * log * //

    // 1) Initialize a string input to the value of position.
    let input = match position {
        HashRelatedBlankNodePosition::Graph => position.serialize().to_string(),
        // 2) If position is not g, append <, the value of the predicate in quad, and > to input.
        _ => format!("{}<{}>", position.serialize(), quad.predicate.value()),
    };

    // 3) If there is a canonical identifier for related, or an identifier issued by issuer,
    // append the string _:, followed by that identifier (using the canonical identifier if
    // present, otherwise the one issued by issuer) to input.

    // * log * //
    let span_hrbn_3 = debug_span!("").entered();
    // * log * //

    let identifier = match state.canonical_issuer.get(related) {
        Some(id) => format!("_:{}", id),
        None => match issuer.get(related) {
            Some(id) => format!("_:{}", id),
            // 4) Otherwise, append the result of the Hash First Degree Quads algorithm,
            // passing related to input.
            None => hash_first_degree_quads(state, related)?,
        },
    };

    // * log * //
    span_hrbn_3.exit();
    // * log * //

    let input = format!("{}{}", input, identifier);

    // * log * //
    debug!(indent = 1, "input: \"{}\"", input);
    // * log * //

    // 5) Return the hash that results from passing input through the hash algorithm.
    let output = hash(input);

    // * log * //
    debug!(indent = 1, "hash: {}", output.clone().unwrap_or_default());
    // * log * //

    output
}

#[derive(PartialEq, Eq, Debug)]
struct HashNDegreeQuadsResult {
    hash: String,
    issuer: IdentifierIssuer,
}

impl PartialOrd for HashNDegreeQuadsResult {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.hash.partial_cmp(&other.hash)
    }
}

impl Ord for HashNDegreeQuadsResult {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.hash.cmp(&other.hash)
    }
}

/// **4.9 Hash N-Degree Quads**
///   This algorithm calculates a hash for a given blank node across the quads in a dataset
///   in which that blank node is a component for which the hash does not uniquely identify
///   that blank node. This is done by expanding the search from quads directly referencing
///   that blank node (the mention set), to those quads which contain nodes which are also
///   components of quads in the mention set, called the gossip path. This process proceeds
///   in every greater degrees of indirection until a unique hash is obtained.
/// **4.9.3 Algorithm**
///   The inputs to this algorithm are the canonicalization state, the identifier for the
///   blank node to recursively hash quads for, and path identifier issuer which is an
///   identifier issuer that issues temporary blank node identifiers. The output from this
///   algorithm will be a hash and the identifier issuer used to help generate it.
fn hash_n_degree_quads(
    state: &CanonicalizationState,
    identifier: String,
    path_identifier_issuer: &IdentifierIssuer,
) -> Result<HashNDegreeQuadsResult, CanonicalizationError> {
    // * log * //
    let _span_hndq = debug_span!(
        "hndq",
        message = "log point: Hash N-Degree Quads function (4.9.3)."
    )
    .entered();
    // * log * //

    debug!("identifier: {}", identifier);
    debug!(
        "issuer: {}",
        path_identifier_issuer.serialize_issued_identifiers_map()
    );

    let mut issuer = path_identifier_issuer.clone();

    // 1) Create a new map Hn for relating hashes to related blank nodes.
    let mut h_n = BTreeMap::<String, Vec<String>>::new();

    // 2) Get a reference, quads, to the list of quads from the map entry for identifier
    // in the blank node to quads map.
    // * log * //
    let span_hndq_2 = debug_span!(
        "hndq.2",
        message = "log point: Quads for identifier (4.9.3 (2))."
    )
    .entered();
    // * log * //

    let quads = match state.get_quads_for_blank_node(&identifier) {
        Some(q) => q,
        None => return Err(CanonicalizationError::QuadsNotExist),
    };
    debug!("quads:");
    for quad in quads {
        debug!(indent = 1, "- {}", quad.serialize().trim_end());
    }

    // * log * //
    span_hndq_2.exit();
    // * log * //

    // 3) For each quad in quads:
    // * log * //
    let span_hndq_3 = debug_span!(
        "hndq.3",
        message = "log point: Hash N-Degree Quads function (4.9.3 (3))."
    )
    .entered();
    debug!("with:");
    // * log * //

    for quad in quads {
        // * log * //
        debug!(indent = 1, "- quad: {}", quad.serialize().trim_end());
        let span_hndq_3_1 = debug_span!(
            "hndq.3.1",
            message = "log point: Hash related bnode component (4.9.3 (3.1)).",
            indent = 2
        )
        .entered();
        let mut span_hndq_3_1_flag = false;
        // * log * //

        // 3.1) For each component in quad, where component is the subject, object, or graph name,
        // and it is a blank node that is not identified by identifier:
        if let Subject::BlankNode(bnode) = &quad.subject {
            let bnode_id = bnode.value();
            if bnode_id != identifier {
                // 3.1.1) Set hash to the result of the Hash Related Blank Node algorithm, passing
                // the blank node identifier for component as related, quad, issuer, and position
                // as either s, o, or g based on whether component is a subject, object, graph name,
                // respectively.

                // * log * //
                if !span_hndq_3_1_flag {
                    debug!("with:");
                    span_hndq_3_1_flag = true;
                }
                // * log * //

                let hash = hash_related_blank_node(
                    state,
                    &bnode_id,
                    quad,
                    &issuer,
                    HashRelatedBlankNodePosition::Subject,
                )?;

                // 3.1.2) Add a mapping of hash to the blank node identifier for component to Hn,
                // adding an entry as necessary.
                h_n.entry(hash)
                    .or_insert_with(Vec::<String>::new)
                    .push(bnode_id);
            };
        };
        // 3.1) For each component in quad, where component is the subject, object, or graph name,
        // and it is a blank node that is not identified by identifier:
        if let Object::BlankNode(bnode) = &quad.object {
            let bnode_id = bnode.value();
            if bnode_id != identifier {
                // 3.1.1) Set hash to the result of the Hash Related Blank Node algorithm, passing
                // the blank node identifier for component as related, quad, issuer, and position
                // as either s, o, or g based on whether component is a subject, object, graph name,
                // respectively.

                // * log * //
                if !span_hndq_3_1_flag {
                    debug!("with:");
                    span_hndq_3_1_flag = true;
                }
                // * log * //

                let hash = hash_related_blank_node(
                    state,
                    &bnode_id,
                    quad,
                    &issuer,
                    HashRelatedBlankNodePosition::Object,
                )?;

                // 3.1.2) Add a mapping of hash to the blank node identifier for component to Hn,
                // adding an entry as necessary.
                h_n.entry(hash)
                    .or_insert_with(Vec::<String>::new)
                    .push(bnode_id);
            };
        };
        // 3.1) For each component in quad, where component is the subject, object, or graph name,
        // and it is a blank node that is not identified by identifier:
        if let Graph::BlankNode(bnode) = &quad.graph {
            let bnode_id = bnode.value();
            if bnode_id != identifier {
                // 3.1.1) Set hash to the result of the Hash Related Blank Node algorithm, passing
                // the blank node identifier for component as related, quad, issuer, and position
                // as either s, o, or g based on whether component is a subject, object, graph name,
                // respectively.

                // * log * //
                if !span_hndq_3_1_flag {
                    debug!("with:");
                }
                // * log * //

                let hash = hash_related_blank_node(
                    state,
                    &bnode_id,
                    quad,
                    &issuer,
                    HashRelatedBlankNodePosition::Graph,
                )?;

                // 3.1.2) Add a mapping of hash to the blank node identifier for component to Hn,
                // adding an entry as necessary.
                h_n.entry(hash)
                    .or_insert_with(Vec::<String>::new)
                    .push(bnode_id);
            };
        };

        // * log * //
        span_hndq_3_1.exit();
        // * log * //
    }

    // * log * //
    debug!("Hash to bnodes:");
    for (hash, bnodes) in h_n.iter() {
        debug!(indent = 1, "{}:", hash);
        for bnode in bnodes.iter() {
            debug!(indent = 2, "- {}", bnode);
        }
    }
    span_hndq_3.exit();
    // * log * //

    // 4) Create an empty string, data to hash.
    let mut data_to_hash = Vec::<String>::new();

    // 5) For each related hash to blank node list mapping in Hn, code point ordered by related hash:
    // TODO: check if keys in BTreeMap is actually sorted in **code point order**

    // * log * //
    let span_hndq_5 = debug_span!(
        "hndq.5",
        message = "log point: Hash N-Degree Quads function (4.9.3 (5)), entering loop."
    )
    .entered();
    debug!("with:");
    // * log * //

    for (related_hash, blank_node_list) in h_n {
        // * log * //
        debug!(indent = 1, "- related hash: {}", related_hash);
        debug!(indent = 2, "data to hash: \"{}\"", data_to_hash.join(""));
        // * log * //

        // 5.1) Append the related hash to the data to hash.
        data_to_hash.push(related_hash);

        // 5.2) Create a string chosen path.
        let mut chosen_path = String::new();

        // 5.3) Create an unset chosen issuer variable.
        let mut chosen_issuer = IdentifierIssuer::new("UNSET");

        // 5.4) For each permutation p of blank node list:

        // * log * //
        let span_hndq_5_4 = debug_span!(
            "hndq.5.4",
            message = "log point: Hash N-Degree Quads function (4.9.3 (5.4)), entering loop.",
            indent = 2
        )
        .entered();
        // * log * //

        'perm_loop: for p in blank_node_list.iter().permutations(blank_node_list.len()) {
            // * log * //
            debug!("with:");
            debug!(indent = 1, "- perm: {:?}", p);
            // * log * //

            // 5.4.1) Create a copy of issuer, issuer copy.
            let mut issuer_copy = issuer.clone();

            // 5.4.2) Create a string path.
            let mut path_vec = Vec::<String>::new();

            // 5.4.3) Create a recursion list, to store blank node identifiers that must be
            // recursively processed by this algorithm.
            let mut recursion_list = Vec::<&String>::new();

            // 5.4.4) For each related in p:

            // * log * //
            let span_hndq_5_4_4 = debug_span!(
                "hndq.5.4.4",
                message = "log point: Hash N-Degree Quads function (4.9.3 (5.4.4)), entering loop.",
                indent = 2
            )
            .entered();
            debug!("with:");
            // * log * //

            for related in p {
                // * log * //
                debug!(indent = 1, "- related: {}", related);
                // * log * //

                if let Some(canonical_identifier) = state.canonical_issuer.get(related) {
                    // 5.4.4.1) If a canonical identifier has been issued for related by
                    // canonical issuer, append the string _:, followed by the canonical
                    // identifier for related, to path.
                    path_vec.push(format!("_:{}", canonical_identifier));
                } else {
                    // 5.4.4.2) Otherwise:
                    // 5.4.4.2.1) If issuer copy has not issued an identifier for
                    // related, append related to recursion list.
                    if issuer_copy.get(related).is_none() {
                        recursion_list.push(related);
                    }
                    // 5.4.4.2.2) Use the Issue Identifier algorithm, passing issuer
                    // copy and related, and append the string _:, followed by the result,
                    // to path.
                    path_vec.push(format!("_:{}", issuer_copy.issue(related)));
                }

                // 5.4.4.3) If chosen path is not empty and the length of path is greater
                // than or equal to the length of chosen path and path is greater than
                // chosen path when considering code point order, then skip to the next
                // permutation p.
                let path = path_vec.join("");

                // * log * //
                debug!(indent = 2, "path: \"{}\"", path);
                // * log * //

                if !chosen_path.is_empty() && path.len() >= chosen_path.len() && path >= chosen_path
                {
                    continue 'perm_loop;
                }
            }

            // * log * //
            span_hndq_5_4_4.exit();
            // * log * //

            // 5.4.5) For each related in recursion list:

            // * log * //
            let span_hndq_5_4_5 = debug_span!(
                "hndq.5.4.5",
                message = "log point: Hash N-Degree Quads function (4.9.3 (5.4.5)), before possible recursion.",
                indent = 2
            )
            .entered();
            debug!("recursion list: {:?}", recursion_list);
            debug!("path: {:?}", chosen_path);
            if !recursion_list.is_empty() {
                debug!("with:");
            }
            // * log * //

            for related in recursion_list {
                // * log * //
                debug!(indent = 1, "- related: {}", related);
                // * log * //

                // 5.4.5.1) Set result to the result of recursively executing the Hash
                // N-Degree Quads algorithm, passing the canonicalization state, related
                // for identifier, and issuer copy for path identifier issuer.

                // * log * //
                let span_hndq_5_4_5_1 = debug_span!("", indent = 1).entered();
                // * log * //

                let result = hash_n_degree_quads(state, related.clone(), &issuer_copy)?;

                // * log * //
                span_hndq_5_4_5_1.exit();
                // * log * //

                // 5.4.5.2) Use the Issue Identifier algorithm, passing issuer copy and
                // related; append the string _:, followed by the result, to path.
                path_vec.push(format!("_:{}", issuer_copy.issue(related)));

                // 5.4.5.3) Append <, the hash in result, and > to path.
                path_vec.push("<".to_string());
                path_vec.push(result.hash);
                path_vec.push(">".to_string());

                // 5.4.5.4) Set issuer copy to the identifier issuer in result.

                // * log * //
                let span_hndq_5_4_5_4 = debug_span!(
                    "hndq.5.4.5.4",
                    message = "log point: Hash N-Degree Quads function (4.9.3 (5.4.5.4)), combine result of recursion.",
                    indent = 2
                ).entered();
                // * log * //

                issuer_copy = result.issuer;
                let path = path_vec.join("");

                // * log * //
                debug!("path: \"{}\"", path);
                debug!(
                    "issuer copy: {}",
                    issuer_copy.serialize_issued_identifiers_map()
                );
                span_hndq_5_4_5_4.exit();
                // * log * //

                // 5.4.5.5) If chosen path is not empty and the length of path is greater
                // than or equal to the length of chosen path and path is greater than
                // chosen path when considering code point order, then skip to the next p.
                if !chosen_path.is_empty() && path.len() >= chosen_path.len() && path >= chosen_path
                {
                    continue 'perm_loop;
                }
            }

            // * log * //
            span_hndq_5_4_5.exit();
            // * log * //

            // 5.4.6) If chosen path is empty or path is less than chosen path when
            // considering code point order, set chosen path to path and chosen issuer to
            // issuer copy.
            let path = path_vec.join("");
            if chosen_path.is_empty() || path < chosen_path {
                chosen_path = path;
                chosen_issuer = issuer_copy;
            }
        }

        // * log * //
        span_hndq_5_4.exit();
        // * log * //

        // 5.5) Append chosen path to data to hash.

        // * log * //
        let span_hndq_5_5 = debug_span!(
            "hndq.5.5",
            message = "log point: Hash N-Degree Quads function (4.9.3 (5.5). End of current loop with Hn hashes.",
            indent = 2
        )
        .entered();
        debug!("chosen path: \"{}\"", chosen_path);
        // * log * //

        data_to_hash.push(chosen_path);

        // * log * //
        debug!("data to hash: \"{}\"", data_to_hash.join(""));
        span_hndq_5_5.exit();
        // * log * //

        // 5.6) Replace issuer, by reference, with chosen issuer.
        issuer = chosen_issuer;
    }

    // * log * //
    span_hndq_5.exit();
    // * log * //

    // 6) Return issuer and the hash that results from passing data to hash through the
    // hash algorithm.

    // * log * //
    let span_hndq_6 = debug_span!(
        "hndq.6",
        message = "log point: Leaving Hash N-Degree Quads function (4.9.3 (6))."
    )
    .entered();
    // * log * //

    let hash = hash(data_to_hash.join(""))?;

    // * log * //
    debug!("hash: {}", hash);
    debug!("issuer: {}", issuer.serialize_issued_identifiers_map());
    span_hndq_6.exit();
    // * log * //

    Ok(HashNDegreeQuadsResult { hash, issuer })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rdf::{DefaultGraph, NamedNode, Predicate};

    #[test]
    fn test_issue_identifier() {
        let mut canonical_issuer = IdentifierIssuer::new("c14n");
        assert_eq!(
            canonical_issuer.issue(&"b0".to_string()),
            "c14n0".to_string()
        );
        assert_eq!(
            canonical_issuer.issue(&"b1".to_string()),
            "c14n1".to_string()
        );
        assert_eq!(
            canonical_issuer.issue(&"b99".to_string()),
            "c14n2".to_string()
        );
        assert_eq!(
            canonical_issuer.issue(&"xyz".to_string()),
            "c14n3".to_string()
        );
        assert_eq!(
            canonical_issuer.issue(&"xyz".to_string()),
            "c14n3".to_string()
        );
        assert_eq!(
            canonical_issuer.issue(&"b99".to_string()),
            "c14n2".to_string()
        );
        assert_eq!(
            canonical_issuer.issue(&"b1".to_string()),
            "c14n1".to_string()
        );
        assert_eq!(
            canonical_issuer.issue(&"b0".to_string()),
            "c14n0".to_string()
        );
    }

    #[test]
    fn test_hash_first_degree_quads_unique_hashes() {
        let mut state = CanonicalizationState::new();

        let e0 = BlankNode::new(None);
        let e1 = BlankNode::new(None);
        let p = NamedNode::new("http://example.com/#p");
        let q = NamedNode::new("http://example.com/#q");
        let r = NamedNode::new("http://example.com/#r");
        let s = NamedNode::new("http://example.com/#s");
        let t = NamedNode::new("http://example.com/#t");
        let u = NamedNode::new("http://example.com/#u");
        let default_graph = DefaultGraph::new();
        let input_dataset = vec![
            Quad::new(
                &Subject::NamedNode(p.clone()),
                &Predicate::NamedNode(q),
                &Object::BlankNode(e0.clone()),
                &Graph::DefaultGraph(default_graph.clone()),
            ),
            Quad::new(
                &Subject::NamedNode(p),
                &Predicate::NamedNode(r),
                &Object::BlankNode(e1.clone()),
                &Graph::DefaultGraph(default_graph.clone()),
            ),
            Quad::new(
                &Subject::BlankNode(e0.clone()),
                &Predicate::NamedNode(s),
                &Object::NamedNode(u.clone()),
                &Graph::DefaultGraph(default_graph.clone()),
            ),
            Quad::new(
                &Subject::BlankNode(e1.clone()),
                &Predicate::NamedNode(t),
                &Object::NamedNode(u),
                &Graph::DefaultGraph(default_graph),
            ),
        ];

        state.update_blank_node_to_quads_map(&input_dataset);

        let hash_e0 = hash_first_degree_quads(&state, &e0.value());
        assert_eq!(
            hash_e0.unwrap(),
            "21d1dd5ba21f3dee9d76c0c00c260fa6f5d5d65315099e553026f4828d0dc77a".to_string()
        );
        let hash_e1 = hash_first_degree_quads(&state, &e1.value());
        assert_eq!(
            hash_e1.unwrap(),
            "6fa0b9bdb376852b5743ff39ca4cbf7ea14d34966b2828478fbf222e7c764473".to_string()
        );
    }

    #[test]
    fn test_hash_first_degree_quads_shared_hashes() {
        let mut state = CanonicalizationState::new();

        let e0 = BlankNode::new(None);
        let e1 = BlankNode::new(None);
        let e2 = BlankNode::new(None);
        let e3 = BlankNode::new(None);
        let p = NamedNode::new("http://example.com/#p");
        let q = NamedNode::new("http://example.com/#q");
        let r = NamedNode::new("http://example.com/#r");
        let default_graph = DefaultGraph::new();
        let input_dataset = vec![
            Quad::new(
                &Subject::NamedNode(p.clone()),
                &Predicate::NamedNode(q.clone()),
                &Object::BlankNode(e0.clone()),
                &Graph::DefaultGraph(default_graph.clone()),
            ),
            Quad::new(
                &Subject::NamedNode(p.clone()),
                &Predicate::NamedNode(q),
                &Object::BlankNode(e1.clone()),
                &Graph::DefaultGraph(default_graph.clone()),
            ),
            Quad::new(
                &Subject::BlankNode(e0.clone()),
                &Predicate::NamedNode(p.clone()),
                &Object::BlankNode(e2.clone()),
                &Graph::DefaultGraph(default_graph.clone()),
            ),
            Quad::new(
                &Subject::BlankNode(e1.clone()),
                &Predicate::NamedNode(p),
                &Object::BlankNode(e3.clone()),
                &Graph::DefaultGraph(default_graph.clone()),
            ),
            Quad::new(
                &Subject::BlankNode(e2.clone()),
                &Predicate::NamedNode(r),
                &Object::BlankNode(e3.clone()),
                &Graph::DefaultGraph(default_graph),
            ),
        ];

        state.update_blank_node_to_quads_map(&input_dataset);

        let hash_e0 = hash_first_degree_quads(&state, &e0.value());
        assert_eq!(
            hash_e0.unwrap(),
            "3b26142829b8887d011d779079a243bd61ab53c3990d550320a17b59ade6ba36".to_string()
        );
        let hash_e1 = hash_first_degree_quads(&state, &e1.value());
        assert_eq!(
            hash_e1.unwrap(),
            "3b26142829b8887d011d779079a243bd61ab53c3990d550320a17b59ade6ba36".to_string()
        );
        let hash_e2 = hash_first_degree_quads(&state, &e2.value());
        assert_eq!(
            hash_e2.unwrap(),
            "15973d39de079913dac841ac4fa8c4781c0febfba5e83e5c6e250869587f8659".to_string()
        );
        let hash_e3 = hash_first_degree_quads(&state, &e3.value());
        assert_eq!(
            hash_e3.unwrap(),
            "7e790a99273eed1dc57e43205d37ce232252c85b26ca4a6ff74ff3b5aea7bccd".to_string()
        );
    }

    #[test]
    fn test_hash_related_blank_node() {
        let mut state = CanonicalizationState::new();
        state
            .canonical_issuer
            .issued_identifiers_map
            .insert("e2".to_string(), "c14n0".to_string());
        let issuer = IdentifierIssuer::new("b");
        let position = HashRelatedBlankNodePosition::Object;
        let e0 = BlankNode::new(None);
        let e2 = BlankNode::new(None);
        let p = NamedNode::new("http://example.com/#p");
        let default_graph = DefaultGraph::new();
        let quad = Quad::new(
            &Subject::BlankNode(e0),
            &Predicate::NamedNode(p),
            &Object::BlankNode(e2),
            &Graph::DefaultGraph(default_graph),
        );
        let related_hash =
            hash_related_blank_node(&state, &"e2".to_string(), &quad, &issuer, position);
        assert_eq!(
            related_hash.unwrap(),
            "29cf7e22790bc2ed395b81b3933e5329fc7b25390486085cac31ce7252ca60fa".to_string()
        );
    }

    #[test]
    fn test_hash_n_degree_quads() {
        let mut state = CanonicalizationState::new();

        let e0 = BlankNode::new(None);
        let e1 = BlankNode::new(None);
        let e2 = BlankNode::new(None);
        let e3 = BlankNode::new(None);
        let p = NamedNode::new("http://example.com/#p");
        let q = NamedNode::new("http://example.com/#q");
        let r = NamedNode::new("http://example.com/#r");
        let default_graph = DefaultGraph::new();
        let input_dataset = vec![
            Quad::new(
                &Subject::NamedNode(p.clone()),
                &Predicate::NamedNode(q.clone()),
                &Object::BlankNode(e0.clone()),
                &Graph::DefaultGraph(default_graph.clone()),
            ),
            Quad::new(
                &Subject::NamedNode(p.clone()),
                &Predicate::NamedNode(q),
                &Object::BlankNode(e1.clone()),
                &Graph::DefaultGraph(default_graph.clone()),
            ),
            Quad::new(
                &Subject::BlankNode(e0.clone()),
                &Predicate::NamedNode(p.clone()),
                &Object::BlankNode(e2.clone()),
                &Graph::DefaultGraph(default_graph.clone()),
            ),
            Quad::new(
                &Subject::BlankNode(e1.clone()),
                &Predicate::NamedNode(p),
                &Object::BlankNode(e3.clone()),
                &Graph::DefaultGraph(default_graph.clone()),
            ),
            Quad::new(
                &Subject::BlankNode(e2.clone()),
                &Predicate::NamedNode(r),
                &Object::BlankNode(e3.clone()),
                &Graph::DefaultGraph(default_graph),
            ),
        ];

        state.update_blank_node_to_quads_map(&input_dataset);

        for (n, _quads) in state.blank_node_to_quads_map.iter() {
            let hash = hash_first_degree_quads(&state, n).unwrap();
            state
                .hash_to_blank_node_map
                .entry(hash)
                .or_insert_with(Vec::<String>::new)
                .push(n.clone());
        }

        let mut new_hash_to_blank_node_map = state.hash_to_blank_node_map.clone();
        for (hash, identifier_list) in state.hash_to_blank_node_map.iter() {
            if identifier_list.len() > 1 {
                continue;
            }
            let identifier = &identifier_list[0];
            state.canonical_issuer.issue(identifier);
            new_hash_to_blank_node_map.remove(hash);
        }
        state.hash_to_blank_node_map = new_hash_to_blank_node_map;

        for (_hash, identifier_list) in state.hash_to_blank_node_map.iter() {
            let mut hash_path_list = Vec::<HashNDegreeQuadsResult>::new();
            for n in identifier_list {
                if state.canonical_issuer.get(n).is_some() {
                    continue;
                }
                let mut temporary_issuer = IdentifierIssuer::new("b");
                temporary_issuer.issue(n);
                let result = hash_n_degree_quads(&state, n.clone(), &temporary_issuer).unwrap();
                hash_path_list.push(result);
            }
            hash_path_list.sort();
            assert_eq!(
                hash_path_list[0].hash,
                "2c0b377baf86f6c18fed4b0df6741290066e73c932861749b172d1e5560f5045"
            );
            assert_eq!(
                hash_path_list[1].hash,
                "fbc300de5afafd97a4b9ee1e72b57754dcdcb7ebb724789ac6a94a5b82a48d30"
            );
        }
    }
}
