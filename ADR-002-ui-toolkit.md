# ADR-002: UI toolkit

**Status:** Accepted — Tauri v2. Supersedes the one-line dismissal of Flutter in
`RESEARCH.md` §4, which was under-researched.

**Decision is coupled to the no-mobile-app decision (`CRITIQUE.md` §3.5).** If
that reverses, this one should too. The frontend framework is a separate
decision, in `ADR-003`.

---

## The challenge

`RESEARCH.md` dismissed Flutter in a single line: *"Dart crypto story is weak
and you'd FFI to Rust anyway — so you get Tauri's seam without Tauri's security
model."*

That was lazy in two ways. The FFI claim is now false, and my main argument for
Tauri over native toolkits — *"Signal-grade polish is 5–10× the effort in
immediate-mode Rust GUI"* — **does not apply to Flutter at all.** Flutter is a
mature retained-mode toolkit that is specifically good at beautiful UIs. I used
an argument about `egui` to dismiss something that isn't `egui`.

## What I got wrong

**1. `flutter_rust_bridge` is mature.** v2.13.0, 6.7M downloads, an official
*Flutter Favorite*, supporting async Rust, arbitrary non-serialisable types,
traits, and Rust-calls-Dart. It auto-compiles Rust as part of the Flutter build.
The seam I hand-waved as a dealbreaker is well-trodden.

**2. Canonical now leads Flutter Desktop.** Announced at Google I/O 2026:
Canonical is "lead maintainer and Strategic Steward" for the Linux, Windows and
macOS embedders, and owns the desktop roadmap. Canonical ships real Flutter apps
in Ubuntu (App Center, Firmware Updater, Security Center). This specifically
strengthens Linux, which was the weakest target — and Linux is exactly where
Tauri is weakest.

**3. Impeller is default on desktop** as of Flutter 3.47 — Metal on macOS,
Vulkan on Windows and Linux, shaders precompiled. Multi-window support (dialogs,
tooltips, popups) landed in 2026, which had been a genuine blocker.

**4. There are production KDBX password managers in Flutter.** AuthPass ships on
the Mac App Store, Microsoft Store, Snap and Flathub. And `keepass_flutter`
exists — a Flutter plugin that **bundles a Rust KDBX core via cargokit**. That
is almost exactly the architecture I'd be building. The approach is proven.

**5. No WebView is a real security argument.** Flutter draws with Impeller; there
is no browser engine, no JS runtime, no DOM, no JIT in the UI layer. For a
password manager that removes an entire category of attack surface. I gave this
one clause; it deserved more.

## Why I still choose Tauri

### 1. Mobile is out of scope, and that is Flutter's whole advantage

Every honest source says the same thing: Flutter Desktop's real value is **code
sharing with an existing Flutter mobile app** (70–85% reuse). One 2026 audit puts
it plainly — *"the code sharing is the real value, not the individual desktop
runtime."*

We decided in `CRITIQUE.md` §3.5 that there will be no mobile app, because
KeePassium already is one. So we'd be paying Flutter's costs for a benefit we
have declared out of scope.

I re-examined that decision rather than assuming it. It holds: the reason for no
mobile isn't that the UI is hard, it's App Store review, mobile autofill
integration, and the platform-compliance treadmill that Buttercup's maintainer
named as what killed him — *"requirements for software, especially security
software, that requires near constant attention."* A free UI layer does not
address any of that.

**This is a scope argument, not a technology argument, and it's the decisive one.**

### 2. A password manager is nothing but text fields — and that's Flutter's weak spot

This isn't peripheral for us, it's the entire product surface. From Flutter's
own tracker, all open or recently open in 2026:

- **Windows still uses IMM32, not TSF** (`text_input_manager.cc`). Microsoft has
  formally deprecated IMM32. TSF is how modern Windows routes speech, handwriting
  and IME input — so `Win+H` voice typing has nowhere to write. (#182876)
- **Korean IME places the cursor before the composing character**, on every
  Flutter version ever tested. Reporter's words: *"a fundamental problem with
  Flutter Desktop's IME handling architecture."* (#172270)
- **IME composition leaks between text fields** and stays corrupted until app
  restart. (#191196)
- **External dictation and accessibility tools cannot find Flutter text fields**
  on Windows *or* macOS, because UIA/AXUIElement editable roles aren't properly
  exposed. Works in native apps and in Electron; not in Flutter. (#182876, #191503)

A vault where CJK users can't type reliably, and where assistive tech can't see
the password field, is not shippable — and these are architectural, not a bad
release.

### 3. Flutter apps look like Flutter apps

The stated goal is "Signal Desktop, but a password manager." Flutter ships
Material and Cupertino; a bespoke look means fighting the framework's grain. HTML
and CSS have unlimited design freedom and the largest design-system ecosystem in
existence — and it is literally the medium Signal Desktop is built in.

On the user's *primary* criterion, web tech is the shorter path.

### 4. The project is ~90% Rust

`vardo-kdbx` is 4,259 lines of Rust and the merge engine will be larger. Tauri
makes that the host language with typed commands and no codegen step. Flutter
makes Rust a guest reached through generated bindings. FRB is good, but it's
still a seam, a build-integration story, and a second toolchain in CI.

## The honest costs of this decision

- **WebKitGTK on Linux** is materially behind WebView2/WKWebView, and Canonical
  just made Flutter's Linux story *better* than Tauri's. This is the place we're
  actively choosing the worse option.
- **We keep a browser engine in the process.** Mitigated architecturally — the
  WebView never holds secrets in bulk, so a compromise leaks entry titles rather
  than the vault — but mitigated is not eliminated. Flutter would eliminate it.
- **Bundle and memory.** Tauri wins here (single-digit MB vs ~100MB; ~50MB vs
  ~200–380MB idle) but I'm deliberately *not* leaning on this. The tidiest
  numbers I found come from a content farm that also published a fabricated
  "Tauri CVE exfiltrated 127k records" incident report. Treat the specific
  figures as unverified; the direction is right, the magnitude is marketing.

## What would flip this

1. **Mobile enters scope.** Then Flutter wins decisively and it isn't close.
2. **WebKitGTK becomes intolerable on Linux.** The architecture keeps `ui/` a
   thin presentation layer over typed commands precisely so a different frontend
   can be swapped against the same `vardo-core`.
3. **A Flutter release fixes TSF on Windows and desktop accessibility.** Then
   objection 2 evaporates and this becomes a genuine coin-flip decided by taste.

## Options considered and rejected

| Option | Why not |
|---|---|
| **Electron** | The realistic counterfactual, and strictly worse than Tauri here: bundled Chromium plus a Node main process with full system access by default. Signal Desktop uses it, which is the only argument for it. |
| **Compose Multiplatform** | Genuinely strong on desktop — JetBrains ships their IDEs on it. But it's a JVM runtime, a third language, and the same "drawn, not native" tradeoffs as Flutter without Flutter's ecosystem. |
| **Qt** | What KeePassXC uses. Mature, native, excellent accessibility. Also what makes KeePassXC look like 2009, and the licensing story for a permissive project is a headache. |
| **Slint** | Rust-native, no WebView, designed for UI — the most tempting alternative. Rejected on ecosystem size and licensing (GPL / commercial / royalty-free-with-conditions is a poor fit for Apache-2.0 OR MIT). |
| **egui / iced** | The toolkits my original argument was actually about. Correct to reject for a design-led product. |

## Verdict

Tauri v2, held — but on rebuilt reasoning, and closer than `RESEARCH.md`
implied. Flutter is a legitimate choice that I dismissed too fast; it loses here
mainly because **we aren't building a mobile app**, and secondarily because a
password manager is a text-input-heavy product and that is precisely where
Flutter Desktop is weakest today.

If the mobile decision ever changes, reopen this immediately.
