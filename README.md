# Valence

A 3D molecule-building game. Drag elements out of the periodic table into the
lab, drop more elements onto atoms to bond them, and watch the molecule take
shape, with its common and IUPAC names.

![Valence demo scene](docs/screenshot.png)

```
cargo run --release            # play
cargo run --release -- --demo  # start with a shelf of example molecules
cargo test --workspace         # rules, naming, layout, stress and crash-rollback tests
cargo build --profile dist     # optimised shipping build → target/dist/
```

Runs on Windows (DirectX 12), macOS (Apple Silicon and Intel, Metal) and Linux
(X11 or Wayland, Vulkan).
Requires Rust 1.95+. Linux builds also need `pkg-config libx11-dev
libxkbcommon-dev libwayland-dev` (Debian/Ubuntu names); Windows and macOS need
nothing beyond Rust. No C compiler is required on any platform.

## How to play

| Action | Result |
| --- | --- |
| Drag an element into empty space | Drops a free atom |
| Drag an element onto an atom | Bonds it, if chemistry allows. Green ring = OK, red ring + reason = not allowed |
| Select an atom, then click elements | Builds onto the selected atom without dragging |
| Drag an atom | Moves its molecule; drop it on another atom to bond the two |
| Shift + drag an atom | Moves just that atom (use it to close rings, e.g. benzene) |
| Drag empty space / Shift + drag empty space | Orbit / pan (works on trackpads) |
| Right-drag / middle-drag | Orbit / pan |
| Scroll or pinch | Zoom |
| Right-click, Delete or Backspace on an atom | Removes it (Delete/Backspace also acts on the selected atom) |
| Ctrl/Cmd + Z, Ctrl/Cmd + Shift + Z (or Ctrl + Y) | Undo / redo (also buttons in the panel) |
| F | Frame everything |
| Esc | Cancel a drag |

**Visual language**

- Atoms are spheres coloured by periodic-table column (f-block has its own two colours).
- Bonds are white tubes: one tube = single, two = double, three = triple.
  Ionic bonds are a dotted line of beads.
- Bright yellow dots are open bonding sites (unpaired electrons); pale pairs
  of dots are lone pairs.
- Ions glow (blue = cation, red = anion) and carry a charge badge.
- Every molecule has a tag with its name, formula, IUPAC name, and whether
  it is **complete** (no open bonding sites).

## Interface

- **☰ menu** (left of the title):
  - **File:** New lab, Open example shelf, Export image…, Quit.
  - **Edit:** Undo, Redo, Delete selected atom, Select none.
  - **Settings…**
  - **Help:** Controls, About.
- **Settings window:** graphics quality (Auto/Full/Lite), graphics API
  (Auto, DirectX 12, Vulkan, Metal or OpenGL, depending on OS), the GPU in
  use, and the settings, exported images and crash log locations. Graphics
  changes apply on the next launch. An API that turns out not to be available
  falls back to Auto instead of stopping the game from starting.
- **Sidebar tabs:**
  - **Build:** periodic table, element and molecule info, molecule list, and
    quick toggles (symbols, names, lone pairs).
  - **Reactions:** see below.
  - **Export:** image export with its own quick toggles.

## Exporting images

On the **Export** tab (or **☰ → File → Export image…**), choose what to
export:

- **Whole build area** as you currently see it.
- **Selected molecule:** isolated and framed on its own.
- **Reaction:** its molecules, with the + / → diagram.

Optionally include atom symbols and name tags, and a **details card**. For a
molecule the card shows the name, IUPAC name, formula, molar mass, atoms,
bond types, estimated bond energy, charge, reactivity and radioactivity. The
image is cropped to the 3D view and saved to `Pictures/Valence`, and your view
is restored exactly afterwards.

## Reactions

On the **Reactions** tab, select molecules in the 3D view and add them as
**inputs**, **catalysts** or **outputs**, each with a coefficient. Valence:

- writes the equation, e.g. `2 H₂ + O₂ → 2 H₂O`, with any catalyst over the
  arrow;
- checks that every atom and the charge balance, and can **balance the
  coefficients** automatically (exact integer arithmetic);
- estimates the **energy change** from average bond energies (exothermic or
  endothermic);
- recognises well-known reactions from a built-in library: combustion of
  hydrogen, methane and ethanol; respiration and photosynthesis; the
  Haber–Bosch and Contact processes; H₂O₂ decomposition; neutralisation;
  hydrogenation; limestone decomposition; rusting; and more. It also checks
  whether the right catalyst is present, and **"Build the products"** adds
  them to the lab for you;
- **Arrange in 3D** lays the reaction out left to right (catalyst above the
  arrow) and draws "+", "→", coefficients and captions over the view.

Predicting the products of arbitrary inputs isn't possible with a valence
model. Outside the library you build the outputs yourself, and Valence checks
them.

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

## Stability ("no-crash" design)

- **The core never panics.** `crates/chem` and the app deny `unwrap`, `expect`,
  `panic!` and `unreachable!` outside tests (enforced by clippy). Every lookup
  is checked, every search is bounded (the bond solver is polynomial, not
  exponential), positions are protected from NaN/∞, and the lab is capped at
  2000 atoms.
- **Every edit is transactional.** Player actions go through `Chem::edit`,
  which snapshots the lab and runs the change under `catch_unwind`. A
  rejected action changes nothing. If an unknown bug panics, the lab rolls
  back, the player sees "that change was undone", and the game keeps running.
  Physics and naming are guarded the same way (naming falls back to the formula).
- **Crash log.** Any panic is appended, with a backtrace, to
  `%LOCALAPPDATA%\Valence\crash.log` (Windows),
  `~/Library/Logs/Valence/crash.log` (macOS), or
  `~/.local/state/valence/crash.log` (Linux).
- **Bevy errors are warnings.** A failing system logs instead of quitting
  (`set_error_handler(warn)`). Minimised or tiny windows get a safe viewport,
  and the window has a minimum size.
- **Tested hard.** `crates/chem/tests/stress.rs` runs 300,000 random player
  actions (including garbage input such as invalid elements, dead atom ids
  and NaN positions) and checks every invariant after each one. A dense fused
  carbon sheet exercises the solver's worst case. Separate tests prove that a
  panicking edit rolls back cleanly and that undo/redo works.
- Release builds unwind on panic (so the guard can catch it). The Windows
  release build is a GUI app with no console window.

## Graphics quality (Full / Lite)

One build serves both modern and older hardware:

- **Full:** bloom glow, film-style tone mapping, translucent ion halos and
  lone pairs.
- **Lite:** a single opaque material and no post-processing, so far fewer
  shaders need compiling. Ions get coloured rings instead of glowing halos.
  The chemistry, interface and every feature are the same.
- **Auto** (default) picks Lite for software renderers and Intel integrated
  GPUs up to Gen 9.5 (~2015–2020, e.g. HD 530 / UHD 630), and Full otherwise.
  Override it in the panel (**Graphics: Auto / Full / Lite**, applies next
  launch), or with `--lite` / `--full` on the command line. The choice is
  saved in `settings.txt` in the platform config folder (`%APPDATA%\Valence`,
  `~/Library/Application Support/Valence`, `~/.config/valence`).
- If Full takes more than 10 s to show the 3D view, the panel offers to switch
  to Lite.

Startup is staged: the interface's tiny shaders compile first, so the
periodic table appears almost at once. The 3D scene is built afterwards,
with a "Preparing 3D graphics…" indicator while its shaders compile.
Bevy's unused depth-prepass and deferred-lighting pipelines are switched off
too. (GPU mesh preprocessing must stay on; turning it off stops opaque meshes
drawing in Bevy 0.20.)

Measured time from launch until the interface / the 3D scene are ready:

| Machine | Interface | 3D scene (Lite) | 3D scene (Full) |
|---|---|---|---|
| Linux, RTX 4060 Ti (Vulkan) | 0.1 s | 0.3 s | 0.4 s |
| macOS, Apple A18 Pro (Metal) | 0.1 s | 0.2 s | 0.2 s |
| Windows, Intel HD 530 (DX12) | 3.0 s | 21 s (Auto picks Lite) | 36 s |

On that Windows machine everything used to show black for about 40 s.

## Windows notes

- **Graphics:** the game uses DirectX 12 on Windows. Vulkan is only a
  fallback, because older Intel drivers crash in their Vulkan driver
  (`igvk64.dll`). Shaders are compiled with Microsoft's DXC, linked into the
  exe; the default FXC compiler left the window black on older GPUs. Set
  `WGPU_BACKEND` or `WGPU_DX12_COMPILER` to override either choice.
- **Smart App Control** (Windows 11) blocks unsigned programs it doesn't
  recognise, including freshly built Rust binaries and the game itself. For
  local builds, turn it off. For releases, sign the exe with a trusted
  code-signing certificate (e.g. Azure Trusted Signing) so players with Smart
  App Control on can run it.
- On older integrated GPUs the first frame can take a long time while shaders
  compile; the launch test in `scripts/check.ps1` allows up to 90 s.

## Checking a platform locally

All building and testing happens on your own machines; nothing uses hosted CI.

- **macOS / Linux:** `scripts/check.sh`
- **Windows (PowerShell):** `.\scripts\check.ps1`

Each script runs format, clippy (warnings are errors), the full test suite and
the `dist` build. It then launches the game, renders the demo scene, saves a
screenshot and exits. A pass means the binary builds, starts, draws and shuts
down cleanly on that machine.

From Linux you can also compile-check the other platforms without their SDKs:

```
rustup target add x86_64-pc-windows-msvc aarch64-apple-darwin
cargo check --workspace --all-targets --target x86_64-pc-windows-msvc
cargo check --workspace --all-targets --target aarch64-apple-darwin
```

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
  state.rs     Chem resource: transactional edits, undo/redo, cached names
  crash.rs     panic hook → crash log, catch_unwind guard
  menu.rs      ☰ menu, Settings / Help / About windows
  export.rs    PNG export (isolate, frame, capture, crop, restore)
  reactions_ui.rs  Reactions tab, 3D arrangement and diagram
  facts.rs     molecule facts for info cards and exports
  quality.rs   Full/Lite graphics and staged startup
  debug.rs     --demo scene, VALENCE_SCREENSHOT / VALENCE_FEATURE_TEST / VALENCE_EXPORT_DIR /
               VALENCE_ZOOM / VALENCE_WINDOW / VALENCE_FAKE_DRAG
scripts/      check.sh (macOS/Linux) and check.ps1 (Windows) local verification
```

Performance notes:
- The lab sleeps when the layout settles, so an idle scene does no physics and
  no transform writes.
- Repulsion uses a uniform grid (O(n)). About 0.4 ms per frame for 1000 atoms
  (`cargo test -p valence_chem --release perf -- --ignored --nocapture`).
- Names and molecules are recomputed only when the lab's revision changes;
  drop checks are cached per (target, element, revision).
- Meshes and materials are shared; one material per element.
- Only the Bevy features in use are compiled (no audio, gamepad, glTF, scenes
  or bevy_ui), which means fewer system dependencies and faster builds.
- Full frame rate while focused, throttled to about 10 fps in the background.

Fonts: DejaVu Sans (see `assets/fonts/LICENSE-DejaVu.txt`).
