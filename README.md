# Valence

A 3D molecule-building game. Drag elements out of the periodic table into the
lab, drop more elements onto atoms to bond them, and watch the molecule take
shape, with its common and IUPAC names.

![Valence demo scene](docs/screenshot.png)

```
cargo run --release            # play
cargo run --release -- --demo  # start with a shelf of example molecules
cargo test --workspace         # chemistry rules, naming and layout tests
```

## How to play

| Action | Result |
| --- | --- |
| Drag an element into empty space | Drops a free atom |
| Drag an element onto an atom | Bonds it, if chemistry allows. Green ring = OK, red ring + reason = not allowed |
| Select an atom, then click elements | Builds onto the selected atom without dragging |
| Drag an atom | Moves its molecule; drop it on another atom to bond the two |
| Shift + drag an atom | Moves just that atom (use it to close rings, e.g. benzene) |
| Right-drag / middle-drag / scroll | Orbit / pan / zoom |
| Right-click or Delete on an atom | Removes it |
| F | Frame everything |

**Visual language**

- Atoms are spheres coloured by periodic-table column (f-block has its own two colours).
- Bonds are white tubes: one tube = single, two = double, three = triple.
  Ionic bonds are a dotted line of beads.
- Bright yellow dots are open bonding sites (unpaired electrons); pale pairs
  of dots are lone pairs.
- Ions glow (blue = cation, red = anion) and carry a charge badge.
- Every molecule has a tag with its name, formula, IUPAC name, and whether
  it is **complete** (no open bonding sites).

## Chemistry model

A simplified but grounded valence model, in `crates/chem`:

- **Valence capacity**: each element has its allowed valences (C 4; N 3;
  O 2; P 3/5; S 2/4/6; Cl 1/3/5/7; Xe 0/2/4/6; metals use common oxidation
  states). An atom uses the smallest that fits and expands when needed, so
  SO₂ → SO₃ and PCl₃ → PCl₅ work.
- **Bond orders are solved, not chosen**: after each change, a maximum
  b-matching (greedy plus augmenting paths) raises bond orders to fill open
  sites. O + O becomes O=O, N + N becomes N≡N, and adding H to acetylene
  hydrogenates it to ethene and then ethane.
- **Ionic vs covalent**: alkali and alkaline-earth metals (except Be), or any
  metal–nonmetal pair with ΔEN ≥ 1.7, transfer electrons and form ions
  (NaCl, MgO, Ca(OH)₂). Metal + metal is rejected (that would be an alloy).
- **Protonation**: H on a full N/O/P/S with a lone pair makes an -onium ion
  (NH₄⁺, H₃O⁺).
- **Refusals** are explained: noble gases, superheavy elements, Kr/Xe/Rn
  partner limits, full octets, metals out of electrons.
- **Geometry**: VSEPR-style electron-domain repulsion plus bond springs gives
  tetrahedral CH₄, bent H₂O, linear CO₂ and XeF₂, and planar benzene.

Not modelled: formal-charge resonance structures (so nitrate and ozone can't be
built), coordination complexes, and stereochemistry.

## Naming

1. **Curated table** (`compounds.rs`, ~120 entries as SMILES), matched by a
   structure hash (Weisfeiler–Lehman with aromatic perception). Build order
   doesn't matter, and isomers stay distinct (ethanol vs dimethyl ether).
2. **Ionic compounds**: cation and anion names with Roman numerals and
   polyatomic anions (calcium carbonate, iron(III) chloride, Ca(OH)₂).
3. **Binary covalent**: Greek prefixes (dinitrogen tetroxide, sulfur hexafluoride).
4. **Organic** (`organic.rs`): IUPAC-style names for acyclic and monocyclic
   hydrocarbons, including benzene, with alkyl, halo and hydroxy groups,
   ene/yne, and -ol (e.g. 3-ethyl-2-methylpentane, propan-2-ol,
   1,2-dichlorobenzene).

## Architecture

```
crates/chem   engine-independent core (no Bevy): elements, Lab (rules + solver),
              layout (physics), naming, SMILES, structure hashing. Unit-tested.
src/          Bevy 0.20 front end
  scene.rs     mirrors Lab → entities; structure rebuilt only on change,
               transforms written only while something moves
  interact.rs  ray picking, drag & drop, drop-feedback cache
  ui.rs        egui periodic table, info cards, 2D overlays (symbols, badges, tags)
  camera.rs    orbit camera confined to the right-hand viewport
  debug.rs     --demo scene, VALENCE_SCREENSHOT / VALENCE_ZOOM / VALENCE_FAKE_DRAG
```

Performance notes:
- The lab sleeps when the layout settles, so an idle scene does no physics and
  no transform writes.
- Repulsion uses a uniform grid (O(n)). About 0.4 ms per frame for 1000 atoms
  (`cargo test -p valence_chem --release perf -- --ignored --nocapture`).
- Names and molecules are recomputed only when the lab's revision changes;
  drop checks are cached per (target, element, revision).
- Meshes and materials are shared; one material per element.

Fonts: DejaVu Sans (see `assets/fonts/LICENSE-DejaVu.txt`).
