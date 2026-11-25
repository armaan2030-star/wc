//! An exact full-tile entry for the audited transmission prefilter.

use super::{LOCAL_WAVES, Lower, NEEDS, Parts, Stop, host_max_wave, unsupported};
use crate::emit::{container, group_sync};
use crate::gfx10::Fields;
use crate::ir::{Inst, Reg};
use crate::native::{
    HwStage, NativeProgram, ResourceType, RootParam, RootType, StageIo, UsedResource,
};
use std::collections::HashMap;

use super::super::bitcode::{Attr, Bin, Term, V, pred};

const DIGEST: [u8; 16] = [
    0x81, 0x36, 0xb5, 0x2c, 0xba, 0xf2, 0xa2, 0x66, 0x36, 0xe4, 0x9a, 0x5c, 0x6b, 0xc8, 0x0e, 0x03,
];
const READLANES: [usize; 5] = [0x8d0, 0x8e0, 0x8e8, 0x8f0, 0x8f8];

#[derive(Default)]
pub(super) struct Witness {
    pub(super) counter_ready: Option<V>,
    gate: Option<V>,
    exchanges: [Option<[V; 2]>; 5],
    native_barrier: Option<V>,
}

impl Witness {
    pub(super) fn exchange(&mut self, offset: usize, before: V, after: V) {
        if let Some(i) = READLANES.iter().position(|&at| at == offset) {
            self.exchanges[i] = Some([before, after]);
        }
    }

    pub(super) fn native_barrier(&mut self, offset: usize, barrier: V) {
        if offset == 0x9b0 {
            self.native_barrier = Some(barrier);
        }
    }

    pub(super) fn certificate(self) -> Option<(V, V, [V; 11])> {
        let [a, b, c, d, e] = self.exchanges;
        let [a0, a1] = a?;
        let [b0, b1] = b?;
        let [c0, c1] = c?;
        let [d0, d1] = d?;
        let [e0, e1] = e?;
        Some((
            self.gate?,
            self.counter_ready?,
            [a0, a1, b0, b1, c0, c1, d0, d1, e0, e1, self.native_barrier?],
        ))
    }
}

// Experimental opt-in: if either dimension is not a multiple of 16, the
// entire dispatch produces no writes, including otherwise complete tiles.
// Default translation retains Wave64.
fn enabled() -> bool {
    #[cfg(test)]
    if let Some(on) = TEST_PREFILTER.get() {
        return on;
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_SPLIT_PREFILTER").is_some_and(|s| s == "1"))
}

#[cfg(test)]
thread_local! {
    static TEST_PREFILTER: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

fn identity(native: &NativeProgram) -> bool {
    let cbv = UsedResource {
        kind: ResourceType::Cbv,
        root: RootType::Table,
        table_offset_dwords: 72,
        lower_bound: 23,
        stride: 0,
        dwords: 4,
        user_data: 5,
        slot: 1,
    };
    let root = RootParam {
        root: RootType::Table,
        table_offset_dwords: 72,
        sgprs: 1,
        base_register: 23,
        user_data: 5,
        slot: 1,
    };
    native.stage == HwStage::Cs
        && native.wave64
        && native.vgprs == 37
        && matches!(native.io, StageIo::Cs(cs) if cs.threads == [256, 1, 1] && cs.rsrc2 == 0x898c)
        && native.code.len() == 3072
        && container::hash(&native.code) == DIGEST
        && native
            .used
            .iter()
            .filter(|r| r.kind == ResourceType::Cbv)
            .eq([&cbv])
        && native
            .layout
            .iter()
            .filter(|r| r.user_data == 5 && r.table_offset_dwords == 72)
            .eq([&root])
}

pub(super) fn try_lower(x: &Parts) -> Option<super::Lowered> {
    if !enabled()
        || host_max_wave() != 32
        || LOCAL_WAVES.get()
        || group_sync() == 0
        || !identity(x.native)
        || !x.st.wide.is_empty()
        || crate::spill::build(x.native).ok()? != *x.p
    {
        return None;
    }
    // The emitted uniform gate rejects partial or fully absent tiles. On the
    // fall-through EXEC is full. Keep the saved entry EXEC and remove its
    // now-unreachable zero branch; no later native SCC use sees this update.
    let mut code = x.native.code.clone();
    code[0xe8..0xec].copy_from_slice(&0xbe80_047eu32.to_le_bytes()); // s_mov_b64 s[0:1], exec
    code[0xec..0xf0].copy_from_slice(&0xbf80_0000u32.to_le_bytes()); // s_nop 0
    let report = |e| {
        if std::env::var_os("VICTORY_WAVE_WHY").is_some() {
            eprintln!("WAVE64-PREFILTER did not certify: {e}");
        }
    };
    let p = crate::ir::build(&code, true, x.native.vgprs)
        .map_err(report)
        .ok()?;
    let st = crate::structure::structure(&p).map_err(report).ok()?;
    let io = crate::io::raise(&p, x.native).map_err(report).ok()?;
    let res = crate::resource::track(&p, &x.native.layout, HwStage::Cs, Some(&st))
        .map_err(report)
        .ok()?;
    let parts = Parts {
        p: &p,
        st: &st,
        io: &io,
        res: &res,
        native: x.native,
        inputs: x.inputs.clone(),
        outputs: x.outputs.clone(),
        prims: x.prims.clone(),
        mesh_stride: x.mesh_stride,
    };
    let before = NEEDS.with_borrow(Vec::len);
    let mut bad = HashMap::new();
    loop {
        let result = Lower::new(&parts, &bad, false, true).run();
        NEEDS.with_borrow_mut(|n| n.truncate(before));
        match result {
            Ok(l) => return Some(l),
            Err(Stop::Retry(v, why)) if bad.insert(v, why).is_none() => {}
            Err(Stop::Fail(e)) => {
                report(e);
                return None;
            }
            Err(_) => return None,
        }
    }
}

impl Lower<'_> {
    pub(super) fn prefilter_counter_init(&mut self) -> Result<(), Stop> {
        let slot = self
            .halves
            .ok_or_else(|| unsupported(0, "a prefilter without a full wave exchange"))?;
        let entry = self.s.entry;
        let init = self.s.m.new_block();
        let join = self.s.m.new_block();
        let thread = self.s.op(
            entry,
            "flattenedThreadIdInGroup.i32",
            Attr::ReadNone,
            self.s.t.i32,
            96,
            &[],
        );
        let zero = self.s.m.i32(0);
        let leader = self.s.m.cmp(entry, pred::EQ, thread, zero);
        self.s.m.terminate(entry, Term::CondBr(leader, init, join));
        let slot = self.s.m.i32(slot);
        let lds = self.s.lds();
        let ptr = self.s.m.element(init, lds, slot);
        self.s.m.store(init, ptr, zero);
        self.s.m.terminate(init, Term::Br(join));
        self.s.entry = join;
        Ok(())
    }

    pub(super) fn prefilter_gate(&mut self, inst: &Inst) -> Result<(), &'static str> {
        const BAD: &str = "a prefilter without its width and height CBV load";
        if !matches!(inst.fields, Fields::Smem { offset: 0, .. })
            || inst.name() != "s_buffer_load_dwordx2"
        {
            return Err(BAD);
        }
        let width = self.i32_of(inst.write(Reg::S(0)).ok_or(BAD)?)?;
        let height = self.i32_of(inst.write(Reg::S(1)).ok_or(BAD)?)?;
        let values = [width, height];
        let mut valid = self.s.m.bool(true);
        let size_mask = self.s.m.i32(15);
        let zero = self.s.m.i32(0);
        let shift = self.s.m.i32(4);
        let at = self.cur;
        for (c, size) in values.into_iter().enumerate() {
            let component = self.s.m.i32(c as u32);
            let group = self.s.op(
                self.s.entry,
                "groupId.i32",
                Attr::ReadNone,
                self.s.t.i32,
                94,
                &[component],
            );
            let tail = self.s.m.bin(at, Bin::And, size, size_mask);
            let complete = self.s.m.cmp(at, pred::EQ, tail, zero);
            let tiles = self.s.m.bin(at, Bin::LShr, size, shift);
            let in_bounds = self.s.m.cmp(at, pred::IULT, group, tiles);
            let axis = self.s.m.bin(at, Bin::And, complete, in_bounds);
            valid = self.s.m.bin(at, Bin::And, valid, axis);
        }
        let exit = self.abort_block();
        let active = self.s.m.new_block();
        self.s.m.terminate(at, Term::CondBr(valid, active, exit));
        self.cur = active;
        self.prefilter.as_mut().expect("prefilter").gate = Some(valid);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::TEST_HOST_WAVE;
    use super::{TEST_PREFILTER, identity};
    use crate::emit::{DispatchContract, group_sync, shader::TEST_VIEW_LIMIT, translate};
    use crate::{Container, FourCC, native_programs};

    #[test]
    #[ignore = "requires a local 2da4 capture in VICTORY_PREFILTER_CAPTURE"]
    fn captured_prefilter_certifies_a_wave32_split() {
        let path = std::env::var_os("VICTORY_PREFILTER_CAPTURE").expect("local 2da4 capture");
        let bytes = std::fs::read(path).unwrap();
        let c = Container::parse(&bytes).unwrap();
        let native = native_programs(c.part(FourCC::DXIL).unwrap().data)
            .unwrap()
            .remove(0);
        assert!(identity(&native));
        let mut modified = native.clone();
        modified.code[0] ^= 1;
        assert!(!identity(&modified));
        let mut modified = native;
        modified
            .used
            .iter_mut()
            .find(|r| r.kind == super::ResourceType::Cbv)
            .unwrap()
            .slot = 0;
        assert!(!identity(&modified));

        let wave_bounds = |dxil: &[u8]| {
            let container = Container::parse(dxil).unwrap();
            let psv = container.part(FourCC::PSV0).unwrap().data;
            [20, 24].map(|offset| crate::dxbc::u32_at(psv, offset).unwrap())
        };
        TEST_HOST_WAVE.set(Some(32));
        for limit in [0, 999_999] {
            TEST_VIEW_LIMIT.set(Some(limit));
            TEST_PREFILTER.set(Some(false));
            let baseline = translate(&bytes).unwrap();
            assert_eq!(wave_bounds(&baseline.dxil), [64, 64]);
            assert_eq!(baseline.dispatch_contract, DispatchContract::None);
            TEST_PREFILTER.set(Some(true));
            let shader = translate(&bytes).unwrap();
            if group_sync() == 0 {
                assert_eq!(shader.dispatch_contract, DispatchContract::None);
                assert_eq!(shader.dxil, baseline.dxil);
                continue;
            }
            assert_eq!(wave_bounds(&shader.dxil), [32, 32], "host heap limit {limit}");
            assert_eq!(shader.dispatch_contract, DispatchContract::Prefilter);
            #[cfg(windows)]
            {
                use crate::emit::validator::Validator;
                let mut dxil = shader.dxil;
                eprintln!(
                    "Prefilter Wave32, {} DXIL bytes, heap limit {limit}",
                    dxil.len()
                );
                let mut validator = Validator::load(Validator::default_path().as_deref()).unwrap();
                validator.validate(&mut dxil).unwrap();
            }
        }
        TEST_PREFILTER.set(None);
        TEST_VIEW_LIMIT.set(None);
        TEST_HOST_WAVE.set(None);
    }
}
