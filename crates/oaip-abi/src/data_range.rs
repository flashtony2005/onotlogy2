//! Parser for composable ODL property-range expressions.

use crate::ir::{PropertyDataRange, PropertyFacet};
use crate::validation::{property_one_of_json_values, validate_property_constraints};

const MAX_DATA_RANGE_DEPTH: usize = 8;
const MAX_DATA_RANGE_NODES: usize = 64;

#[derive(Clone, Debug)]
enum Token {
    Word(String),
    Text(String),
    Punct(char),
}

#[derive(Debug)]
struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
    nodes: usize,
    depth: usize,
}

/// Parse a `union`, `intersection`, `complement`, `restriction`, `one_of`, or
/// base `datatype` expression.  Parsing is structural only; typed facet
/// validation is performed by [`validate_data_range`] so the same checks cover
/// parsed ODL and governance-supplied runtime IR.
pub fn parse_data_range(source: &str) -> Result<PropertyDataRange, String> {
    let mut parser = Parser::new(source)?;
    let range = parser.expression()?;
    if parser.cursor != parser.tokens.len() {
        return Err("unexpected input after range expression".to_owned());
    }
    Ok(range)
}

/// Validate structure, datatype compatibility, and conservative resource bounds.
pub fn validate_data_range(
    name: &str,
    ty: crate::ir::ScalarType,
    range: &PropertyDataRange,
) -> Result<(), String> {
    shape(range)?;
    match range {
        PropertyDataRange::Datatype => Ok(()),
        PropertyDataRange::OneOf(values) => {
            let normalized =
                property_one_of_json_values(ty, &[PropertyFacet::OneOf(values.clone())])?;
            if normalized.is_empty() {
                return Err("enumeration requires at least one value".to_owned());
            }
            Ok(())
        }
        PropertyDataRange::Restriction(facets) => {
            if facets.iter().any(|facet| {
                matches!(
                    facet,
                    PropertyFacet::MinCardinality(_)
                        | PropertyFacet::MaxCardinality(_)
                        | PropertyFacet::OneOf(_)
                        | PropertyFacet::Range(_)
                )
            }) {
                return Err("restriction accepts only datatype facets, not presence, enumeration, or nested range constructs".to_owned());
            }
            validate_property_constraints(name, ty, facets)
        }
        PropertyDataRange::Complement(child) => validate_data_range(name, ty, child),
        PropertyDataRange::Union(children) | PropertyDataRange::Intersection(children) => {
            for child in children {
                validate_data_range(name, ty, child)?;
            }
            Ok(())
        }
    }
}

fn shape(range: &PropertyDataRange) -> Result<(), String> {
    fn visit(range: &PropertyDataRange, depth: usize, nodes: &mut usize) -> Result<(), String> {
        if depth > MAX_DATA_RANGE_DEPTH {
            return Err(format!(
                "data range exceeds nesting depth {MAX_DATA_RANGE_DEPTH}"
            ));
        }
        *nodes += 1;
        if *nodes > MAX_DATA_RANGE_NODES {
            return Err(format!("data range exceeds {MAX_DATA_RANGE_NODES} nodes"));
        }
        if let PropertyDataRange::Union(children) | PropertyDataRange::Intersection(children) =
            range
        {
            if children.len() < 2 {
                return Err(format!("`{}` requires at least two branches", range.name()));
            }
            for child in children {
                visit(child, depth + 1, nodes)?;
            }
        }
        if let PropertyDataRange::Complement(child) = range {
            visit(child, depth + 1, nodes)?;
        }
        Ok(())
    }

    let mut nodes = 0;
    visit(range, 1, &mut nodes)
}

impl Parser {
    fn new(source: &str) -> Result<Self, String> {
        Ok(Self {
            tokens: tokenize(source)?,
            cursor: 0,
            nodes: 0,
            depth: 0,
        })
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.cursor)
    }

    fn next(&mut self) -> Result<Token, String> {
        let token = self
            .tokens
            .get(self.cursor)
            .cloned()
            .ok_or_else(|| "unexpected end of range expression".to_owned())?;
        self.cursor += 1;
        Ok(token)
    }

    fn eat_punct(&mut self, expected: char) -> Result<bool, String> {
        if matches!(self.peek(), Some(Token::Punct(ch)) if *ch == expected) {
            self.cursor += 1;
            return Ok(true);
        }
        Ok(false)
    }

    fn expect_punct(&mut self, expected: char) -> Result<(), String> {
        if !self.eat_punct(expected)? {
            return Err(format!("expected `{expected}` in range expression"));
        }
        Ok(())
    }

    fn expression(&mut self) -> Result<PropertyDataRange, String> {
        self.depth += 1;
        if self.depth > MAX_DATA_RANGE_DEPTH {
            return Err(format!(
                "data range exceeds nesting depth {MAX_DATA_RANGE_DEPTH}"
            ));
        }
        self.nodes += 1;
        if self.nodes > MAX_DATA_RANGE_NODES {
            return Err(format!("data range exceeds {MAX_DATA_RANGE_NODES} nodes"));
        }

        let Token::Word(word) = self.next()? else {
            return Err("expected a range combinator".to_owned());
        };
        let range = match word.as_str() {
            "datatype" => {
                if self.eat_punct('(')? {
                    self.expect_punct(')')?;
                }
                PropertyDataRange::Datatype
            }
            "one_of" => PropertyDataRange::OneOf(self.values()?),
            "restriction" => PropertyDataRange::Restriction(self.restriction_facets()?),
            "complement" => {
                self.expect_punct('(')?;
                let child = self.expression()?;
                self.expect_punct(')')?;
                PropertyDataRange::Complement(Box::new(child))
            }
            "union" | "intersection" => {
                self.expect_punct('(')?;
                let first = self.expression()?;
                let mut children = vec![first];
                while self.eat_punct(',')? {
                    children.push(self.expression()?);
                }
                self.expect_punct(')')?;
                if children.len() < 2 {
                    return Err(format!("`{}` requires at least two branches", word));
                }
                if word == "union" {
                    PropertyDataRange::Union(children)
                } else {
                    PropertyDataRange::Intersection(children)
                }
            }
            other => return Err(format!("unknown range combinator `{other}`")),
        };
        self.depth -= 1;
        Ok(range)
    }

    fn values(&mut self) -> Result<Vec<String>, String> {
        self.expect_punct('[')?;
        let mut values = vec![self.value()?];
        while self.eat_punct(',')? {
            values.push(self.value()?);
        }
        self.expect_punct(']')?;
        Ok(values)
    }

    fn value(&mut self) -> Result<String, String> {
        match self.next()? {
            Token::Text(value) | Token::Word(value) => Ok(value),
            _ => Err("expected an enumeration value".to_owned()),
        }
    }

    fn restriction_facets(&mut self) -> Result<Vec<PropertyFacet>, String> {
        self.expect_punct('(')?;
        let mut facets = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        loop {
            let name = match self.next()? {
                Token::Word(name) => name,
                _ => return Err("expected a restriction facet".to_owned()),
            };
            let value = self.value()?;
            let facet = property_facet(&name, value)?;
            if !seen.insert(facet.name()) {
                return Err(format!("duplicate `{}` facet", facet.name()));
            }
            facets.push(facet);
            if !self.eat_punct(',')? {
                break;
            }
        }
        self.expect_punct(')')?;
        Ok(facets)
    }
}

fn property_facet(name: &str, value: String) -> Result<PropertyFacet, String> {
    let facet = match name {
        "min_inclusive" => PropertyFacet::MinInclusive(value),
        "min_exclusive" => PropertyFacet::MinExclusive(value),
        "max_inclusive" => PropertyFacet::MaxInclusive(value),
        "max_exclusive" => PropertyFacet::MaxExclusive(value),
        "min_length" => PropertyFacet::MinLength(value),
        "max_length" => PropertyFacet::MaxLength(value),
        "total_digits" => PropertyFacet::TotalDigits(value),
        "fraction_digits" => PropertyFacet::FractionDigits(value),
        "pattern" => PropertyFacet::Pattern(value),
        other => return Err(format!("unknown restriction facet `{other}`")),
    };
    Ok(facet)
}

fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = source.chars().peekable();
    while let Some(char) = chars.next() {
        if char.is_whitespace() {
            continue;
        }
        if matches!(char, '(' | ')' | '[' | ']' | ',') {
            tokens.push(Token::Punct(char));
            continue;
        }
        if char == '"' {
            let mut text = String::new();
            let mut escaped = false;
            loop {
                let Some(next) = chars.next() else {
                    return Err("unterminated quoted range value".to_owned());
                };
                match (escaped, next) {
                    (true, value) => {
                        if !matches!(value, '"' | '\\') {
                            text.push('\\');
                        }
                        text.push(value);
                        escaped = false;
                    }
                    (false, '\\') => escaped = true,
                    (false, '"') => break,
                    (false, value) => text.push(value),
                }
            }
            tokens.push(Token::Text(text));
            continue;
        }
        let mut word = String::new();
        word.push(char);
        while let Some(&next) = chars.peek() {
            if next.is_whitespace() || matches!(next, '(' | ')' | '[' | ']' | ',' | '"') {
                break;
            }
            word.push(next);
            chars.next();
        }
        tokens.push(Token::Word(word));
    }
    Ok(tokens)
}
