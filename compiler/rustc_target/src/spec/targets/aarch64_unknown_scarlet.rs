use crate::spec::{
    Arch, Cc, LinkerFlavor, Lld, Os, PanicStrategy, RelocModel, SanitizerSet, StackProbeType,
    Target, TargetMetadata, TargetOptions,
};

pub(crate) fn target() -> Target {
    let opts = TargetOptions {
        os: Os::Scarlet,
        linker_flavor: LinkerFlavor::Gnu(Cc::No, Lld::Yes),
        linker: Some("rust-lld".into()),
        pre_link_args: TargetOptions::link_args(
            LinkerFlavor::Gnu(Cc::No, Lld::No),
            &["--fix-cortex-a53-843419"],
        ),
        features: "+v8a,+strict-align,+neon".into(),
        supported_sanitizers: SanitizerSet::KCFI | SanitizerSet::KERNELADDRESS,
        relocation_model: RelocModel::Static,
        disable_redzone: true,
        max_atomic_width: Some(128),
        stack_probes: StackProbeType::Inline,
        panic_strategy: PanicStrategy::Abort,
        main_needs_argc_argv: true,
        default_uwtable: true,
        ..Default::default()
    };

    Target {
        llvm_target: "aarch64-unknown-scarlet".into(),
        metadata: TargetMetadata {
            description: Some("Scarlet Native AArch64 (ARMv8-A ISA)".into()),
            tier: Some(3),
            host_tools: Some(false),
            std: Some(true),
        },
        pointer_width: 64,
        data_layout: "e-m:e-p270:32:32-p271:32:32-p272:64:64-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32".into(),
        arch: Arch::AArch64,
        options: opts,
    }
}
