# Valence: developer notes

Technical documentation for working on Valence. For what the app does and how
to use it, see the [README](../README.md).

## Building

```bash
cargo run --release            # run the app
cargo run --release -- --demo  # start with a shelf of example molecules
cargo test --workspace         # rules, naming, layout, stress, crash-rollback tests
cargo build --profile dist     # optimised shipping build → target/dist/
```

Requirements:
- Rust 1.95+.
- **Linux:** `pkg-config libx11-dev libxkbcommon-dev libwayland-dev`
  (Debian/Ubuntu names).
- **Windows:** MSVC Build Tools, including the **C++ ATL** component. The
  statically linked DXC shader compiler links `atls.lib`.
- **macOS:** Xcode Command Line Tools.

No other C toolchain is needed; `blake3` is built in pure-Rust mode.

## Layout

```
crates/chem/    engine-independent chemistry core (no Bevy), unit-tested
  elements.rs   periodic table data, categories, radioactivity
  lab.rs        the lab: bonding rules, bond-order solver, add_molecule
  layout.rs     VSEPR-style 3D relaxation (springs + domain repulsion)
  naming.rs     common + IUPAC names; organic.rs: generated organic names
  canon.rs      structure hashing (Weisfeiler–Lehman + aromatic perception)
  compounds.rs  curated compounds, anions, ionic common names (SMILES)
  smiles.rs     minimal SMILES reader
  energy.rs     average bond enthalpies
  reactions.rs  balance checks, exact balancing, known-reaction library
  tests/stress.rs  randomised stress test (300k actions)
src/            Bevy 0.20 front end
  main.rs       app setup, background frame-rate policy
  state.rs      Chem resource: transactional edits, undo/redo, cached names
  scene.rs      mirrors the Lab into entities; isolation for exports
  interact.rs   ray picking, drag & drop, keyboard
  camera.rs     orbit camera (also hosts the egui context)
  ui.rs         sidebar tabs, periodic table, cards, 2D overlays
  menu.rs       ☰ menu, Settings / Help / About windows
  library.rs    Library panel: built-in catalog, My molecules
  document.rs   open file, unsaved changes, Save / Open flows, dialogs
  labfile.rs    .valence file format, validation, atomic save
  export.rs     PNG export (isolate, frame, capture, crop, restore)
  reactions_ui.rs  Reactions tab, 3D arrangement, + / → diagram
  facts.rs      molecule facts for cards and exports
  quality.rs    Full/Lite graphics, staged start-up, pipeline tracking
  gpu.rs        graphics API choice (DX12 first on Windows), GPU probe
  crash.rs      panic hook → crash log, catch_unwind guard
  settings_file.rs  key = value settings file
  debug.rs      demo scene and test/doc tooling (see below)
build.rs        Windows: embeds the icon and version info in the .exe
scripts/        check.sh (macOS/Linux), check.ps1 (Windows)
packaging/      release packaging per platform, app icon source
```

## Chemistry model

- **Valence capacity:** each element has allowed valences (C 4; N 3; O 2;
  P 3/5; S 2/4/6; Cl 1/3/5/7; Xe 0/2/4/6; metals use common oxidation
  states). An atom uses the smallest that fits and expands when needed
  (SO₂ → SO₃, PCl₃ → PCl₅).
- **Bond orders are solved, not chosen:** after each change, a maximum
  b-matching (greedy, then breadth-first augmenting paths) raises bond orders
  to fill open sites.
- **Ionic vs covalent:** alkali/alkaline-earth metals (not Be) or any
  metal–nonmetal pair with ΔEN ≥ 1.7 transfer electrons. Metal + metal is
  rejected.
- **Protonation:** H on a full N/O/P/S with a lone pair makes an -onium ion.
- **Geometry:** bond springs plus repulsion between electron domains (bonds,
  lone pairs, open sites). A lone pair next to a double bond is treated as
  conjugated: it leaves the VSEPR set and sits perpendicular to the plane, so
  amides and caffeine's ring nitrogens are flat.
- **Not modelled:** formal-charge resonance structures, coordination
  complexes, stereochemistry.

### Naming

1. Curated table (~120 SMILES), matched by structure hash, so build order
   doesn't matter and isomers stay distinct.
2. Ionic compounds: cations with Roman numerals plus monatomic or polyatomic
   anions.
3. Binary covalent compounds with Greek prefixes.
4. Generated IUPAC names for acyclic and monocyclic hydrocarbons (including
   benzene) with alkyl, halo and hydroxy groups, ene/yne and -ol.

### Reactions

`reactions.rs` checks element and charge balance and finds the smallest
whole-number coefficients via exact rational null-space computation. It
estimates ΔH from average bond enthalpies (skipped when ionic or unlisted
bonds are involved), and matches inputs against a library of 17 reactions
(with catalysts as formula keys). Library species are normalised through
`Lab::add_molecule`, so ionic species match however they were drawn.

## Stability ("no-crash" design)

- **No panics:** `unwrap`, `expect`, `panic!` and `unreachable!` are denied
  outside tests in both crates (clippy). Searches are bounded, positions are
  NaN-guarded, and the lab is capped at 2000 atoms.
- **Transactional edits:** user actions go through `Chem::edit`, which
  snapshots the lab and runs the change under `catch_unwind`.
  - A rejected action changes nothing.
  - A panic rolls the lab back, and the user sees "that change was undone".
  - Physics and naming are guarded the same way.
- **Crash log:** panics are appended, with a backtrace, to
  `%LOCALAPPDATA%\Valence\crash.log`, `~/Library/Logs/Valence/crash.log` or
  `~/.local/state/valence/crash.log`.
- **Bevy errors become warnings** (`set_error_handler(warn)`). Minimised and
  tiny windows are handled safely.
- **Tests:**
  - `crates/chem/tests/stress.rs` runs 300,000 random actions, including
    garbage input, checking every invariant after each one.
  - Separate tests cover crash rollback and undo/redo.

## Lab data files (`.valence`)

Pretty-printed JSON with a `format: "valence-lab"` / `version` envelope
(`labfile.rs`). It holds the lab (atoms: element, position, valence,
protonation; bonds as index pairs, with orders re-solved on load), reaction
roles keyed by atom index, My molecules (atoms, charges, bond orders) and the
camera. Newer versions than the app understands are refused rather than
half-read.

**Saving** (`labfile::save_atomic`):
1. Write `<name>.saving-<pid>` next to the target and `fsync` it.
2. Read it back; it must match byte for byte and parse.
3. If the target exists, copy it to `<name>.bak.saving`, then rename that to
   `<name>.bak` (the original stays in place throughout).
4. Rename the temporary file over the target (atomic on all three
   platforms; `MoveFileExW` with replace on Windows).
5. `fsync` the directory on Unix so the rename itself is durable.

Any failure removes the temporary file and leaves the original untouched.

**Opening** parses and fully validates first (`LabFile::validate` and
`Lab::from_data`: element numbers, finite positions, allowed valences,
bond indices, duplicates, bonding rules, capacity). Only then is the open lab
replaced, as one undoable `Chem::edit`, and the reactions, My molecules and
camera swapped in.

**Unsaved changes** (`document.rs`) compare the lab revision, a fingerprint
of reactions and My molecules, and a "moved by the user" flag with the last
save. New lab, Open, the example shelf, Quit and the window's close button
(`close_when_requested: false`) all go through the "Save changes?" prompt.
File dialogs are `rfd` async dialogs, polled each frame so the app keeps
drawing; on Linux they use the XDG desktop portal (zenity fallback).

## Rendering notes

- **One camera.** The 3D camera also hosts the egui context. The scene is
  centred between the sidebar and the Library with a `SubCameraView` (an off-centre
  projection), not a viewport, so the UI and 3D view are drawn in one pass.
- **Font sizes are snapped** (`snap_font` in `ui.rs`). egui rebuilds its
  whole glyph atlas when it passes 80% full, and the entire UI disappears for
  that frame. Labels that scale with zoom or rotation must not request new
  font sizes every frame.
- **Labels fade** with how covered their atom is, eased over ~0.1 s, so they
  never pop.
- **Background frame rate:** an unfocused window keeps full rate while
  anything moves (layout settling, camera, effects, exports, start-up), and
  idles otherwise.

## Graphics quality and start-up

- **Full:** bloom, TonyMcMapface tone mapping, translucent halos and lone
  pairs.
- **Lite:** one opaque lit material, no post-processing; ions get UI rings.
- **Auto:** picks Lite for software renderers and Intel integrated GPUs up to
  Gen 9.5 (by PCI device id), Full otherwise. Override with `--lite` /
  `--full` or the Settings window.

Start-up is staged:
1. Only the UI exists until the `egui_pipeline` is compiled.
2. Then the 3D scene is built and post-processing added (Full only).
3. Readiness ignores Bevy's always-on background pipelines (atmosphere,
   SSAO, environment maps).

Depth prepass and deferred lighting are disabled. GPU mesh preprocessing must
stay on: disabling it stops opaque meshes drawing in Bevy 0.20.

Measured launch → interface / 3D ready:

| Machine | Interface | 3D (Lite) | 3D (Full) |
|---|---|---|---|
| Linux, RTX 4060 Ti (Vulkan) | 0.1 s | 0.3 s | 0.4 s |
| macOS, Apple A18 Pro (Metal) | 0.1 s | 0.2 s | 0.2 s |
| Windows, Intel HD 530 (DX12) | 3.0 s | 21 s (Auto picks Lite) | 36 s |

## Windows notes

- **DirectX 12 first:** older Intel drivers crash in their Vulkan driver
  (`igvk64.dll`); Vulkan/GL are fallbacks when no DX12 adapter exists.
- **Shaders use statically linked DXC.** FXC left the window black on older
  GPUs.
- **Overrides:** `WGPU_BACKEND` and `WGPU_DX12_COMPILER` override both
  choices, as does the Settings window's graphics API.
- **Smart App Control** blocks unsigned, unknown binaries. Release builds
  should be signed with a trusted code-signing certificate.

## Checking a platform locally

All building and testing happens on your own machines; there is no hosted CI.

- macOS / Linux: `scripts/check.sh`
- Windows (PowerShell): `.\scripts\check.ps1`

Each script runs:
1. format, clippy (warnings are errors), the full test suite and the `dist`
   build;
2. a **launch test**: renders the demo and verifies both the UI and the 3D
   bonds are visible;
3. a **feature test**: reactions, all three exports, the Settings window, a
   lab data save → wipe → open round trip (including the `.bak` backup) and
   a Library screenshot.

Cross-check other platforms from Linux:

```bash
rustup target add x86_64-pc-windows-msvc aarch64-apple-darwin
cargo check --workspace --all-targets --target x86_64-pc-windows-msvc
cargo check --workspace --all-targets --target aarch64-apple-darwin
```

## Releasing

Everything is built on your own machines; GitHub only hosts the results.
Bump `version` in `Cargo.toml` first. Each script writes to `dist/`.

| Platform | Run on | Command | Output |
|---|---|---|---|
| Linux | Linux with Docker | `packaging/linux/package.sh` | `valence_<v>_amd64.deb`, `Valence-<v>-x86_64.AppImage` |
| macOS | a Mac | `packaging/macos/package.sh` | `Valence-<v>-macos-universal.zip` |
| Windows | Windows | `.\packaging\windows\package.ps1` | `Valence-<v>-windows-x86_64.zip` |

- **Linux** builds inside an Ubuntu 22.04 container (`packaging/linux/Dockerfile`),
  so the packages need only glibc 2.35. `--local` builds on the host instead.
  The AppImage needs `appimagetool` on `PATH` or in `APPIMAGETOOL`.
- **macOS** builds both architectures and joins them with `lipo` into one
  universal `Valence.app`. Set `MACOS_SIGN_IDENTITY` to the Developer ID
  Application identity and `NOTARY_PROFILE` to a `notarytool` keychain
  profile to sign, notarize and staple it; otherwise it's signed ad hoc. Run
  it in a Terminal on the Mac itself: over SSH the login keychain is locked,
  and `codesign` fails with `errSecInternalComponent`.
- **Windows** embeds the icon (`assets/icon/valence.ico`) through `build.rs`.
  The exe isn't code-signed yet.
- **Icons** come from `packaging/icon.py` (pycairo); `packaging/icons.sh`
  regenerates `assets/icon/`.

Then tag and publish:

```bash
git tag v<version> && git push origin v<version>
gh release create v<version> dist/* --title "Valence <version>" --notes-file <notes>
```

## Debug and documentation tooling (`src/debug.rs`)

| Variable / flag | Effect |
|---|---|
| `--demo` | Start with the example shelf |
| `--lite` / `--full` | Force graphics quality |
| `VALENCE_SCREENSHOT=path` | Launch test: screenshot when ready (labels hidden), verify content, exit 0/3/4 |
| `VALENCE_FEATURE_TEST=dir` | Drive reactions, exports, settings, lab data save/open and the Library end to end, then exit |
| `VALENCE_EXPORT_DIR=dir` | Write exports here instead of `Pictures/Valence` |
| `VALENCE_DOC_SHOT=path` | Documentation screenshot with labels, then exit (`VALENCE_TAB`, `VALENCE_SETTINGS=1`) |
| `VALENCE_SHOWCASE=name` | Build one compound (e.g. `caffeine`) and spin it (`VALENCE_SPIN` deg/s) for recordings |
| `VALENCE_BURST=dir` | Save 150 consecutive frames and log egui atlas use, for diagnosing flicker |
| `VALENCE_ZOOM`, `VALENCE_WINDOW`, `VALENCE_FAKE_DRAG` | Camera close-up, window size, simulated drag |

## Performance

- The layout sleeps once it settles: an idle scene does no physics and no
  transform writes.
- Repulsion uses a uniform grid, about 0.4 ms per frame for 1000 atoms
  (`cargo test -p valence_chem --release perf -- --ignored --nocapture`).
- Names and molecules are recomputed only when the lab's revision changes.
- Meshes and materials are shared.
- Only the Bevy features in use are compiled.
