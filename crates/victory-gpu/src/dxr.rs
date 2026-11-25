//! Vendor-neutral native DXR experiment. Never changes title rendering.
//! The optional probe asks the actual driver for native BLAS/TLAS build
//! sizes. It does not submit ray work or interpret console BVH addresses.

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::Interface;

fn tier(device: &ID3D12Device) -> D3D12_RAYTRACING_TIER {
    let mut options = D3D12_FEATURE_DATA_D3D12_OPTIONS5::default();
    // SAFETY: OPTIONS5 writes precisely the supplied structure.
    if unsafe {
        device.CheckFeatureSupport(
            D3D12_FEATURE_D3D12_OPTIONS5,
            (&raw mut options).cast(),
            std::mem::size_of_val(&options) as u32,
        )
    }.is_ok() {
        options.RaytracingTier
    } else {
        D3D12_RAYTRACING_TIER_NOT_SUPPORTED
    }
}

fn inline_supported(tier: D3D12_RAYTRACING_TIER) -> bool {
    tier.0 >= D3D12_RAYTRACING_TIER_1_1.0
}

/// Synthetic unindexed triangle geometry. No title memory involved.
fn triangle(vertex_address: u64) -> D3D12_RAYTRACING_GEOMETRY_DESC {
    D3D12_RAYTRACING_GEOMETRY_DESC {
        Type: D3D12_RAYTRACING_GEOMETRY_TYPE_TRIANGLES,
        Flags: D3D12_RAYTRACING_GEOMETRY_FLAG_OPAQUE,
        Anonymous: D3D12_RAYTRACING_GEOMETRY_DESC_0 {
            Triangles: D3D12_RAYTRACING_GEOMETRY_TRIANGLES_DESC {
                VertexFormat: DXGI_FORMAT_R32G32B32_FLOAT,
                IndexFormat: DXGI_FORMAT_UNKNOWN,
                VertexCount: 3,
                VertexBuffer: D3D12_GPU_VIRTUAL_ADDRESS_AND_STRIDE {
                    StartAddress: vertex_address,
                    StrideInBytes: 12,
                },
                ..Default::default()
            },
        },
    }
}

fn prebuild(device: &ID3D12Device5) -> Option<(u64, u64, u64, u64)> {
    // A size query does not dereference GPU addresses. Geometry descriptors
    // themselves remain alive throughout the synchronous call.
    let geometry = triangle(0);
    let blas = D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS {
        Type: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_TYPE_BOTTOM_LEVEL,
        Flags: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_BUILD_FLAG_PREFER_FAST_TRACE,
        NumDescs: 1,
        DescsLayout: D3D12_ELEMENTS_LAYOUT_ARRAY,
        Anonymous: D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS_0 {
            pGeometryDescs: &geometry,
        },
    };
    let tlas = D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS {
        Type: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_TYPE_TOP_LEVEL,
        Flags: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_BUILD_FLAG_PREFER_FAST_TRACE,
        NumDescs: 1,
        DescsLayout: D3D12_ELEMENTS_LAYOUT_ARRAY,
        Anonymous: D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS_0 {
            InstanceDescs: 0,
        },
    };
    let mut bottom = D3D12_RAYTRACING_ACCELERATION_STRUCTURE_PREBUILD_INFO::default();
    let mut top = D3D12_RAYTRACING_ACCELERATION_STRUCTURE_PREBUILD_INFO::default();
    // SAFETY: valid CPU descriptors and output structures for a size query.
    unsafe {
        device.GetRaytracingAccelerationStructurePrebuildInfo(&blas, &mut bottom);
        device.GetRaytracingAccelerationStructurePrebuildInfo(&tlas, &mut top);
    }
    let sizes = (bottom.ResultDataMaxSizeInBytes, bottom.ScratchDataSizeInBytes,
                 top.ResultDataMaxSizeInBytes, top.ScratchDataSizeInBytes);
    (sizes.0 > 0 && sizes.1 > 0 && sizes.2 > 0 && sizes.3 > 0).then_some(sizes)
}

pub(crate) fn report(device: &ID3D12Device) {
    let tier = tier(device);
    let inline = inline_supported(tier);
    tracing::info!(tier = tier.0, inline, "host DXR capability (existing renderer unchanged)");
    if !std::env::var_os("VICTORY_DXR_PROBE").is_some_and(|v| v == "1") {
        return;
    }
    if !inline {
        tracing::info!("native DXR 1.1 unavailable; existing compute path retained");
        return;
    }
    let sizes = device.cast::<ID3D12Device5>().ok().and_then(|d| prebuild(&d));
    match sizes {
        Some((blas_result, blas_scratch, tlas_result, tlas_scratch)) => {
            tracing::info!(blas_result, blas_scratch, tlas_result, tlas_scratch,
                "native DXR synthetic prebuild size probe passed");
        }
        None => tracing::warn!("native DXR synthetic prebuild size probe failed; existing renderer retained"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_requires_tier_1_1() {
        assert!(!inline_supported(D3D12_RAYTRACING_TIER_NOT_SUPPORTED));
        assert!(!inline_supported(D3D12_RAYTRACING_TIER_1_0));
        assert!(inline_supported(D3D12_RAYTRACING_TIER_1_1));
    }

    #[test]
    fn synthetic_triangle_descriptor_is_unindexed() {
        let g = triangle(0x1000);
        assert_eq!(g.Type, D3D12_RAYTRACING_GEOMETRY_TYPE_TRIANGLES);
        // SAFETY: triangle() selected this union member.
        let t = unsafe { g.Anonymous.Triangles };
        assert_eq!(t.VertexCount, 3);
        assert_eq!(t.IndexCount, 0);
        assert_eq!(t.VertexBuffer.StartAddress, 0x1000);
        assert_eq!(t.VertexBuffer.StrideInBytes, 12);
    }
}
