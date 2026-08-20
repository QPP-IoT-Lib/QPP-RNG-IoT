# Test harness environment setup

How to stand up everything `test-harness/*` and `xtask` need, on a fresh Linux, macOS, or Windows machine. Two tiers:

- **Tier A -- core (required):** Rust + this workspace. Enough to build everything, run Tier 1 stats, bench,
  differential, and footprint's cycle-count track.
- **Tier B -- external tools (optional):** `ent`, the NIST SP 800-90B reference tool, NIST SP 800-22 STS, `cargo-bloat`,
  `cargo-size`,
  `cargo-call-stack`, `probe-rs`, QEMU. Each one the harness's
  `find_tool()`-style detection treats as "N/A" when absent -- nothing breaks without them, you just don't get that
  track's numbers.

**Verification status**: everything under macOS below was built and run for real on this machine (Apple Silicon) over
the course of getting this harness working. See the "Known gotchas" section, which exists specifically because several
of these steps failed in non-obvious ways on the first attempt. Linux and Windows instructions are accurate to each
ecosystem's normal packaging (Debian/Ubuntu `apt`, MSYS2 `pacman`, etc.) but were not built end-to-end in this session,
so treat them as a strong starting point, not a guarantee, the same way the harness's own tier2 parsers are documented
as best-effort until checked against a real run.

---

## Tier A: Rust + the workspace (all platforms)

1. Install Rust via [rustup](https://rustup.rs):
    - Linux/macOS: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
    - Windows: download and run [rustup-init.exe](https://win.rustup.rs), or `winget install Rustlang.Rustup`
2. Clone the repo and build:
   ```bash
   git clone <repo-url> qpp-rng
   cd qpp-rng
   cargo build --workspace
   cargo test --workspace
   ```
3. Sanity-check `xtask` (works out of the box via the `.cargo/config.toml` alias):
   ```bash
   cargo xtask targets
   cargo xtask compare --dry-run
   ```

That's it for Tier A. Nothing above needs any of the external tools. Windows note: use PowerShell or `cmd`, not WSL, if
you want a *native*
Windows Rust toolchain; `cargo xtask` works identically there since it's plain Rust.

---

## Tier B: external tools, per platform

### macOS (verified)

```bash
# Packaged tools
brew install ent qemu
cargo install cargo-bloat
cargo install cargo-binutils --locked
rustup component add llvm-tools-preview
rustup toolchain install nightly
cargo +nightly install cargo-call-stack
cargo install probe-rs-tools --locked
```

**NIST SP 800-90B reference tool** (`ea_iid`/`ea_non_iid`). No package, build from source. The Makefile's default
`g++`/`-fopenmp` combination does **not** work here (see gotcha #1 below). This is the verified working command:

```bash
brew install libdivsufsort jsoncpp bzip2 openssl gmp mpfr libomp
git clone https://github.com/usnistgov/SP800-90B_EntropyAssessment.git
cd SP800-90B_EntropyAssessment/cpp
JSONCPP=$(brew --prefix jsoncpp) DIVSUF=$(brew --prefix libdivsufsort) SSL=$(brew --prefix openssl@3) \
GMP=$(brew --prefix gmp) MPFR=$(brew --prefix mpfr) BZ2=$(brew --prefix bzip2) OMP=$(brew --prefix libomp)
make CXX=clang++ \
  CXXFLAGS="-std=c++11 -Xpreprocessor -fopenmp -O2 -ffloat-store -I${JSONCPP}/include -I${DIVSUF}/include -I${SSL}/include -I${GMP}/include -I${MPFR}/include -I${BZ2}/include -I${OMP}/include -L${JSONCPP}/lib -L${DIVSUF}/lib -L${SSL}/lib -L${GMP}/lib -L${MPFR}/lib -L${BZ2}/lib -L${OMP}/lib -lomp" \
  iid non_iid
```

Produces `ea_iid`/`ea_non_iid` in the current directory. Put them somewhere on `PATH` (see "Making tools visible to the
harness" below).

**NIST SP 800-22 STS** (`assess`). Also build from source, but a plain C project, so no ABI-mismatch trouble:

```bash
curl -f -L -o sts-2.1.2.zip "https://csrc.nist.gov/CSRC/media/Projects/Random-Bit-Generation/documents/sts-2_1_2.zip"
unzip sts-2.1.2.zip
cd sts-2.1.2/sts-2.1.2
make
```

Produces `assess` in the current directory. **You do not need to run
`assess` interactively yourself**. `stats::tier2::run_sp800_22`
already drives its menu prompts and reads its report file automatically. Just get the binary onto `PATH`.

### Linux (Debian/Ubuntu; adjust package manager for other distros)

```bash
sudo apt update
sudo apt install -y ent qemu-system-arm build-essential libbz2-dev \
  libjsoncpp-dev libdivsufsort-dev libgmp-dev libmpfr-dev libssl-dev unzip
cargo install cargo-bloat
cargo install cargo-binutils --locked
rustup component add llvm-tools-preview
rustup toolchain install nightly
cargo +nightly install cargo-call-stack
cargo install probe-rs-tools --locked
```

`ent` and `qemu-system-arm` are both real Debian/Ubuntu packages (unlike macOS, where only `ent` is in Homebrew and QEMU
needs `brew install
qemu` separately for the same effect).

**NIST SP 800-90B tool**: on Linux this should build with the Makefile essentially as-is -- `apt`'s `g++`,
`libjsoncpp-dev`, `libdivsufsort-dev`, etc. are all built against the same system `libstdc++`, so the Clang/`libc++` vs.
GCC/`libstdc++` split that broke the naive build on macOS doesn't apply here:

```bash
git clone https://github.com/usnistgov/SP800-90B_EntropyAssessment.git
cd SP800-90B_EntropyAssessment/cpp
make CXXFLAGS="-std=c++11 -fopenmp -O2 -ffloat-store -I/usr/include/jsoncpp" iid non_iid
```

(Dropping `-march=native` is usually unnecessary on Linux since GCC supports it natively on both x86 and arm64, but pass
`ARCH=` to disable it if you hit an "unsupported architecture" error on an unusual chip.)

**NIST SP 800-22 STS**: identical to macOS --

```bash
curl -f -L -o sts-2.1.2.zip "https://csrc.nist.gov/CSRC/media/Projects/Random-Bit-Generation/documents/sts-2_1_2.zip"
unzip sts-2.1.2.zip && cd sts-2.1.2/sts-2.1.2 && make
```

### Windows

Native Windows support is genuinely uneven across these tools -- most of the friction is the same C/C++ ecosystem gap
that made the macOS build non-trivial, just more so (no Homebrew-equivalent default package manager most people already
have). **Recommended path: WSL2**, then just follow the Linux instructions above verbatim inside it:

```powershell
wsl --install -d Ubuntu
```

Everything -- `cargo build`, `cargo test`, `cargo xtask`, the NIST tools -- works identically inside WSL2's Linux
userspace, and this is the path most likely to just work on the first try.

**If you want a fully native Windows setup instead:**

- Rust tooling (`cargo-bloat`, `cargo-binutils`, `cargo-call-stack`,
  `probe-rs-tools`) installs the same way via `cargo install` -- these are plain Rust crates, platform-independent:
  ```powershell
  cargo install cargo-bloat
  cargo install cargo-binutils --locked
  rustup component add llvm-tools-preview
  rustup toolchain install nightly
  cargo +nightly install cargo-call-stack
  cargo install probe-rs-tools --locked
  ```
- `ent`: Fourmilab ships a prebuilt Windows binary directly on
  [fourmilab.ch/random](https://www.fourmilab.ch/random/) -- no build needed.
- QEMU: `winget install SoftwareFreedomConservancy.QEMU` (or the installer
  from [qemu.org](https://www.qemu.org/download/#windows)).
- NIST SP 800-90B tool and STS: install
  [MSYS2](https://www.msys2.org), then from an **MSYS2 MinGW64 shell**
  (not the plain MSYS shell -- you want the `mingw-w64-x86_64-*`
  toolchain, which is internally consistent the same way apt's packages are, avoiding the macOS ABI trap):
  ```bash
  pacman -S mingw-w64-x86_64-gcc mingw-w64-x86_64-jsoncpp \
    mingw-w64-x86_64-libdivsufsort mingw-w64-x86_64-gmp \
    mingw-w64-x86_64-mpfr mingw-w64-x86_64-openssl mingw-w64-x86_64-make unzip
  ```
  then the same `make`/build steps as the Linux section, run from that MinGW64 shell. This is the one part of this whole
  document that's genuinely unverified rather than just "verified on a different OS but should transfer" -- if `pacman`
  's package names have drifted or the Makefile needs adjusting, that's the likeliest friction point.

---

## Making tools visible to the harness

Every Tier B tool is found via a `PATH` scan (`stats::tier2::find_tool` / `footprint::toolshell::find_tool`) at the
moment the harness actually runs -- there's no config file to edit. Whatever directory you put a built binary in just
needs to be on
`PATH` for the process that runs `cargo test`/`cargo xtask compare`.

- **Linux**: add the directory to `~/.bashrc`/`~/.zshrc` (`export
  PATH="$PATH:/path/to/tools"`) and open a new terminal, or `source` the file.
- **Windows**: `setx PATH "%PATH%;C:\path\to\tools"` (persists across new terminals), or add it via *System Properties >
  Environment Variables*.
- **macOS**: same idea, but has one extra wrinkle worth knowing before it costs you a debugging session -- see gotcha #4
  below.

Verify with the harness itself rather than trusting `which` alone, since the harness's own process might not inherit the
same environment your interactive shell does (see gotcha #4):

```bash
cargo run -p stats --bin stats-cli --release -- generate-samples --dir /tmp/qpp-samples --bytes 1000000
cargo run -p stats --bin stats-cli --release -- full --dir /tmp/qpp-samples --out /tmp/stats-report.json --sts-work-dir /tmp/qpp-sts-work
```

If a tool's JSON output shows `"parsed": null` with `"tool_path": null`, the harness didn't find it on `PATH` at all.
`"tool_path"` set but
`"parsed": null` means it ran but didn't produce output this crate's parser recognized -- worth reporting, since every
parser here is documented as best-effort pending exactly that kind of real-world check.

---

## Known gotchas (found and fixed this way for a reason)

These cost real debugging time to track down, so they're recorded here rather than left to be rediscovered:

1. **macOS: SP 800-90B tool fails to link with `g++`/GCC.** Homebrew's
   `jsoncpp`/`gmp`/`mpfr`/etc. are built against Apple Clang's `libc++`; the Makefile's default `CXX = g++` (which
   resolves to a real GCC via Homebrew, using `libstdc++`) produces "symbol not found" linker errors from mixing the two
   C++ ABIs. Fix: build with `CXX=clang++`
   and Homebrew's `libomp` for OpenMP support instead (the exact command is in the macOS section above).

2. **NIST STS's `assess` needs 6 stdin answers, not the commonly-cited 4.** `fixParameters()` inserts a "Select Test (0
   to continue)"
   parameter-customization prompt right after "apply all tests," and there's a final ASCII-vs-Binary input-mode prompt
   at the very end. Getting this wrong doesn't error -- `assess` just blocks forever on the next unanswered prompt,
   indistinguishable from "still computing"
   from the outside. Not something you need to handle by hand though:
   `stats::tier2::run_sp800_22` already drives the correct sequence.

3. **`assess` truncates any input path containing a space** (it reads the filename with C's `scanf("%s", ...)`, which
   stops at the first whitespace -- no quoting can fix this, it's a limitation in the tool itself). If your project
   lives under a path like `.../My Documents/...`, this bites every run. `run_sp800_22` already works around it by
   always copying the sample to `std::env::temp_dir()` (guaranteed space-free) before invoking `assess` -- but if you
   ever shell out to
   `assess` directly yourself, remember this.

4. **macOS: a tool installed via `/etc/paths.d/<name>` is only visible to *login* shells**, via `path_helper`, which
   only `/etc/zprofile`
   (sourced on login shells) invokes. A brand-new Terminal.app window is a login shell, so it works there -- but an
   IDE's Run/Debug button often inherits the IDE process's own environment (which, if the IDE was opened from
   Finder/Dock rather than a terminal, was never a login shell either, and macOS's per-session `launchctl` environment
   is nearly empty). If tools resolve fine in a terminal but the IDE can't find them, this is almost always why -- fix
   it by setting `PATH`
   explicitly in the IDE's own run-configuration environment variables rather than relying on shell-login semantics.

5. **`assess` exits with status 1 even on a fully successful run.** Its
   `main()` just doesn't return `0`. `stats::tier2::ToolRun::exit_success`
   being `false` for STS specifically is not a failure signal --
   `StatReport::overall_pass()` already keys off whether a result actually *parsed*, not the exit code, for exactly this
   reason.

6. **A `.zip` that fails to unzip with "unsupported format" may just be empty.** `curl -LO` doesn't fail on an HTTP
   error by default -- a dead/moved URL can silently produce a 0-byte file with the right name. `file <path>` (should
   say `Zip archive data...`, not `empty`)
   and `curl -sIL <url>` (check for `HTTP/2 200`, not `404`) are worth checking before assuming the archive itself is
   corrupt. The STS download URL in this document (`csrc.nist.gov/CSRC/media/...`) is the one that actually resolves as
   of this writing; NIST's site has moved this file before and may again.
