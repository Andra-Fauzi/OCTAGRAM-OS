// OCTAGRAM-OS userspace target (`x86_64-unknown-octagram`).
//
// This is the *host* triple of the native toolchain: rustc and cargo are built
// for it so they run as OCTAGRAM processes. Keep it in sync with
// user/rust/targets/x86_64-unknown-octagram.json, which is the JSON spec used
// for cross builds from Linux (`-Z build-std`).
//
// ABI facts this spec encodes:
//   * static ELF `ET_EXEC`, base 0x400000, `user/linker.ld`, no dynamic loader;
//   * TLS is local-exec/initial-exec only, no TLS in shared objects;
//   * linker is the GCC driver in user/toolchain/bin, which adds `crt0.o`,
//     `libc.a`, `libc++.a` and the linker script. That driver only exists on
//     a host, so builds inside OCTAGRAM override it (`cargo` passes
//     `-C linker=tcc`, or `$OCTAGRAM_RUST_LINKER`);
//   * panic strategy abort, no red zone, SSE/SSE2 only.

use crate::spec::{
    Arch, Cc, CodeModel, FramePointer, LinkerFlavor, Lld, Os, PanicStrategy, RelocModel, RelroLevel,
    Target, TargetMetadata, TargetOptions, TlsModel, cvs,
};

pub(crate) fn target() -> Target {
    Target {
        // LLVM only needs the x86-64 bare-metal triple: OCTAGRAM adds no
        // codegen-relevant distinction, and the JSON spec pins the same value.
        llvm_target: "x86_64-unknown-none".into(),
        metadata: TargetMetadata {
            description: Some("OCTAGRAM-OS userspace (static, own libc)".into()),
            tier: Some(3),
            // The whole point of this target is running host tools on it.
            host_tools: Some(true),
            std: Some(true),
        },
        pointer_width: 64,
        data_layout:
            "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128".into(),
        arch: Arch::X86_64,
        options: TargetOptions {
            os: Os::Octagram,
            cpu: "x86-64".into(),
            features: "+sse,+sse2".into(),
            max_atomic_width: Some(64),
            // `target_family = "unix"` selects the generic Unix PAL in
            // library/std/src/sys/pal, matching library/std/build.rs.
            families: cvs!["unix"],
            dynamic_linking: false,
            position_independent_executables: false,
            static_position_independent_executables: false,
            crt_static_default: true,
            crt_static_respected: true,
            relro_level: RelroLevel::None,
            relocation_model: RelocModel::Static,
            code_model: Some(CodeModel::Small),
            disable_redzone: true,
            frame_pointer: FramePointer::Always,
            has_thread_local: true,
            tls_model: TlsModel::InitialExec,
            panic_strategy: PanicStrategy::Abort,
            eh_frame_header: false,
            linker: Some("x86_64-octagram-gcc".into()),
            linker_flavor: LinkerFlavor::Gnu(Cc::Yes, Lld::No),
            ..Default::default()
        },
    }
}
