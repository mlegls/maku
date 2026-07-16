//! Primitive runtime data atoms.

use super::{EntityRef, Figure};
use std::rc::Rc;

#[derive(Clone, Debug)]
pub enum DataAtom<E> {
    Num(f64),
    Kw(Rc<str>),
    Figure(Figure<E>),
    Handle(EntityRef),
    Nothing,
}
