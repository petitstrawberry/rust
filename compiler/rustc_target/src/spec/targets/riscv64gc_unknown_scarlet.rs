use crate::spec::{
    Arch, Cc, CodeModel, LinkerFlavor, Lld, Os, PanicStrategy, RelocModel, SanitizerSet, Target,
    TargetMetadata, TargetOptions,
};

pub(crate) fn target() -> Target {
    Target {
        data_layout: "e-m:e-p:64:64-i64:64-i128:128-n32:64-S128".into(),
        metadata: TargetMetadata {
            description: Some("Scarlet Native RISC-V (RV64IMAFDC ISA)".into()),
            tier: Some(3),
            host_tools: Some(false),
            std: Some(true),
        },
        llvm_target: "riscv64-unknown-scarlet".into(),
        pointer_width: 64,
        arch: Arch::RiscV64,

        options: TargetOptions {
            os: Os::Scarlet,
            linker_flavor: LinkerFlavor::Gnu(Cc::No, Lld::Yes),
            linker: Some("rust-lld".into()),
            llvm_abiname: "lp64d".into(),
            cpu: "generic-rv64".into(),
            max_atomic_width: Some(64),
            features: "+m,+a,+f,+d,+c,+zicsr,+zifencei".into(),
            panic_strategy: PanicStrategy::Abort,
            main_needs_argc_argv: true,
            relocation_model: RelocModel::Static,
            code_model: Some(CodeModel::Medium),
            emit_debug_gdb_scripts: false,
            eh_frame_header: false,
            supported_sanitizers: SanitizerSet::KERNELADDRESS | SanitizerSet::SHADOWCALLSTACK,
            ..Default::default()
        },
    }
}
