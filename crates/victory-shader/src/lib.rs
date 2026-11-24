//! Xbox Series shaders to host shaders.
//!
//! An Xbox Series shader is a standard DXBC container ([`dxbc`]). Its `DXIL`
//! part ([`dxil`]) may carry Xbox additions around the LLVM bitcode: an
//! `XDAT` table before it indexing blobs after it, among them `xbop`
//! (compiler options) and the zlib-compressed GFX10 code. The bitcode may use
//! console-only intrinsics, so the GFX10 code is translated in general.

pub mod bvh;
pub mod dxbc;
pub mod dxil;
pub mod emit;
pub mod gfx10;
pub mod io;
pub mod ir;
pub mod native;
pub mod resource;
pub mod spill;
pub mod structure;

pub use dxbc::{Container, FourCC, Part};
pub use dxil::{DxilProgram, ShaderKind, XboxExtras};
pub use native::{
    HwStage, NativeProgram, ResourceType, RootParam, RootType, UsedResource, native_programs,
};

/// Exports `D3D12SDKVersion`/`D3D12SDKPath` from this crate's own test
/// binary (see `build.rs`), so the `host`-feature GPU tests under
/// `emit::{valu,intersect,waterfall,wave64}_tests` and `emit::diff_tests`
/// can get shader model 6.6 from a redistributed DirectX 12 Agility SDK
/// core, rather than whatever the OS's `d3d12.dll` happens to support.
/// `#[cfg(test)]`-gated so these never end up in the rlib apps/victory
/// links against (which exports the same symbols itself, from its own exe).
#[cfg(test)]
mod agility_export {
    #[repr(transparent)]
    pub struct ReadOnlyCStr(*const u8);
    // SAFETY: points at a 'static byte-string literal; never written to.
    unsafe impl Sync for ReadOnlyCStr {}

    #[unsafe(no_mangle)]
    pub static D3D12SDKVersion: u32 = 620;
    #[unsafe(no_mangle)]
    pub static D3D12SDKPath: ReadOnlyCStr = ReadOnlyCStr(c".\\D3D12\\".as_ptr().cast());
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ShaderError {
    #[error("not a DXBC container")]
    NotDxbc,
    #[error("malformed shader: {0}")]
    Malformed(&'static str),
    #[error("GFX10 code at {offset:#x}: {reason}")]
    Isa { offset: usize, reason: &'static str },
    /// Valid code the translator cannot handle yet: an opcode name or a construct.
    #[error("GFX10 code at {offset:#x}: {what} is not supported")]
    Unsupported { offset: usize, what: &'static str },
}
