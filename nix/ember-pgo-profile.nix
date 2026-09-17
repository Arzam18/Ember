{
  pkgs,
  lib,
  arch,
}:

# Builds an instrumented static Linux ember, runs a deterministic fixed-depth
# bench workload, and installs the merged LLVM profile. PGO data is
# function-level execution counting over target-independent IR, so the
# same-arch profile is reused by the Linux, Windows, and macOS release
# packages of that architecture.
#
# amd64 runs the workload natively on x86_64 builders; arm64 runs it under
# qemu-aarch64 (counters are exact under user-mode emulation, only wall time
# grows). On non-x86_64 Linux hosts amd64 runs under qemu-x86_64 as well.
let
  target =
    {
      amd64 = "x86_64-unknown-linux-musl";
      arm64 = "aarch64-unknown-linux-musl";
    }
    .${arch} or (throw "unsupported PGO profile architecture: ${arch}");
  targetCpu =
    {
      amd64 = "x86-64-v3";
      arm64 = "generic";
    }
    .${arch};
  nativeSystem = pkgs.stdenv.hostPlatform.system;
  profileSystem =
    {
      amd64 = "x86_64-linux";
      arm64 = "aarch64-linux";
    }
    .${arch};
  emulator =
    if nativeSystem == profileSystem then
      ""
    else
      "${pkgs.qemu-user}/bin/qemu-${if arch == "arm64" then "aarch64" else "x86_64"} ";
  crossPackages =
    {
      amd64 = pkgs.pkgsCross.musl64;
      arm64 = pkgs.pkgsCross.aarch64-multiplatform-musl;
    }
    .${arch};
  crossCc = crossPackages.stdenv.cc;
  linker = "${crossCc}/bin/${crossCc.targetPrefix}cc";
  archiver = "${crossCc}/bin/${crossCc.targetPrefix}ar";
  cargoTargetEnv = lib.toUpper (builtins.replaceStrings [ "-" ] [ "_" ] target);
  rustToolchainConfig = (builtins.fromTOML (builtins.readFile ../rust-toolchain.toml)).toolchain;
  rustToolchain = pkgs.rust-bin.fromRustupToolchain (
    rustToolchainConfig
    // {
      targets = [ target ];
    }
  );
  rustPlatform = pkgs.makeRustPlatform {
    cargo = rustToolchain;
    rustc = rustToolchain;
  };
  version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).package.version;
in
rustPlatform.buildRustPackage {
  pname = "ember-pgo-profile-${arch}";
  inherit version;

  src = import ./ember-source.nix { inherit lib; };
  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [
    crossCc
    pkgs.buildPackages.binutils
    pkgs.llvmPackages.llvm
  ] ++ pkgs.lib.optionals (emulator != "") [ pkgs.qemu-user ];

  buildPhase = ''
    runHook preBuild
    export CARGO_TARGET_${cargoTargetEnv}_LINKER="${linker}"
    export CC_${builtins.replaceStrings [ "-" ] [ "_" ] target}="${linker}"
    export AR_${builtins.replaceStrings [ "-" ] [ "_" ] target}="${archiver}"
    export RUSTFLAGS="-C target-cpu=${targetCpu} -C target-feature=+crt-static -Cprofile-generate=$PWD/profraw"
    mkdir -p profraw
    cargo build --frozen --release --bin ember --target ${target}
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    printf 'bench depth 12\nbench depth 14\nquit\n' \
      | ${emulator}target/${target}/release/ember >/dev/null
    ${pkgs.llvmPackages.llvm}/bin/llvm-profdata merge \
      -o merged.profdata profraw/*.profraw
    test -s merged.profdata || {
      echo "PGO profile merge produced no data" >&2
      exit 1
    }
    mkdir -p "$out"
    cp merged.profdata "$out/merged.profdata"
    runHook postInstall
  '';

  doCheck = false;
  dontFixup = true;

  passthru = {
    inherit arch target;
    workload = "bench depth 12; bench depth 14";
  };

  meta = {
    description = "LLVM PGO profile for Ember ${arch} release builds";
    mainProgram = "ember";
  };
}
