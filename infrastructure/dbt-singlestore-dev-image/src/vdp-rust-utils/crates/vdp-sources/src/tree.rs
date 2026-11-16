//! YAML parsed into a tree that remembers where each node starts and ends.

use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::{Marker, TScalarStyle};

/// A position: 1-based line, 0-based column (characters).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

impl From<Marker> for Pos {
    fn from(mark: Marker) -> Self {
        Self {
            line: mark.line(),
            col: mark.col(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum YValue {
    Scalar {
        text: String,
        plain: bool,
        literal: bool,
    },
    Seq(Vec<YNode>),
    Map(Vec<(YNode, YNode)>),
    Alias(usize),
}

/// A node with where it starts and where the parser saw it end (the position of the following token).
#[derive(Debug, Clone, PartialEq)]
pub struct YNode {
    pub value: YValue,
    pub anchor: usize,
    pub start: Pos,
    pub end: Pos,
}

impl YNode {
    pub fn as_str(&self) -> Option<&str> {
        match &self.value {
            YValue::Scalar { text, .. } => Some(text),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&YNode> {
        match &self.value {
            YValue::Map(entries) => entries
                .iter()
                .find(|(k, _)| k.as_str() == Some(key))
                .map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn items(&self) -> &[YNode] {
        match &self.value {
            YValue::Seq(items) => items,
            _ => &[],
        }
    }

    /// The structure without positions, for comparing two parses.
    pub fn shape(&self) -> Shape {
        match &self.value {
            YValue::Scalar { text, .. } => Shape::Scalar(text.clone()),
            YValue::Alias(id) => Shape::Alias(*id),
            YValue::Seq(items) => Shape::Seq(items.iter().map(Self::shape).collect()),
            YValue::Map(entries) => Shape::Map(
                entries
                    .iter()
                    .map(|(k, v)| (k.shape(), v.shape()))
                    .collect(),
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    Scalar(String),
    Alias(usize),
    Seq(Vec<Shape>),
    Map(Vec<(Shape, Shape)>),
}

enum Open {
    Seq(YNode),
    Map(YNode, Option<YNode>),
}

#[derive(Default)]
struct Builder {
    stack: Vec<Open>,
    documents: Vec<YNode>,
}

impl Builder {
    fn push(&mut self, node: YNode) {
        match self.stack.last_mut() {
            None => self.documents.push(node),
            Some(Open::Seq(seq)) => {
                if let YValue::Seq(items) = &mut seq.value {
                    items.push(node);
                }
            }
            Some(Open::Map(map, key)) => match key.take() {
                None => *key = Some(node),
                Some(k) => {
                    if let YValue::Map(entries) = &mut map.value {
                        entries.push((k, node));
                    }
                }
            },
        }
    }
}

impl MarkedEventReceiver for Builder {
    fn on_event(&mut self, event: Event, mark: Marker) {
        let pos = Pos::from(mark);
        let node = |value, anchor| YNode {
            value,
            anchor,
            start: pos,
            end: pos,
        };
        match event {
            Event::Scalar(text, style, anchor, _) => self.push(node(
                YValue::Scalar {
                    text,
                    plain: style == TScalarStyle::Plain,
                    literal: matches!(style, TScalarStyle::Literal | TScalarStyle::Folded),
                },
                anchor,
            )),
            Event::Alias(id) => self.push(node(YValue::Alias(id), 0)),
            Event::SequenceStart(anchor, _) => {
                self.stack
                    .push(Open::Seq(node(YValue::Seq(Vec::new()), anchor)));
            }
            Event::MappingStart(anchor, _) => {
                self.stack
                    .push(Open::Map(node(YValue::Map(Vec::new()), anchor), None));
            }
            Event::SequenceEnd | Event::MappingEnd => {
                if let Some(Open::Seq(mut done) | Open::Map(mut done, _)) = self.stack.pop() {
                    done.end = pos;
                    self.push(done);
                }
            }
            _ => {}
        }
    }
}

/// The first document of `text` (`None` for an empty file).
pub(crate) fn parse(text: &str) -> Result<Option<YNode>, String> {
    let mut builder = Builder::default();
    Parser::new_from_str(text)
        .load(&mut builder, false)
        .map_err(|e| e.to_string())?;
    Ok(builder.documents.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_positions() {
        let root = parse("a: 1\nlist:\n  - name: x\n  - name: y\nz: 2\n")
            .unwrap()
            .unwrap();
        let list = root.get("list").unwrap();
        assert_eq!(list.items().len(), 2);
        assert_eq!(list.items()[0].start.line, 3);
        assert_eq!(list.items()[1].get("name").unwrap().as_str(), Some("y"));
        assert_eq!(list.end.line, 5);
    }
}
