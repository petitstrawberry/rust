use crate::spec::{
    Arch, Cc, CodeModel, LinkerFlavor, Lld, Os, PanicStrategy, RelocModel, Target, TargetMetadata,
    TargetOptions,
};

pub(crate) fn target() -> Target {
    Target {
        data_layout: "e-m:e-p:32:32-i64:64-n32-S128".into(),
        metadata: TargetMetadata {
            description: Some("Scarlet Native RISC-V (RV32IMAFDC ISA)".into()),
            tier: Some(3),
            host_tools: Some(false),
            std: Some(true),
        },
        llvm_target: "riscv32-unknown-scarlet".into(),
        pointer_width: 32,
        arch: Arch::RiscV32,
        options: TargetOptions {
            os: Os::Scarlet,
            linker_flavor: LinkerFlavor::Gnu(Cc::No, Lld::Yes),
            linker: Some("rust-lld".into()),
            llvm_abiname: "ilp32d".into(),
            cpu: "generic-rv32".into(),
            max_atomic_width: Some(32),
            features: "+m,+a,+f,+d,+c,+zicsr,+zifencei".into(),
            panic_strategy: PanicStrategy::Abort,
            main_needs_argc_argv: true,
            // Scarlet uses OS-level TLS instead of ELF native TLS blocks.
            has_thread_local: false,
            relocation_model: RelocModel::Static,
            code_model: Some(CodeModel::Medium),
            emit_debug_gdb_scripts: false,
            eh_frame_header: false,
            ..Default::default()
        },
    }
}
