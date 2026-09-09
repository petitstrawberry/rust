use crate::spec::{Target, TargetMetadata, targets};

pub(crate) fn target() -> Target {
    let mut target = targets::riscv64gc_unknown_scarlet::target();
    target.metadata = TargetMetadata {
        description: Some("Scarlet Native RISC-V (RVA23 ISA)".into()),
        tier: Some(3),
        host_tools: Some(false),
        std: Some(true),
    };
    target.options.features = "+rva23u64".into();
    target
}
