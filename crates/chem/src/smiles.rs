//! Minimal SMILES reader for the built-in compound table (Kekulé form only).
//! Supports: organic-subset atoms with implicit H, bracket atoms with H count
//! and charge, `-` `=` `#` bonds, branches, and ring-closure digits.

use crate::elements::{self, by_symbol, element};
use crate::graph::{BondKind, MolGraph};

struct Pending {
    z: u8,
    charge: i8,
    explicit_h: Option<u8>,
}

pub fn parse(s: &str) -> Result<MolGraph, String> {
    let b = s.as_bytes();
    let mut atoms: Vec<Pending> = Vec::new();
    let mut bonds: Vec<(usize, usize, u8)> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut prev: Option<usize> = None;
    let mut order: u8 = 1;
    let mut rings: [Option<(usize, u8)>; 10] = [None; 10];
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        match c {
            '-' => order = 1,
            '=' => order = 2,
            '#' => order = 3,
            '(' => stack.push(prev.ok_or("branch before atom")?),
            ')' => prev = Some(stack.pop().ok_or("unbalanced )")?),
            '0'..='9' => {
                let d = c as usize - '0' as usize;
                let p = prev.ok_or("ring digit before atom")?;
                match rings[d].take() {
                    Some((q, o)) => bonds.push((q, p, o.max(order))),
                    None => rings[d] = Some((p, order)),
                }
                order = 1;
            }
            '[' => {
                let end = s[i..].find(']').ok_or("unclosed [")? + i;
                let inner = &s[i + 1..end];
                let atom = parse_bracket(inner)?;
                atoms.push(atom);
                let idx = atoms.len() - 1;
                if let Some(p) = prev {
                    bonds.push((p, idx, order));
                }
                prev = Some(idx);
                order = 1;
                i = end;
            }
            _ if c.is_ascii_uppercase() => {
                let two = s.get(i..i + 2).unwrap_or("");
                let (sym, len) = if two == "Cl" || two == "Br" {
                    (two, 2)
                } else {
                    (&s[i..i + 1], 1)
                };
                let e = by_symbol(sym).ok_or_else(|| format!("unknown atom {sym}"))?;
                atoms.push(Pending {
                    z: e.z,
                    charge: 0,
                    explicit_h: None,
                });
                let idx = atoms.len() - 1;
                if let Some(p) = prev {
                    bonds.push((p, idx, order));
                }
                prev = Some(idx);
                order = 1;
                i += len - 1;
            }
            _ => return Err(format!("unsupported SMILES char {c:?} in {s}")),
        }
        i += 1;
    }

    let mut used = vec![0u8; atoms.len()];
    for &(a, b, o) in &bonds {
        used[a] += o;
        used[b] += o;
    }
    let mut g = MolGraph::default();
    let mut heavy = Vec::with_capacity(atoms.len());
    let mut hs: Vec<(usize, u8)> = Vec::new();
    for (idx, a) in atoms.iter().enumerate() {
        let protonated = a.charge > 0 && elements::can_protonate(a.z);
        let p = protonated as u8;
        let h = match a.explicit_h {
            Some(h) => h,
            None => {
                let v = element(a.z)
                    .valences
                    .iter()
                    .copied()
                    .find(|&v| v >= used[idx])
                    .unwrap_or(used[idx]);
                v - used[idx]
            }
        };
        let total = used[idx] + h;
        let cap = element(a.z)
            .valences
            .iter()
            .copied()
            .find(|&v| v + p >= total)
            .map(|v| v + p)
            .unwrap_or(total);
        heavy.push(g.add_atom(a.z, a.charge, cap - total));
        hs.push((idx, h));
    }
    for &(a, b, o) in &bonds {
        g.add_edge(heavy[a], heavy[b], o, BondKind::Covalent);
    }
    for (idx, h) in hs {
        for _ in 0..h {
            let hi = g.add_atom(1, 0, 0);
            g.add_edge(heavy[idx], hi, 1, BondKind::Covalent);
        }
    }
    Ok(g)
}

fn parse_bracket(inner: &str) -> Result<Pending, String> {
    let bytes = inner.as_bytes();
    let mut i = 1;
    if bytes.len() > 1 && bytes[1].is_ascii_lowercase() {
        i = 2;
    }
    let sym = &inner[..i];
    let e = by_symbol(sym).ok_or_else(|| format!("unknown atom [{sym}]"))?;
    let mut h = 0u8;
    let mut charge = 0i8;
    let rest = &inner[i..];
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'H' => {
                let mut n = 1;
                if let Some(d) = chars.peek().and_then(|c| c.to_digit(10)) {
                    n = d as u8;
                    chars.next();
                }
                h = n;
            }
            '+' | '-' => {
                let sign = if c == '+' { 1 } else { -1 };
                let mut n = 1;
                if let Some(d) = chars.peek().and_then(|c| c.to_digit(10)) {
                    n = d as i8;
                    chars.next();
                }
                charge += sign * n;
            }
            _ => return Err(format!("unsupported bracket content {inner}")),
        }
    }
    Ok(Pending {
        z: e.z,
        charge,
        explicit_h: Some(h),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        let w = parse("O").unwrap();
        assert_eq!(w.formula(), "H₂O");
        assert!(w.is_complete());
        let a = parse("CC(=O)O").unwrap();
        assert_eq!(a.formula(), "C₂H₄O₂");
        let nh4 = parse("[NH4+]").unwrap();
        assert_eq!(nh4.formula(), "NH₄⁺");
        assert!(nh4.is_complete());
        let co = parse("[C]=O").unwrap();
        assert_eq!(co.total_free(), 2);
        let bz = parse("C1=CC=CC=C1").unwrap();
        assert_eq!(bz.formula(), "C₆H₆");
        assert!(bz.is_complete());
        let s = parse("OS(=O)(=O)O").unwrap();
        assert!(s.is_complete());
        assert_eq!(s.formula(), "H₂SO₄");
    }
}
