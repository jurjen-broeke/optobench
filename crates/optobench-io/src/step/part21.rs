//! The STEP exchange structure: ISO 10303-21 ("Part 21") clear-text files.
//!
//! Written from the standard's grammar (ISO 10303-21:2016, clauses 5–7): a header
//! section and data sections of entity instances, each simple (`#1=NAME(…);`) or complex
//! (`#1=(A(…)B(…));`, one record per partial entity). Parameters are integers, reals,
//! strings, enumerations, binaries, references, `$` (omitted), `*` (derived), lists and
//! typed parameters (`LENGTH_MEASURE(1.0)`). Comments are `/* … */`.
//!
//! Strings are decoded: `''` is a quote, `\\` a backslash, `\X2\…\X0\` UTF-16 and
//! `\X4\…\X0\` UTF-32 in hexadecimal, `\X\hh` one ISO 8859-1 byte, `\S\c` the character
//! c + 128, and `\P?\` page switches are dropped.

use std::collections::HashMap;

/// A parameter value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// An integer.
    Integer(i64),
    /// A real.
    Real(f64),
    /// A decoded string.
    String(String),
    /// An enumeration, without the dots: `.T.` is `T`.
    Enumeration(String),
    /// A binary, as its hexadecimal text.
    Binary(String),
    /// A reference to entity `#n`.
    Reference(u64),
    /// `$`: no value.
    Omitted,
    /// `*`: derived by the schema.
    Derived,
    /// A list.
    List(Vec<Value>),
    /// A typed parameter, such as `LENGTH_MEASURE(1.0)`.
    Typed(String, Box<Value>),
}

impl Value {
    /// The value as a number (integers widen), unwrapping a typed parameter.
    pub fn number(&self) -> Option<f64> {
        match self {
            Value::Real(v) => Some(*v),
            Value::Integer(i) => Some(*i as f64),
            Value::Typed(_, inner) => inner.number(),
            _ => None,
        }
    }

    /// The referenced entity number.
    pub fn reference(&self) -> Option<u64> {
        match self {
            Value::Reference(r) => Some(*r),
            _ => None,
        }
    }

    /// The list's items.
    pub fn list(&self) -> Option<&[Value]> {
        match self {
            Value::List(items) => Some(items),
            _ => None,
        }
    }

    /// The string.
    pub fn text(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// The enumeration's name.
    pub fn enumeration(&self) -> Option<&str> {
        match self {
            Value::Enumeration(e) => Some(e),
            _ => None,
        }
    }

    /// A STEP logical or boolean: `.T.` is true, `.F.` false, anything else `None`.
    pub fn boolean(&self) -> Option<bool> {
        match self.enumeration()? {
            "T" => Some(true),
            "F" => Some(false),
            _ => None,
        }
    }
}

/// One record: a simple entity, or one partial entity of a complex one.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// The entity type name, upper case.
    pub name: String,
    /// The parameters in order.
    pub parameters: Vec<Value>,
}

/// One entity instance.
#[derive(Debug, Clone, PartialEq)]
pub enum Instance {
    /// `#n=NAME(…);`
    Simple(Record),
    /// `#n=(A(…)B(…));`
    Complex(Vec<Record>),
}

impl Instance {
    /// The record of type `name`: the instance itself if simple, or that partial entity.
    pub fn record(&self, name: &str) -> Option<&Record> {
        match self {
            Instance::Simple(r) => (r.name == name).then_some(r),
            Instance::Complex(records) => records.iter().find(|r| r.name == name),
        }
    }

    /// Whether the instance is of type `name` (or has it as a partial entity).
    pub fn is(&self, name: &str) -> bool {
        self.record(name).is_some()
    }

    /// The type name of a simple instance; for a complex one, its records' names joined
    /// by `+`.
    pub fn type_name(&self) -> String {
        match self {
            Instance::Simple(r) => r.name.clone(),
            Instance::Complex(records) => records
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>()
                .join("+"),
        }
    }
}

/// A parsed exchange structure.
#[derive(Debug, Clone, Default)]
pub struct Exchange {
    /// Header entities in order (FILE_DESCRIPTION, FILE_NAME, FILE_SCHEMA, …).
    pub header: Vec<Record>,
    /// Data instances by entity number.
    pub entities: HashMap<u64, Instance>,
}

impl Exchange {
    /// The header record of type `name`.
    pub fn header_record(&self, name: &str) -> Option<&Record> {
        self.header.iter().find(|r| r.name == name)
    }
}

/// Why a file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line.
    pub line: usize,
    /// What was expected or found.
    pub message: String,
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Keyword(String),
    Entity(u64),
    Integer(i64),
    Real(f64),
    String(String),
    Enumeration(String),
    Binary(String),
    Dollar,
    Star,
    Open,
    Close,
    Comma,
    Equals,
    Semicolon,
}

struct Lexer<'a> {
    bytes: &'a [u8],
    at: usize,
    line: usize,
}

impl<'a> Lexer<'a> {
    fn error<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError {
            line: self.line,
            message: message.into(),
        })
    }

    fn skip_space(&mut self) -> Result<(), ParseError> {
        loop {
            match self.bytes.get(self.at) {
                Some(b'\n') => {
                    self.line += 1;
                    self.at += 1;
                }
                Some(c) if c.is_ascii_whitespace() => self.at += 1,
                Some(b'/') if self.bytes.get(self.at + 1) == Some(&b'*') => {
                    self.at += 2;
                    loop {
                        match self.bytes.get(self.at) {
                            None => return self.error("unterminated comment"),
                            Some(b'*') if self.bytes.get(self.at + 1) == Some(&b'/') => {
                                self.at += 2;
                                break;
                            }
                            Some(b'\n') => {
                                self.line += 1;
                                self.at += 1;
                            }
                            Some(_) => self.at += 1,
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn take_while(&mut self, keep: impl Fn(u8) -> bool) -> &'a str {
        let start = self.at;
        while self.bytes.get(self.at).is_some_and(|&c| keep(c)) {
            self.at += 1;
        }
        // Only ASCII is taken here, so the slice is valid UTF-8.
        core::str::from_utf8(&self.bytes[start..self.at]).unwrap_or("")
    }

    fn string(&mut self) -> Result<Token, ParseError> {
        self.at += 1; // opening quote
        let mut raw = Vec::new();
        loop {
            match self.bytes.get(self.at) {
                None => return self.error("unterminated string"),
                Some(b'\'') if self.bytes.get(self.at + 1) == Some(&b'\'') => {
                    raw.push(b'\'');
                    self.at += 2;
                }
                Some(b'\'') => {
                    self.at += 1;
                    break;
                }
                Some(&c) => {
                    if c == b'\n' {
                        self.line += 1;
                    }
                    raw.push(c);
                    self.at += 1;
                }
            }
        }
        let text = String::from_utf8_lossy(&raw);
        Ok(Token::String(decode(&text)))
    }

    fn next(&mut self) -> Result<Option<Token>, ParseError> {
        self.skip_space()?;
        let Some(&c) = self.bytes.get(self.at) else {
            return Ok(None);
        };
        let token = match c {
            b'(' => Token::Open,
            b')' => Token::Close,
            b',' => Token::Comma,
            b'=' => Token::Equals,
            b';' => Token::Semicolon,
            b'$' => Token::Dollar,
            b'*' => Token::Star,
            b'\'' => return self.string().map(Some),
            b'"' => {
                self.at += 1;
                let hex = self.take_while(|c| c != b'"').to_owned();
                self.at += 1;
                return Ok(Some(Token::Binary(hex)));
            }
            b'#' => {
                self.at += 1;
                let digits = self.take_while(|c| c.is_ascii_digit());
                return match digits.parse() {
                    Ok(n) => Ok(Some(Token::Entity(n))),
                    Err(_) => self.error("`#` must be followed by an entity number"),
                };
            }
            b'.' if self
                .bytes
                .get(self.at + 1)
                .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_') =>
            {
                self.at += 1;
                let name = self
                    .take_while(|c| c.is_ascii_alphanumeric() || c == b'_')
                    .to_owned();
                if self.bytes.get(self.at) != Some(&b'.') {
                    return self.error("enumeration must end with `.`");
                }
                self.at += 1;
                return Ok(Some(Token::Enumeration(name.to_ascii_uppercase())));
            }
            c if c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.' => {
                let text = self.take_while(|c| {
                    c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.' | b'E' | b'e')
                });
                if text.contains(['.', 'E', 'e']) {
                    // "1." and "1.E-06" are valid reals; Rust parses them.
                    return match text.parse::<f64>() {
                        Ok(v) => Ok(Some(Token::Real(v))),
                        Err(_) => self.error(format!("`{text}` is not a number")),
                    };
                }
                return match text.parse::<i64>() {
                    Ok(v) => Ok(Some(Token::Integer(v))),
                    Err(_) => self.error(format!("`{text}` is not a number")),
                };
            }
            c if c.is_ascii_alphabetic() || c == b'_' || c == b'!' => {
                let name = self
                    .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'!'));
                return Ok(Some(Token::Keyword(name.to_ascii_uppercase())));
            }
            other => return self.error(format!("unexpected character `{}`", other as char)),
        };
        self.at += 1;
        Ok(Some(token))
    }
}

/// Decodes the control directives of a STEP string.
fn decode(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let hex_at = |i: usize, n: usize| -> Option<u32> {
        let s: String = chars.get(i..i + n)?.iter().collect();
        u32::from_str_radix(&s, 16).ok()
    };
    while i < chars.len() {
        if chars[i] != '\\' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let rest: String = chars[i..chars.len().min(i + 4)].iter().collect();
        if rest.starts_with("\\\\") {
            out.push('\\');
            i += 2;
        } else if rest.starts_with("\\X2\\") || rest.starts_with("\\X4\\") {
            let width = if rest.starts_with("\\X2\\") { 4 } else { 8 };
            i += 4;
            let mut units = Vec::new();
            while i + width <= chars.len() && chars[i] != '\\' {
                match hex_at(i, width) {
                    Some(v) => units.push(v),
                    None => break,
                }
                i += width;
            }
            if width == 4 {
                let utf16: Vec<u16> = units.iter().map(|&u| u as u16).collect();
                out.push_str(&String::from_utf16_lossy(&utf16));
            } else {
                out.extend(units.iter().filter_map(|&u| char::from_u32(u)));
            }
            // Skip the closing \X0\.
            let close: String = chars[i..chars.len().min(i + 4)].iter().collect();
            if close == "\\X0\\" {
                i += 4;
            }
        } else if rest.starts_with("\\X\\") {
            if let Some(v) = hex_at(i + 3, 2) {
                out.extend(char::from_u32(v));
                i += 5;
            } else {
                out.push('\\');
                i += 1;
            }
        } else if rest.starts_with("\\S\\") && i + 3 < chars.len() {
            out.extend(char::from_u32(chars[i + 3] as u32 + 128));
            i += 4;
        } else if rest.starts_with("\\P") && rest.ends_with('\\') && rest.len() == 4 {
            i += 4;
        } else {
            out.push('\\');
            i += 1;
        }
    }
    out
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    peeked: Option<Token>,
}

impl<'a> Parser<'a> {
    fn peek(&mut self) -> Result<Option<&Token>, ParseError> {
        if self.peeked.is_none() {
            self.peeked = self.lexer.next()?;
        }
        Ok(self.peeked.as_ref())
    }

    fn next(&mut self) -> Result<Token, ParseError> {
        match self.peeked.take() {
            Some(t) => Ok(t),
            None => match self.lexer.next()? {
                Some(t) => Ok(t),
                None => self.lexer.error("unexpected end of file"),
            },
        }
    }

    fn expect(&mut self, wanted: &Token, what: &str) -> Result<(), ParseError> {
        let token = self.next()?;
        if &token == wanted {
            Ok(())
        } else {
            self.lexer
                .error(format!("expected {what}, found {token:?}"))
        }
    }

    fn keyword(&mut self, wanted: &str) -> Result<(), ParseError> {
        match self.next()? {
            Token::Keyword(k) if k == wanted => Ok(()),
            other => self
                .lexer
                .error(format!("expected `{wanted}`, found {other:?}")),
        }
    }

    /// `( value, value, … )` after the opening parenthesis has been read.
    fn parameters(&mut self) -> Result<Vec<Value>, ParseError> {
        let mut values = Vec::new();
        if self.peek()? == Some(&Token::Close) {
            self.next()?;
            return Ok(values);
        }
        loop {
            values.push(self.value()?);
            match self.next()? {
                Token::Comma => continue,
                Token::Close => return Ok(values),
                other => {
                    return self
                        .lexer
                        .error(format!("expected `,` or `)`, found {other:?}"));
                }
            }
        }
    }

    fn value(&mut self) -> Result<Value, ParseError> {
        Ok(match self.next()? {
            Token::Integer(i) => Value::Integer(i),
            Token::Real(r) => Value::Real(r),
            Token::String(s) => Value::String(s),
            Token::Enumeration(e) => Value::Enumeration(e),
            Token::Binary(b) => Value::Binary(b),
            Token::Entity(n) => Value::Reference(n),
            Token::Dollar => Value::Omitted,
            Token::Star => Value::Derived,
            Token::Open => Value::List(self.parameters()?),
            Token::Keyword(name) => {
                self.expect(&Token::Open, "`(` after a typed parameter")?;
                let mut inner = self.parameters()?;
                let value = if inner.len() == 1 {
                    inner.remove(0)
                } else {
                    Value::List(inner)
                };
                Value::Typed(name, Box::new(value))
            }
            other => {
                return self
                    .lexer
                    .error(format!("expected a value, found {other:?}"));
            }
        })
    }

    fn record(&mut self, name: String) -> Result<Record, ParseError> {
        self.expect(&Token::Open, "`(`")?;
        Ok(Record {
            name,
            parameters: self.parameters()?,
        })
    }

    fn header(&mut self) -> Result<Vec<Record>, ParseError> {
        self.keyword("HEADER")?;
        self.expect(&Token::Semicolon, "`;`")?;
        let mut records = Vec::new();
        loop {
            match self.next()? {
                Token::Keyword(k) if k == "ENDSEC" => {
                    self.expect(&Token::Semicolon, "`;`")?;
                    return Ok(records);
                }
                Token::Keyword(k) => {
                    records.push(self.record(k)?);
                    self.expect(&Token::Semicolon, "`;` after a header entity")?;
                }
                other => {
                    return self
                        .lexer
                        .error(format!("expected a header entity, found {other:?}"));
                }
            }
        }
    }

    fn instance(&mut self) -> Result<Instance, ParseError> {
        let instance = match self.next()? {
            Token::Keyword(name) => Instance::Simple(self.record(name)?),
            Token::Open => {
                let mut records = Vec::new();
                loop {
                    match self.next()? {
                        Token::Keyword(name) => records.push(self.record(name)?),
                        Token::Close => break,
                        other => {
                            return self
                                .lexer
                                .error(format!("expected a partial entity, found {other:?}"));
                        }
                    }
                }
                Instance::Complex(records)
            }
            other => {
                return self
                    .lexer
                    .error(format!("expected an entity, found {other:?}"));
            }
        };
        self.expect(&Token::Semicolon, "`;` after an entity")?;
        Ok(instance)
    }
}

/// Parses an exchange structure.
pub fn parse(text: &str) -> Result<Exchange, ParseError> {
    let mut parser = Parser {
        lexer: Lexer {
            bytes: text.as_bytes(),
            at: 0,
            line: 1,
        },
        peeked: None,
    };
    parser.keyword("ISO-10303-21")?;
    parser.expect(&Token::Semicolon, "`;`")?;
    let mut exchange = Exchange {
        header: parser.header()?,
        entities: HashMap::new(),
    };
    loop {
        match parser.next()? {
            Token::Keyword(k) if k == "DATA" => {
                // DATA; or DATA('name', (schemas));
                if parser.peek()? == Some(&Token::Open) {
                    parser.next()?;
                    parser.parameters()?;
                }
                parser.expect(&Token::Semicolon, "`;` after DATA")?;
                loop {
                    match parser.next()? {
                        Token::Entity(n) => {
                            parser.expect(&Token::Equals, "`=`")?;
                            let instance = parser.instance()?;
                            if exchange.entities.insert(n, instance).is_some() {
                                return parser.lexer.error(format!("entity #{n} is defined twice"));
                            }
                        }
                        Token::Keyword(k) if k == "ENDSEC" => {
                            parser.expect(&Token::Semicolon, "`;`")?;
                            break;
                        }
                        other => {
                            return parser
                                .lexer
                                .error(format!("expected `#n=` or ENDSEC, found {other:?}"));
                        }
                    }
                }
            }
            Token::Keyword(k) if k == "END-ISO-10303-21" => {
                parser.expect(&Token::Semicolon, "`;`")?;
                return Ok(exchange);
            }
            other => {
                return parser.lexer.error(format!(
                    "expected DATA or END-ISO-10303-21, found {other:?}"
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: &str = "ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('a test'),'2;1');
FILE_NAME('t.step','2026-11-07',(''),(''),'optobench','tests','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));
ENDSEC;
DATA;
/* a comment */
#1=CARTESIAN_POINT('NONE',(0.,1.5,-2.E-03));
#2 = DIRECTION ( '', ( 0.0, 0.0, 1.0 ) ) ;
#3=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
#4=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#3,'distance_accuracy_value','');
#5=PRODUCT('it''s \\X2\\00E9\\X0\\','\\X\\E9t\\S\\i',$,(#6,#7));
ENDSEC;
END-ISO-10303-21;
";

    #[test]
    fn reads_simple_complex_and_typed_entities() {
        let x = parse(SMALL).expect("valid");
        assert_eq!(x.header.len(), 3);
        assert_eq!(
            x.header_record("FILE_SCHEMA").map(|r| r.parameters.clone()),
            Some(vec![Value::List(vec![Value::String(
                "AUTOMOTIVE_DESIGN".into()
            )])])
        );
        let point = x.entities[&1].record("CARTESIAN_POINT").expect("point");
        assert_eq!(
            point.parameters[1],
            Value::List(vec![Value::Real(0.0), Value::Real(1.5), Value::Real(-2e-3)])
        );
        let unit = &x.entities[&3];
        assert!(unit.is("LENGTH_UNIT") && unit.is("SI_UNIT"));
        assert_eq!(unit.type_name(), "LENGTH_UNIT+NAMED_UNIT+SI_UNIT");
        assert_eq!(
            unit.record("SI_UNIT").map(|r| r.parameters.clone()),
            Some(vec![
                Value::Enumeration("MILLI".into()),
                Value::Enumeration("METRE".into())
            ])
        );
        let measure = x.entities[&4]
            .record("UNCERTAINTY_MEASURE_WITH_UNIT")
            .expect("measure");
        assert_eq!(measure.parameters[0].number(), Some(1e-6));
        assert_eq!(measure.parameters[1].reference(), Some(3));
    }

    #[test]
    fn decodes_string_directives() {
        let x = parse(SMALL).expect("valid");
        let product = x.entities[&5].record("PRODUCT").expect("product");
        assert_eq!(product.parameters[0].text(), Some("it's é"));
        // \X\E9 is é; \S\i is 'i' + 128 = 'é' in ISO 8859-1.
        assert_eq!(product.parameters[1].text(), Some("été"));
        assert_eq!(product.parameters[2], Value::Omitted);
        assert_eq!(decode("a\\\\b"), "a\\b");
        assert_eq!(decode("\\X2\\03B103B2\\X0\\"), "αβ");
    }

    #[test]
    fn reports_errors_with_line_numbers() {
        let bad = SMALL.replace("#2 = DIRECTION", "#2 DIRECTION");
        let error = parse(&bad).expect_err("rejected");
        assert_eq!(error.line, 10);
        let twice = SMALL.replace("#4=", "#1=");
        assert!(
            parse(&twice)
                .expect_err("rejected")
                .message
                .contains("defined twice")
        );
    }
}
