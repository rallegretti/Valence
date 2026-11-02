//! Systematic (IUPAC 2013-style) names for simple organic molecules:
//! acyclic or monocyclic hydrocarbons (incl. benzene), with alkyl, halo and
//! hydroxy substituents, ene/yne unsaturation and -ol suffixes.
//! Anything outside that scope returns `None` and the caller falls back.

use std::cmp::Reverse;

use crate::graph::MolGraph;

const STEMS: [&str; 20] = [
    "meth", "eth", "prop", "but", "pent", "hex", "hept", "oct", "non", "dec", "undec", "dodec",
    "tridec", "tetradec", "pentadec", "hexadec", "heptadec", "octadec", "nonadec", "icos",
];
const MULT: [&str; 11] = [
    "", "", "di", "tri", "tetra", "penta", "hexa", "hepta", "octa", "nona", "deca",
];
const MULT_COMPLEX: [&str; 11] = [
    "", "", "bis", "tris", "tetrakis", "pentakis", "hexakis", "heptakis", "octakis", "nonakis",
    "decakis",
];

struct Ctx {
    /// Carbon-only adjacency: (carbon index, bond order).
    cadj: Vec<Vec<(usize, u8)>>,
    halo: Vec<Vec<&'static str>>,
    oh: Vec<u8>,
}

fn mult(n: usize) -> &'static str {
    MULT.get(n).copied().unwrap_or("")
}

fn locs(v: &[u32]) -> String {
    v.iter()
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn halo_prefix(z: u8) -> Option<&'static str> {
    Some(match z {
        9 => "fluoro",
        17 => "chloro",
        35 => "bromo",
        53 => "iodo",
        _ => return None,
    })
}

pub fn name(g: &MolGraph) -> Option<String> {
    if !g.is_complete() || g.total_charge() != 0 || g.has_ionic() {
        return None;
    }
    let carbons: Vec<usize> = (0..g.len()).filter(|&i| g.z[i] == 6).collect();
    if carbons.is_empty() || carbons.len() > 20 {
        return None;
    }
    let mut cidx = vec![usize::MAX; g.len()];
    for (k, &c) in carbons.iter().enumerate() {
        cidx[c] = k;
    }
    let n = carbons.len();
    let mut ctx = Ctx {
        cadj: vec![Vec::new(); n],
        halo: vec![Vec::new(); n],
        oh: vec![0; n],
    };

    for i in 0..g.len() {
        match g.z[i] {
            6 => {
                for &(j, e) in g.neighbors(i) {
                    if g.z[j] == 6 {
                        ctx.cadj[cidx[i]].push((cidx[j], g.edges[e].order));
                    }
                }
            }
            1 => {
                let ok = g
                    .neighbors(i)
                    .iter()
                    .all(|&(j, _)| g.z[j] == 6 || g.z[j] == 8);
                if !ok {
                    return None;
                }
            }
            8 => {
                // Only hydroxyl: O bonded to one C and one H by single bonds.
                let nb = g.neighbors(i);
                let c: Vec<usize> = nb
                    .iter()
                    .filter(|&&(j, _)| g.z[j] == 6)
                    .map(|&(j, _)| j)
                    .collect();
                let h = nb.iter().filter(|&&(j, _)| g.z[j] == 1).count();
                if nb.len() != 2
                    || c.len() != 1
                    || h != 1
                    || nb.iter().any(|&(_, e)| g.edges[e].order != 1)
                {
                    return None;
                }
                ctx.oh[cidx[c[0]]] += 1;
            }
            z => {
                let p = halo_prefix(z)?;
                let nb = g.neighbors(i);
                if nb.len() != 1 || g.z[nb[0].0] != 6 {
                    return None;
                }
                ctx.halo[cidx[nb[0].0]].push(p);
            }
        }
    }

    let edges: usize = ctx.cadj.iter().map(|a| a.len()).sum::<usize>() / 2;
    // Carbon skeleton must be connected.
    let mut seen = vec![false; n];
    let mut stack = vec![0];
    seen[0] = true;
    while let Some(u) = stack.pop() {
        for &(v, _) in &ctx.cadj[u] {
            if !seen[v] {
                seen[v] = true;
                stack.push(v);
            }
        }
    }
    if seen.iter().any(|s| !s) {
        return None;
    }
    match edges + 1 - n {
        0 => acyclic(&ctx),
        1 => cyclic(&ctx),
        _ => None,
    }
}

fn order_between(ctx: &Ctx, a: usize, b: usize) -> u8 {
    ctx.cadj[a]
        .iter()
        .find(|&&(v, _)| v == b)
        .map_or(0, |&(_, o)| o)
}

/// IUPAC selection criteria in priority order (smaller is better):
/// most -OH, longest chain, most multiple bonds, most substituents, then
/// lowest locants for -OH, multiple bonds, double bonds, prefixes, and
/// prefixes in alphabetical order.
type SeniorityKey = (
    Reverse<usize>,
    Reverse<usize>,
    Reverse<usize>,
    Reverse<usize>,
    Vec<u32>,
    Vec<u32>,
    Vec<u32>,
    Vec<u32>,
    Vec<u32>,
);

/// Everything needed to compare numberings and to build the name.
struct Analysis {
    key: SeniorityKey,
    oh: Vec<u32>,
    multiple: Vec<(u32, u8)>,
    prefixes: Vec<(String, u32)>,
}

fn analyse(
    ctx: &Ctx,
    parent: &[usize],
    cyclic: bool,
    aromatic: bool,
    principal_oh: bool,
) -> Analysis {
    let len = parent.len();
    let mut oh = Vec::new();
    let mut prefixes: Vec<(String, u32)> = Vec::new();
    for (i, &c) in parent.iter().enumerate() {
        let loc = i as u32 + 1;
        for _ in 0..ctx.oh[c] {
            if principal_oh {
                oh.push(loc);
            } else {
                prefixes.push(("hydroxy".into(), loc));
            }
        }
        for h in &ctx.halo[c] {
            prefixes.push((h.to_string(), loc));
        }
        let prev = if i > 0 {
            Some(parent[i - 1])
        } else if cyclic {
            Some(parent[len - 1])
        } else {
            None
        };
        let next = if i + 1 < len {
            Some(parent[i + 1])
        } else if cyclic {
            Some(parent[0])
        } else {
            None
        };
        for &(v, _) in &ctx.cadj[c] {
            if Some(v) != prev && Some(v) != next && !parent.contains(&v) {
                prefixes.push((branch_name(ctx, v, c), loc));
            }
        }
    }
    let mut multiple = Vec::new();
    if !aromatic {
        let bonds = if cyclic { len } else { len.saturating_sub(1) };
        for i in 0..bonds {
            let o = order_between(ctx, parent[i], parent[(i + 1) % len]);
            if o >= 2 {
                // In rings the bond between the last and first atom gets the locant of the last.
                multiple.push((i as u32 + 1, o));
            }
        }
    }
    let mut prefix_locs: Vec<u32> = prefixes.iter().map(|p| p.1).collect();
    prefix_locs.sort_unstable();
    let mut alpha: Vec<(String, u32)> = prefixes.iter().map(|(n, l)| (alpha_key(n), *l)).collect();
    alpha.sort();
    let multiple_locs: Vec<u32> = multiple.iter().map(|m| m.0).collect();
    let double_locs: Vec<u32> = multiple.iter().filter(|m| m.1 == 2).map(|m| m.0).collect();
    Analysis {
        key: (
            Reverse(oh.len()),
            Reverse(len),
            Reverse(multiple.len()),
            Reverse(prefixes.len()),
            oh.clone(),
            multiple_locs,
            double_locs,
            prefix_locs,
            alpha.iter().map(|a| a.1).collect(),
        ),
        oh,
        multiple,
        prefixes,
    }
}

/// Alphabetisation key: complex names sort on their first letter (after
/// locants), simple ones on the name itself.
fn alpha_key(name: &str) -> String {
    name.trim_start_matches(|c: char| !c.is_ascii_alphabetic())
        .to_string()
}

fn assemble_prefixes(prefixes: &[(String, u32)], show_locants: bool) -> String {
    let mut groups: Vec<(String, Vec<u32>)> = Vec::new();
    for (name, loc) in prefixes {
        match groups.iter_mut().find(|g| &g.0 == name) {
            Some(g) => g.1.push(*loc),
            None => groups.push((name.clone(), vec![*loc])),
        }
    }
    groups.sort_by_key(|g| alpha_key(&g.0));
    let mut parts = Vec::new();
    for (name, mut l) in groups {
        l.sort_unstable();
        let complex = name.chars().any(|c| c.is_ascii_digit() || c == '-');
        let body = match (complex, l.len()) {
            (false, k) => format!("{}{}", mult(k), name),
            (true, 1) => format!("({name})"),
            (true, k) => format!("{}({})", MULT_COMPLEX.get(k).copied().unwrap_or(""), name),
        };
        parts.push(if show_locants {
            format!("{}-{}", locs(&l), body)
        } else {
            body
        });
    }
    parts.join(if show_locants { "-" } else { "" })
}

fn join_prefix(prefix: String, parent: String) -> String {
    if prefix.is_empty() {
        parent
    } else if parent.starts_with(|c: char| c.is_ascii_digit()) {
        format!("{prefix}-{parent}")
    } else {
        format!("{prefix}{parent}")
    }
}

/// Parent hydride + unsaturation + -ol suffix.
fn parent_name(
    len: usize,
    cyclic: bool,
    multiple: &[(u32, u8)],
    oh: &[u32],
    show_unsat_locs: bool,
    show_oh_locs: bool,
) -> Option<String> {
    let stem = format!(
        "{}{}",
        if cyclic { "cyclo" } else { "" },
        STEMS.get(len - 1)?
    );
    let d: Vec<u32> = multiple.iter().filter(|m| m.1 == 2).map(|m| m.0).collect();
    let t: Vec<u32> = multiple.iter().filter(|m| m.1 == 3).map(|m| m.0).collect();
    let mut core = stem;
    if d.is_empty() && t.is_empty() {
        core.push_str("an");
    } else {
        if d.len() > 1 || t.len() > 1 {
            core.push('a');
        }
        if !d.is_empty() {
            if show_unsat_locs {
                core.push_str(&format!("-{}-", locs(&d)));
            }
            core.push_str(&format!("{}en", mult(d.len())));
        }
        if !t.is_empty() {
            if show_unsat_locs {
                core.push_str(&format!("-{}-", locs(&t)));
            }
            core.push_str(&format!("{}yn", mult(t.len())));
        }
    }
    if oh.is_empty() {
        return Some(core + "e");
    }
    let ol = format!("{}ol", mult(oh.len()));
    let sep = if show_oh_locs {
        format!("-{}-", locs(oh))
    } else {
        String::new()
    };
    if ol.starts_with('o') {
        Some(format!("{core}{sep}{ol}"))
    } else {
        Some(format!("{core}e{sep}{ol}"))
    }
}

fn acyclic(ctx: &Ctx) -> Option<String> {
    let n = ctx.cadj.len();
    let ends: Vec<usize> = (0..n).filter(|&c| ctx.cadj[c].len() <= 1).collect();
    let mut best: Option<(Analysis, usize)> = None;
    for &a in &ends {
        for &b in &ends {
            if a == b && n > 1 {
                continue;
            }
            let path = tree_path(ctx, a, b);
            let an = analyse(ctx, &path, false, false, true);
            if best.as_ref().is_none_or(|(x, _)| an.key < x.key) {
                best = Some((an, path.len()));
            }
        }
    }
    let (an, len) = best?;
    let features = an.prefixes.len() + an.oh.len();
    let show_sub = len > 2 || (len == 2 && features > 1);
    let show_unsat = len > 2;
    let parent = parent_name(len, false, &an.multiple, &an.oh, show_unsat, show_sub)?;
    Some(join_prefix(
        assemble_prefixes(&an.prefixes, show_sub),
        parent,
    ))
}

fn cyclic(ctx: &Ctx) -> Option<String> {
    let ring = ring_atoms(ctx)?;
    let len = ring.len();
    let aromatic = len == 6
        && (0..6).all(|i| {
            let prev = order_between(ctx, ring[(i + 5) % 6], ring[i]);
            let next = order_between(ctx, ring[i], ring[(i + 1) % 6]);
            (prev == 2) != (next == 2) && prev <= 2 && next <= 2
        });
    let mut best: Option<Analysis> = None;
    for start in 0..len {
        for dir in [1usize, len - 1] {
            let order: Vec<usize> = (0..len).map(|k| ring[(start + k * dir) % len]).collect();
            let an = analyse(ctx, &order, true, aromatic, true);
            if best.as_ref().is_none_or(|x| an.key < x.key) {
                best = Some(an);
            }
        }
    }
    let an = best?;
    let features = an.prefixes.len() + an.oh.len() + an.multiple.len();
    let show = features > 1;
    let parent = if aromatic {
        match an.oh.len() {
            0 => "benzene".to_string(),
            1 => "phenol".to_string(),
            k => format!("benzene-{}-{}ol", locs(&an.oh), mult(k)),
        }
    } else {
        parent_name(len, true, &an.multiple, &an.oh, show, show)?
    };
    let show_prefix_locs = if aromatic {
        an.prefixes.len() + an.oh.len() > 1
    } else {
        show
    };
    Some(join_prefix(
        assemble_prefixes(&an.prefixes, show_prefix_locs),
        parent,
    ))
}

/// Ring members in cyclic order (single ring assumed).
fn ring_atoms(ctx: &Ctx) -> Option<Vec<usize>> {
    let n = ctx.cadj.len();
    let mut deg: Vec<usize> = ctx.cadj.iter().map(|a| a.len()).collect();
    let mut alive = vec![true; n];
    let mut changed = true;
    while changed {
        changed = false;
        for c in 0..n {
            if alive[c] && deg[c] <= 1 {
                alive[c] = false;
                changed = true;
                for &(v, _) in &ctx.cadj[c] {
                    if alive[v] {
                        deg[v] -= 1;
                    }
                }
            }
        }
    }
    let start = (0..n).find(|&c| alive[c])?;
    let mut ring = vec![start];
    let mut prev = usize::MAX;
    let mut cur = start;
    loop {
        let next = ctx.cadj[cur].iter().map(|&(v, _)| v).find(|&v| {
            alive[v] && v != prev && !(ring.len() > 1 && v == start && ring.len() < 3)
        })?;
        if next == start {
            break;
        }
        if ring.contains(&next) {
            return None;
        }
        ring.push(next);
        prev = cur;
        cur = next;
    }
    Some(ring)
}

fn tree_path(ctx: &Ctx, a: usize, b: usize) -> Vec<usize> {
    let n = ctx.cadj.len();
    let mut parent = vec![usize::MAX; n];
    let mut queue = std::collections::VecDeque::from([a]);
    parent[a] = a;
    while let Some(u) = queue.pop_front() {
        for &(v, _) in &ctx.cadj[u] {
            if parent[v] == usize::MAX {
                parent[v] = u;
                queue.push_back(v);
            }
        }
    }
    let mut path = vec![b];
    let mut cur = b;
    while cur != a {
        cur = parent[cur];
        path.push(cur);
    }
    path.reverse();
    path
}

/// Name of the substituent rooted at `root`, attached to `from`.
fn branch_name(ctx: &Ctx, root: usize, from: usize) -> String {
    // Collect leaves of the branch tree, and paths from root to each.
    let mut best: Option<(Analysis, Vec<usize>)> = None;
    let mut stack = vec![(root, from, vec![root])];
    while let Some((u, p, path)) = stack.pop() {
        let children: Vec<usize> = ctx.cadj[u]
            .iter()
            .map(|&(v, _)| v)
            .filter(|&v| v != p)
            .collect();
        if children.is_empty() {
            let an = analyse_branch(ctx, &path, from);
            if best.as_ref().is_none_or(|(x, _)| an.key < x.key) {
                best = Some((an, path.clone()));
            }
        }
        for v in children {
            let mut np = path.clone();
            np.push(v);
            stack.push((v, u, np));
        }
    }
    let Some((an, path)) = best else {
        return "yl".into();
    };
    let len = path.len();
    let attach_order = order_between(ctx, root, from);
    let stem = STEMS.get(len - 1).copied().unwrap_or("alk");
    let show = len > 1;
    let prefix = assemble_prefixes(&an.prefixes, show);
    let ending = match attach_order {
        2 => "ylidene",
        3 => "ylidyne",
        _ => "yl",
    };
    let core = if an.multiple.is_empty() {
        format!("{stem}{ending}")
    } else if len == 2 {
        let kind = if an.multiple[0].1 == 3 { "yn" } else { "en" };
        format!("eth{kind}{ending}")
    } else {
        let d: Vec<u32> = an
            .multiple
            .iter()
            .filter(|m| m.1 == 2)
            .map(|m| m.0)
            .collect();
        let t: Vec<u32> = an
            .multiple
            .iter()
            .filter(|m| m.1 == 3)
            .map(|m| m.0)
            .collect();
        let mut s = stem.to_string();
        if d.len() > 1 || t.len() > 1 {
            s.push('a');
        }
        if !d.is_empty() {
            s.push_str(&format!("-{}-{}en", locs(&d), mult(d.len())));
        }
        if !t.is_empty() {
            s.push_str(&format!("-{}-{}yn", locs(&t), mult(t.len())));
        }
        format!("{s}-1-{ending}")
    };
    join_prefix(prefix, core)
}

fn analyse_branch(ctx: &Ctx, path: &[usize], from: usize) -> Analysis {
    // Treat the attachment as fixed at locant 1; hydroxyls are prefixes here.
    let mut an = analyse_with_exclusion(ctx, path, from);
    an.key.0 = Reverse(0);
    an
}

fn analyse_with_exclusion(ctx: &Ctx, path: &[usize], exclude: usize) -> Analysis {
    // Same as `analyse`, but never walks back into the parent.
    let len = path.len();
    let mut prefixes: Vec<(String, u32)> = Vec::new();
    for (i, &c) in path.iter().enumerate() {
        let loc = i as u32 + 1;
        for _ in 0..ctx.oh[c] {
            prefixes.push(("hydroxy".into(), loc));
        }
        for h in &ctx.halo[c] {
            prefixes.push((h.to_string(), loc));
        }
        let prev = if i > 0 { path[i - 1] } else { exclude };
        let next = path.get(i + 1).copied();
        for &(v, _) in &ctx.cadj[c] {
            if v != prev && Some(v) != next {
                prefixes.push((branch_name(ctx, v, c), loc));
            }
        }
    }
    let mut multiple = Vec::new();
    for i in 0..len.saturating_sub(1) {
        let o = order_between(ctx, path[i], path[i + 1]);
        if o >= 2 {
            multiple.push((i as u32 + 1, o));
        }
    }
    let mut prefix_locs: Vec<u32> = prefixes.iter().map(|p| p.1).collect();
    prefix_locs.sort_unstable();
    let multiple_locs: Vec<u32> = multiple.iter().map(|m| m.0).collect();
    Analysis {
        key: (
            Reverse(0),
            Reverse(len),
            Reverse(multiple.len()),
            Reverse(prefixes.len()),
            Vec::new(),
            multiple_locs,
            Vec::new(),
            prefix_locs,
            Vec::new(),
        ),
        oh: Vec::new(),
        multiple,
        prefixes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smiles::parse;

    fn n(s: &str) -> String {
        name(&parse(s).unwrap()).unwrap_or_else(|| "<none>".into())
    }

    #[test]
    fn alkanes() {
        assert_eq!(n("C"), "methane");
        assert_eq!(n("CC"), "ethane");
        assert_eq!(n("CCCCCC"), "hexane");
        assert_eq!(n("CC(C)C"), "2-methylpropane");
        assert_eq!(n("CC(C)(C)C"), "2,2-dimethylpropane");
        assert_eq!(n("CCC(C)CC"), "3-methylpentane");
        assert_eq!(n("CC(C)CC(C)C"), "2,4-dimethylpentane");
        assert_eq!(n("CCC(CC)C(C)C"), "3-ethyl-2-methylpentane");
        assert_eq!(
            n("CCCC(C(C)C)CCC"),
            "4-(propan-2-yl)heptane".replace("(propan-2-yl)", "(1-methylethyl)")
        );
    }

    #[test]
    fn unsaturated() {
        assert_eq!(n("C=C"), "ethene");
        assert_eq!(n("C#C"), "ethyne");
        assert_eq!(n("CC=C"), "prop-1-ene");
        assert_eq!(n("CC=CC"), "but-2-ene");
        assert_eq!(n("C=CC=C"), "buta-1,3-diene");
        assert_eq!(n("C=CCC#C"), "pent-1-en-4-yne");
        assert_eq!(n("CC(C)=C"), "2-methylprop-1-ene");
    }

    #[test]
    fn halo_and_alcohols() {
        assert_eq!(n("CCl"), "chloromethane");
        assert_eq!(n("ClCCl"), "dichloromethane");
        assert_eq!(n("CCBr"), "bromoethane");
        assert_eq!(n("ClCCCl"), "1,2-dichloroethane");
        assert_eq!(n("CO"), "methanol");
        assert_eq!(n("CCO"), "ethanol");
        assert_eq!(n("CC(C)O"), "propan-2-ol");
        assert_eq!(n("OCCO"), "ethane-1,2-diol");
        assert_eq!(n("OCC(O)CO"), "propane-1,2,3-triol");
        assert_eq!(n("CC(Cl)CO"), "2-chloropropan-1-ol");
        assert_eq!(n("C=CCO"), "prop-2-en-1-ol");
        assert_eq!(n("FC(F)(F)F"), "tetrafluoromethane");
        assert_eq!(n("BrCCl"), "bromochloromethane");
    }

    #[test]
    fn rings() {
        assert_eq!(n("C1CCCCC1"), "cyclohexane");
        assert_eq!(n("CC1CCCCC1"), "methylcyclohexane");
        assert_eq!(n("CC1CCCCC1C"), "1,2-dimethylcyclohexane");
        assert_eq!(n("OC1CCCCC1"), "cyclohexanol");
        assert_eq!(n("C1=CCCCC1"), "cyclohexene");
        assert_eq!(n("C1=CC=CC=C1"), "benzene");
        assert_eq!(n("CC1=CC=CC=C1"), "methylbenzene");
        assert_eq!(n("ClC1=CC=CC=C1Cl"), "1,2-dichlorobenzene");
        assert_eq!(n("CC1=CC=C(C)C=C1"), "1,4-dimethylbenzene");
        assert_eq!(n("OC1=CC=CC=C1"), "phenol");
        assert_eq!(n("CC1=CC=CC=C1O"), "2-methylphenol");
        assert_eq!(n("CCC1CC1"), "ethylcyclopropane");
    }

    #[test]
    fn out_of_scope() {
        assert_eq!(n("COC"), "<none>");
        assert_eq!(n("CN"), "<none>");
        assert_eq!(n("C1=CC2=CC=CC=C2C=C1"), "<none>");
    }
}
