//! Fourcc preference ordering and operational retry for multi-gpu buffer transfer.
//!
//! Used by client dma-shadow copies and offload framebuffers.
//!
//! Preference order: optional forced format, source/plane format, same bits-per-pixel,
//! 8-bit formats, then any remaining candidate. Callers walk this list when
//! allocate/import/bind/draw fails so a lower-precision transfer is preferred over a
//! hard frame failure.

use crate::backend::allocator::{
    Fourcc, Modifier,
    format::{FormatSet, get_bpp},
};

/// Formats present in both import and bind sets, excluding invalid modifiers.
pub(super) fn intersect_transfer_candidates(import: &FormatSet, bind: &FormatSet) -> FormatSet {
    import
        .intersection(bind)
        .filter(|f| f.modifier != Modifier::Invalid)
        .copied()
        .collect()
}

/// Bind-supported formats only (render-side staging is not imported on the peer).
pub(super) fn bind_transfer_candidates(bind: FormatSet) -> FormatSet {
    bind.into_iter()
        .filter(|f| f.modifier != Modifier::Invalid)
        .collect()
}

pub(super) fn modifiers_for_fourcc(candidates: &FormatSet, code: Fourcc) -> Vec<Modifier> {
    candidates
        .iter()
        .filter(|f| f.code == code)
        .map(|f| f.modifier)
        .collect()
}

/// Build the preferred fourcc attempt order for a transfer. See module docs.
pub(super) fn ordered_transfer_fourccs(
    requested: Option<Fourcc>,
    source: Fourcc,
    candidates: &FormatSet,
) -> Result<Vec<Fourcc>, ()> {
    if let Some(fmt) = requested {
        if !candidates.iter().any(|f| f.code == fmt) {
            return Err(());
        }
    }

    let mut order = Vec::new();
    let mut push = |code: Fourcc| {
        if candidates.iter().any(|f| f.code == code) && !order.contains(&code) {
            order.push(code);
        }
    };

    if let Some(fmt) = requested {
        push(fmt);
    }
    push(source);

    let bpp = get_bpp(source).unwrap_or(8);
    for f in candidates.iter() {
        if get_bpp(f.code).is_some_and(|val| val == bpp) {
            push(f.code);
        }
    }
    for f in candidates.iter() {
        if get_bpp(f.code).is_some_and(|val| val == 8) {
            push(f.code);
        }
    }
    for f in candidates.iter() {
        push(f.code);
    }

    if order.is_empty() { Err(()) } else { Ok(order) }
}

/// Try each fourcc in preference order until `attempt` succeeds.
///
/// Formats with no modifiers are skipped. On total failure returns the last
/// attempt error, if any.
pub(super) fn try_ordered_transfer_formats<E, T>(
    format_order: impl IntoIterator<Item = Fourcc>,
    candidates: &FormatSet,
    mut attempt: impl FnMut(Fourcc, &[Modifier]) -> Result<T, E>,
) -> Result<T, Option<E>> {
    let mut last_err = None;
    for code in format_order {
        let modifiers = modifiers_for_fourcc(candidates, code);
        if modifiers.is_empty() {
            continue;
        }
        match attempt(code, &modifiers) {
            Ok(value) => return Ok(value),
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err)
}
