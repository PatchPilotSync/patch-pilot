//! "Propose to Golden" (#35, strategy point 3): take products from a
//! secondary machine into a new Golden Profile version.

use patchpilot_core::{Manifest, ProductId};

use crate::align_products;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProposeError {
    #[error("{machine} has no product matching {query:?}")]
    NotOnMachine { machine: String, query: String },
    #[error("nothing selected to propose")]
    NothingSelected,
}

/// A copy of `golden` that takes the selected products from `machine`.
///
/// `products` are product IDs or names (case-insensitive). For each selected
/// product, the machine's items replace Golden's items of the same kind,
/// format and platform (or are added); Golden items in other slots stay, so
/// proposing a Mac's AU keeps Golden's Windows VST3. Unselected products are untouched. IDs are aligned
/// first ([`align_products`]), so `unknown/x` on one side matches `vendor/x`.
pub fn propose_to_golden(
    golden: &Manifest,
    machine: &Manifest,
    products: &[String],
) -> Result<Manifest, ProposeError> {
    if products.is_empty() {
        return Err(ProposeError::NothingSelected);
    }
    let mut out = golden.clone();
    let mut machine = machine.clone();
    align_products(&mut out, &mut machine);

    let selected = products
        .iter()
        .map(|q| {
            find_product(&machine, q).ok_or_else(|| ProposeError::NotOnMachine {
                machine: machine.machine.name.clone(),
                query: q.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    for id in selected {
        if out.product(&id).is_none()
            && let Some(p) = machine.product(&id)
        {
            out.products.push(p.clone());
        }
        let theirs: Vec<_> = machine
            .items
            .iter()
            .filter(|i| i.product_id.as_ref() == Some(&id))
            .collect();
        out.items.retain(|g| {
            g.product_id.as_ref() != Some(&id)
                || !theirs
                    .iter()
                    .any(|t| g.kind == t.kind && g.format == t.format && g.platform == t.platform)
        });
        out.items.extend(theirs.into_iter().cloned());
    }
    Ok(out)
}

/// A product in `m` by exact ID or case-insensitive name.
pub fn find_product(m: &Manifest, query: &str) -> Option<ProductId> {
    let q = query.trim();
    m.products
        .iter()
        .find(|p| p.canonical_id.0 == q)
        .or_else(|| m.products.iter().find(|p| p.name.eq_ignore_ascii_case(q)))
        .map(|p| p.canonical_id.clone())
}
