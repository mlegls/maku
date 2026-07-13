//! Source spans and macro-expansion provenance for frontend diagnostics.
//!
//! Runtime forms remain compact. Provenance is a parallel path-indexed table,
//! so the evaluator does not pay per-node metadata costs and frontend rewrites
//! can explicitly carry or extend a node's origin.

use crate::edn::{ExpandedSource, Form};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

pub type FormPath = Vec<usize>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub source: Rc<str>,
    pub start: usize,
    pub end: usize,
}

impl SourceSpan {
    pub fn new(source: impl Into<Rc<str>>, start: usize, end: usize) -> Self {
        Self { source: source.into(), start, end }
    }

    pub fn synthetic(source: impl Into<Rc<str>>) -> Self {
        Self::new(source, 0, 0)
    }
}

impl fmt::Display for SourceSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}..{}", self.source, self.start, self.end)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpansionFrame {
    pub name: Rc<str>,
    pub call_site: SourceSpan,
    pub definition: Option<SourceSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    pub authored: SourceSpan,
    pub expansion_stack: Vec<ExpansionFrame>,
}

impl Provenance {
    pub fn authored(span: SourceSpan) -> Self {
        Self { authored: span, expansion_stack: Vec::new() }
    }

    pub fn expanded(
        &self,
        name: impl Into<Rc<str>>,
        call_site: SourceSpan,
        definition: Option<SourceSpan>,
    ) -> Self {
        let mut expansion_stack = self.expansion_stack.clone();
        expansion_stack.push(ExpansionFrame { name: name.into(), call_site, definition });
        Self { authored: self.authored.clone(), expansion_stack }
    }

    pub fn primary_span(&self) -> &SourceSpan {
        self.expansion_stack
            .last()
            .map(|frame| &frame.call_site)
            .unwrap_or(&self.authored)
    }
}

#[derive(Clone, Debug, Default)]
pub struct ProvenanceMap {
    entries: HashMap<FormPath, Provenance>,
}

impl ProvenanceMap {
    pub fn from_source(source: impl Into<Rc<str>>, src: &str, forms: &[Form]) -> Self {
        let source = source.into();
        let mut scanner = SpanScanner::new(src);
        let raw = scanner.scan_all();
        let mut entries = HashMap::new();
        for (index, form) in forms.iter().enumerate() {
            let span = raw
                .get(index)
                .cloned()
                .unwrap_or_else(|| SpanNode::leaf(0, src.len()));
            align(form, &span, &source, &mut vec![index], &mut entries);
        }
        Self { entries }
    }

    pub fn from_expanded(expanded: &ExpandedSource, forms: &[Form]) -> Self {
        let mut map = Self::from_source("<expanded>", &expanded.text, forms);
        for provenance in map.entries.values_mut() {
            if let Some((source, start, end)) =
                expanded.origin_for(provenance.authored.start, provenance.authored.end)
            {
                provenance.authored = SourceSpan::new(source, start, end);
            }
        }
        map
    }

    pub fn synthetic(source: impl Into<Rc<str>>, forms: &[Form]) -> Self {
        let source = source.into();
        let mut entries = HashMap::new();
        for (index, form) in forms.iter().enumerate() {
            align(
                form,
                &SpanNode::leaf(0, 0),
                &source,
                &mut vec![index],
                &mut entries,
            );
        }
        Self { entries }
    }

    pub fn get(&self, path: &[usize]) -> Option<&Provenance> {
        self.entries.get(path)
    }

    pub fn set(&mut self, path: FormPath, provenance: Provenance) {
        self.entries.insert(path, provenance);
    }

    pub fn carry_subtree(&mut self, from: &[usize], to: &[usize]) {
        let carried: Vec<_> = self
            .entries
            .iter()
            .filter_map(|(path, provenance)| {
                path.strip_prefix(from).map(|tail| {
                    let mut target = to.to_vec();
                    target.extend_from_slice(tail);
                    (target, provenance.clone())
                })
            })
            .collect();
        self.entries.extend(carried);
    }

    pub fn record_expansion(
        &mut self,
        path: &[usize],
        name: impl Into<Rc<str>>,
        call_site: SourceSpan,
        definition: Option<SourceSpan>,
    ) {
        let name = name.into();
        let inherited = (0..=path.len())
            .rev()
            .find_map(|len| self.entries.get(&path[..len]).cloned());
        let mut updated = false;
        for (candidate, provenance) in &mut self.entries {
            if candidate.starts_with(path) {
                *provenance =
                    provenance.expanded(name.clone(), call_site.clone(), definition.clone());
                updated = true;
            }
        }
        if !updated {
            let provenance = inherited
                .unwrap_or_else(|| Provenance::authored(call_site.clone()))
                .expanded(name, call_site, definition);
            self.entries.insert(path.to_vec(), provenance);
        }
    }
}

#[derive(Clone, Debug)]
struct SpanNode {
    start: usize,
    end: usize,
    children: Vec<SpanNode>,
    prefix: Option<u8>,
}

impl SpanNode {
    fn leaf(start: usize, end: usize) -> Self {
        Self { start, end, children: Vec::new(), prefix: None }
    }
}

struct SpanScanner<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> SpanScanner<'a> {
    fn new(src: &'a str) -> Self {
        Self { src: src.as_bytes(), pos: 0 }
    }

    fn scan_all(&mut self) -> Vec<SpanNode> {
        let mut nodes = Vec::new();
        while self.skip_ws() {
            nodes.push(self.scan_form());
        }
        nodes
    }

    fn scan_form(&mut self) -> SpanNode {
        self.skip_ws();
        let start = self.pos;
        match self.peek() {
            b'(' => self.scan_seq(b')', start, None),
            b'[' => self.scan_seq(b']', start, None),
            b'{' => self.scan_seq(b'}', start, None),
            b'c' | b'p' if self.peek_n(1) == b'[' => {
                let prefix = self.peek();
                self.pos += 1;
                self.scan_seq(b']', start, Some(prefix))
            }
            b'`' | b'\'' => {
                let prefix = self.peek();
                self.pos += 1;
                let child = self.scan_form();
                SpanNode { start, end: child.end, children: vec![child], prefix: Some(prefix) }
            }
            b'~' => {
                self.pos += 1;
                if self.peek() == b'@' {
                    self.pos += 1;
                }
                let child = self.scan_form();
                SpanNode { start, end: child.end, children: vec![child], prefix: Some(b'~') }
            }
            b'm' if self.peek_n(1) == b'"' => {
                self.pos += 1;
                self.scan_string();
                SpanNode::leaf(start, self.pos)
            }
            b'"' => {
                self.scan_string();
                SpanNode::leaf(start, self.pos)
            }
            _ => {
                while self.pos < self.src.len()
                    && !matches!(self.peek(), b' ' | b'\t' | b'\n' | b'\r' | b',' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b';')
                {
                    self.pos += 1;
                }
                SpanNode::leaf(start, self.pos)
            }
        }
    }

    fn scan_seq(&mut self, close: u8, start: usize, prefix: Option<u8>) -> SpanNode {
        self.pos += 1;
        let mut children = Vec::new();
        loop {
            if !self.skip_ws() || self.peek() == close {
                if self.peek() == close {
                    self.pos += 1;
                }
                return SpanNode { start, end: self.pos, children, prefix };
            }
            children.push(self.scan_form());
        }
    }

    fn scan_string(&mut self) {
        self.pos += 1;
        while self.pos < self.src.len() {
            match self.peek() {
                b'\\' => self.pos = (self.pos + 2).min(self.src.len()),
                b'"' => {
                    self.pos += 1;
                    break;
                }
                _ => self.pos += 1,
            }
        }
    }

    fn skip_ws(&mut self) -> bool {
        loop {
            while self.pos < self.src.len()
                && matches!(self.peek(), b' ' | b'\t' | b'\n' | b'\r' | b',')
            {
                self.pos += 1;
            }
            if self.peek() == b';' {
                while self.pos < self.src.len() && self.peek() != b'\n' {
                    self.pos += 1;
                }
            } else {
                return self.pos < self.src.len();
            }
        }
    }

    fn peek(&self) -> u8 {
        self.src.get(self.pos).copied().unwrap_or_default()
    }

    fn peek_n(&self, offset: usize) -> u8 {
        self.src.get(self.pos + offset).copied().unwrap_or_default()
    }
}

fn align(
    form: &Form,
    span: &SpanNode,
    source: &Rc<str>,
    path: &mut FormPath,
    entries: &mut HashMap<FormPath, Provenance>,
) {
    let authored = SourceSpan::new(source.clone(), span.start, span.end);
    entries.insert(path.clone(), Provenance::authored(authored.clone()));
    match form {
        Form::List(items) | Form::Vector(items) => {
            let synthetic_head = match span.prefix {
                Some(b'c') | Some(b'p') | Some(b'`') | Some(b'\'') | Some(b'~') => 1,
                _ => 0,
            };
            for (index, child) in items.iter().enumerate() {
                let raw = index
                    .checked_sub(synthetic_head)
                    .and_then(|raw| span.children.get(raw))
                    .unwrap_or(span);
                path.push(index);
                align(child, raw, source, path, entries);
                path.pop();
            }
        }
        Form::Map(fields) => {
            for (index, (key, value)) in fields.iter().enumerate() {
                let key_span = span.children.get(index * 2).unwrap_or(span);
                path.push(index * 2);
                align(key, key_span, source, path, entries);
                path.pop();
                let value_span = span.children.get(index * 2 + 1).unwrap_or(span);
                path.push(index * 2 + 1);
                align(value, value_span, source, path, entries);
                path.pop();
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edn::read_all;

    #[test]
    fn records_authored_child_spans_and_accessor_sugar_origin() {
        let source = "(let [x 1] {:value x :point c[2 3]})";
        let forms = read_all(source).unwrap();
        let map = ProvenanceMap::from_source("fixture.maku", source, &forms);
        assert_eq!(&source[map.get(&[0, 1, 1]).unwrap().authored.start..map.get(&[0, 1, 1]).unwrap().authored.end], "1");
        let point = map.get(&[0, 2, 3]).unwrap();
        assert_eq!(point.authored.source.as_ref(), "fixture.maku");
        assert!(point.authored.start < point.authored.end);
    }

    #[test]
    fn nested_expansion_stacks_preserve_call_order() {
        let forms = read_all("(+ 1 :bad)").unwrap();
        let mut map = ProvenanceMap::from_source("fixture.maku", "(+ 1 :bad)", &forms);
        let outer = SourceSpan::new("fixture.maku", 20, 30);
        let inner = SourceSpan::new("lib.maku", 4, 10);
        map.record_expansion(&[0], "outer", outer.clone(), None);
        map.record_expansion(&[0, 2], "inner", inner.clone(), Some(SourceSpan::new("lib.maku", 0, 3)));
        let provenance = map.get(&[0, 2]).unwrap();
        assert_eq!(provenance.expansion_stack.iter().map(|frame| frame.name.as_ref()).collect::<Vec<_>>(), ["outer", "inner"]);
        assert_eq!(provenance.primary_span(), &inner);
    }

    #[test]
    fn imported_and_root_forms_keep_distinct_sources() {
        let expanded = crate::edn::expand_src_traced("(import \"prelude\")\n(defpattern local [] (wait 1))").unwrap();
        let forms = read_all(&expanded.text).unwrap();
        let map = ProvenanceMap::from_expanded(&expanded, &forms);
        let imported = map.get(&[0]).unwrap();
        let local_index = forms
            .iter()
            .position(|form| matches!(form, Form::List(items)
                if matches!(items.first(), Some(Form::Sym(head)) if head.as_ref() == "defpattern")))
            .unwrap();
        let local = map.get(&[local_index]).unwrap();
        assert_eq!(imported.authored.source.as_ref(), "@lib/prelude.maku");
        assert_eq!(local.authored.source.as_ref(), "<card>");
    }
}
