//! Use the rendered file to locate local changes in the original template.
use similar::{capture_diff_slices, Algorithm, DiffTag};
use std::ops::Range;

pub(super) const PRIVATE_LINE: &str = "[private local reference] ";

fn append(out: &mut String, lines: &[&str]) {
    out.extend(lines.iter().copied());
}

fn end_line(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

fn difference(
    out: &mut String,
    source: &[&str],
    rendered: &[&str],
    local: &[&str],
    protect_local: bool,
) {
    if rendered == local || source == local {
        append(out, source);
        return;
    }
    end_line(out);
    out.push_str("<<<<<<< managed source (retain template logic)\n");
    append(out, source);
    end_line(out);
    out.push_str("=======\n");
    if protect_local {
        // These lines may be credentials. A merge editor's "accept incoming"
        // must not silently turn a rendered value into a catalog literal.
        for line in local {
            out.push_str(PRIVATE_LINE);
            out.push_str(line);
        }
    } else {
        append(out, local);
    }
    end_line(out);
    out.push_str(">>>>>>> local (reconcile into the managed source)\n");
}

pub(super) fn document(
    source: &str,
    rendered: &str,
    local: &str,
    sensitive: bool,
    template: bool,
) -> Vec<u8> {
    let source: Vec<_> = source.split_inclusive('\n').collect();
    let rendered: Vec<_> = rendered.split_inclusive('\n').collect();
    let local: Vec<_> = local.split_inclusive('\n').collect();
    let mut aligned = vec![None; rendered.len()];
    for op in capture_diff_slices(Algorithm::Patience, &rendered, &local) {
        if op.tag() == DiffTag::Equal {
            for (r, l) in op.old_range().zip(op.new_range()) {
                aligned[r] = Some(l);
            }
        }
    }

    let mut out = String::new();
    let (mut source_pos, mut rendered_pos, mut local_pos) = (0, 0, 0);
    let mut anchor = |s: Range<usize>, r: Range<usize>, l: Range<usize>| {
        let managed = &source[source_pos..s.start];
        let base = &rendered[rendered_pos..r.start];
        difference(
            &mut out,
            managed,
            base,
            &local[local_pos..l.start],
            sensitive && (!template || managed != base),
        );
        append(&mut out, &source[s.clone()]);
        (source_pos, rendered_pos, local_pos) = (s.end, r.end, l.end);
    };
    for op in capture_diff_slices(Algorithm::Patience, &rendered, &source) {
        let r = op.old_range();
        let s = op.new_range();
        if op.tag() == DiffTag::Equal {
            for (ri, si) in r.zip(s) {
                if let Some(li) = aligned[ri] {
                    anchor(si..si + 1, ri..ri + 1, li..li + 1);
                }
            }
        } else if r.is_empty() {
            // Source-only lines, such as an inactive branch, sit between rendered lines.
            // Anchor them beside a surviving neighbour, when one exists.
            let boundary = aligned.get(r.start).copied().flatten().or_else(|| {
                r.start
                    .checked_sub(1)
                    .and_then(|i| aligned[i])
                    .map(|i| i + 1)
            });
            if let Some(li) = boundary {
                anchor(s, r, li..li);
            }
        } else if let Some(li) = aligned[r.start] {
            // An unchanged expansion is an anchor too, including expressions producing
            // multiple lines. Emit its original syntax, even beside a changed setting.
            if aligned[r.clone()]
                .iter()
                .enumerate()
                .all(|(i, pos)| *pos == Some(li + i))
            {
                let end = li + r.len();
                anchor(s, r, li..end);
            }
        }
    }
    anchor(
        source.len()..source.len(),
        rendered.len()..rendered.len(),
        local.len()..local.len(),
    );
    out.into_bytes()
}
