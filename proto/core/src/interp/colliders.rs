//! Built-in collider cases and constructors.

use super::*;

#[derive(Clone, Debug)]
pub enum ColliderData {
    None,
    Circle { layer: Symbol, center: (f64, f64), radius: f64 },
    CapsuleChain { layer: Symbol, points: Vec<(f64, f64)>, radius: f64 },
}

impl ColliderData {
    pub fn layer(&self) -> Option<Symbol> {
        match self {
            ColliderData::None => None,
            ColliderData::Circle { layer, .. } | ColliderData::CapsuleChain { layer, .. } => {
                Some(*layer)
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum ColliderDynRepr {
    Slot(ColliderSlot),
}

impl DynKind for ColliderData {
    type Repr = ColliderDynRepr;
}

pub type DynCollider = Dyn<ColliderData>;

/// A collider slot: universal collision-routing metadata plus a
/// shape-specific interpretation of the entity's current figure.
#[derive(Clone, Debug)]
pub struct ColliderSlot {
    pub layer: Symbol,
    pub shape: ColliderSlotShape,
}

#[derive(Clone, Debug)]
pub enum ColliderSlotShape {
    Circle { radius: DynNum },
    CapsuleChain { radius: DynNum, slot: CapsuleChainSlot },
}

impl Dyn<ColliderData> {
    pub fn collider(slot: ColliderSlot) -> DynCollider {
        Dyn { repr: ColliderDynRepr::Slot(slot) }
    }

    pub fn collider_circle(layer: Symbol, radius: DynNum) -> DynCollider {
        DynCollider::collider(ColliderSlot {
            layer,
            shape: ColliderSlotShape::Circle { radius },
        })
    }

    pub fn collider_circle_const(layer: Symbol, radius: f64) -> DynCollider {
        DynCollider::collider_circle(layer, DynNum::num(radius))
    }

    pub fn collider_capsule_chain(
        layer: Symbol,
        radius: DynNum,
        slot: CapsuleChainSlot,
    ) -> DynCollider {
        DynCollider::collider(ColliderSlot {
            layer,
            shape: ColliderSlotShape::CapsuleChain { radius, slot },
        })
    }

    pub fn collider_capsule_chain_const(
        layer: Symbol,
        radius: f64,
        slot: CapsuleChainSlot,
    ) -> DynCollider {
        DynCollider::collider_capsule_chain(layer, DynNum::num(radius), slot)
    }

    pub fn repr(&self) -> &ColliderDynRepr {
        &self.repr
    }

    pub fn slot(&self) -> &ColliderSlot {
        match &self.repr {
            ColliderDynRepr::Slot(slot) => slot,
        }
    }

    pub fn capsule_chain(&self) -> Option<(&ColliderSlot, &CapsuleChainSlot, &DynNum)> {
        let slot = self.slot();
        match &slot.shape {
            ColliderSlotShape::CapsuleChain { radius, slot: shape } => Some((slot, shape, radius)),
            ColliderSlotShape::Circle { .. } => None,
        }
    }
}
