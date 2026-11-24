//! Translated shaders kept on disk (decision 0003): under the configured
//! cache folder, keyed by the container's content and by the translator
//! that made it (the BLAKE3 of its sources, see [`build`]), so a changed
//! translator never serves an old translation and a rebuild that leaves it
//! alone keeps the cache. Entries go through
//! `victory-pipeline`'s content-addressed cache, which writes them
//! atomically; an entry that does not read back is translated again.
//!
//! The entry is our own encoding of [`HostShader`]: a field added to it
//! must be added to `encode` and `decode` too (entries of older builds
//! are ignored anyway).

use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use victory_pipeline::{Cache, Converter};
use victory_shader::emit::{HostShader, StaticSampler, UserData};
use victory_shader::resource::RootIndexedView;
use victory_shader::{HwStage, RootType};

static CACHE: OnceLock<Cache> = OnceLock::new();
static HITS: AtomicU64 = AtomicU64::new(0);
static MISSES: AtomicU64 = AtomicU64::new(0);

/// Keeps translated shaders in `dir` from now on.
pub fn use_shader_cache(dir: PathBuf) {
    match Cache::open(dir) {
        Ok(cache) => {
            let _ = CACHE.set(cache);
        }
        Err(e) => tracing::warn!(error = %e, "no shader cache"),
    }
}

/// Shaders found in the cache and translated since the start.
pub(crate) fn counts() -> (u64, u64) {
    (HITS.load(Ordering::Relaxed), MISSES.load(Ordering::Relaxed))
}

/// What identifies a translation besides the container: the translator that made it and the
/// settings it ran with. The translator is named by the BLAKE3 of the sources of
/// `victory-shader` (`victory_shader::SOURCE_HASH`, made by its `build.rs`, from upstream PR
/// Marvisak/VIctory#254 by wbkepler) and by [`CACHE_SCHEMA_VERSION`] (bumped when this file's
/// encoding changes), not by the executable: a rebuild that leaves the translator alone
/// (another crate changed) serves the translations an earlier build cached, and one that
/// changes it translates again. Run with `VICTORY_SHADER_CACHE_KEY=exe` for the BLAKE3 of the
/// executable instead (any new executable then starts with an empty cache; a build patched
/// outside the translator's sources that changes what it makes should).
fn build() -> &'static [u8; 32] {
    static BUILD: OnceLock<[u8; 32]> = OnceLock::new();
    BUILD.get_or_init(|| {
        let limit = victory_shader::emit::loop_limit();
        if limit != victory_shader::emit::DEFAULT_LOOP_LIMIT {
            tracing::warn!(
                limit,
                "VICTORY_SHADER_LOOP_LIMIT: translated loops stop sooner"
            );
        }
        let translator = if std::env::var_os("VICTORY_SHADER_CACHE_KEY").is_some_and(|v| v == "exe")
        {
            std::env::current_exe().and_then(std::fs::read)
        } else {
            Ok([&CACHE_SCHEMA_VERSION.to_le_bytes()[..], victory_shader::SOURCE_HASH.as_bytes()].concat())
        };
        match translator {
            Ok(translator) => {
                let switches = [
                    "VICTORY_WAVE_WIDE",
                    "VICTORY_WAVE_DEEP_SKIPS",
                    "VICTORY_WAVE_OWN_BITS",
                    "VICTORY_F16_NEAREST",
                ]
                .map(|name| (name, std::env::var_os(name)));
                key(
                    &translator,
                    limit,
                    victory_shader::emit::group_sync(),
                    &switches,
                    victory_shader::emit::shader_opt(),
                )
            }
            Err(e) => {
                // Without it no entry can be trusted: a fresh key per run.
                tracing::warn!(error = %e, "cannot read our executable; shader cache unused");
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                *blake3::hash(&now.as_nanos().to_le_bytes()).as_bytes()
            }
        }
    })
}

/// Serialization schema version of [`encode`] / [`decode`]: bump it whenever [`HostShader`]'s
/// on-disk format changes (the sources hash covers the translator, not this file).
const CACHE_SCHEMA_VERSION: u32 = 1;

/// The key of a translation's context: the translator's identity and the settings that
/// change what it makes (the loop limit, which group barriers wait, the switches that pick
/// the form of a wave64 program, the clean-up passes that ran over the translated code).
fn key(
    translator: &[u8],
    limit: u32,
    group_sync: u8,
    switches: &[(&str, Option<std::ffi::OsString>)],
    shader_opt: u32,
) -> [u8; 32] {
    let mut bytes = translator.to_vec();
    bytes.extend_from_slice(&limit.to_le_bytes());
    bytes.push(group_sync);
    for (name, value) in switches {
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(b'=');
        if let Some(value) = value {
            bytes.extend_from_slice(value.as_encoded_bytes());
        }
        bytes.push(0);
    }
    bytes.extend_from_slice(&shader_opt.to_le_bytes());
    *blake3::hash(&bytes).as_bytes()
}

/// The translation of a console shader container, from the cache when
/// there is one.
pub(crate) fn host_shader(container: &[u8]) -> Result<HostShader, String> {
    let translate = |c: &[u8]| victory_shader::emit::translate(c).map_err(|e| e.to_string());
    match CACHE.get() {
        Some(cache) => cached(cache, build(), container, translate),
        None => translate(container),
    }
}

/// Whether the translation of `container` is in the cache already (nothing is read or
/// translated: the entry's file is looked for).
pub(crate) fn is_cached(container: &[u8]) -> bool {
    CACHE
        .get()
        .is_some_and(|cache| has_entry(cache, build(), container))
}

/// Whether `cache` holds the translation of `container` made in context `build`.
fn has_entry(cache: &Cache, build: &[u8; 32], container: &[u8]) -> bool {
    let converter = Translate {
        translate: |_: &[u8]| Err::<HostShader, String>(String::new()),
        ran: AtomicBool::new(false),
    };
    let input = cache_input(build, container);
    cache
        .path_for(&victory_pipeline::CacheKey::new(&converter, &input))
        .exists()
}

/// The cache entry's converter: the build and the container in, the
/// encoded translation out. `ran` tells a miss from a hit.
struct Translate<F> {
    translate: F,
    ran: AtomicBool,
}

impl<F: Fn(&[u8]) -> Result<HostShader, String> + Send + Sync> Converter for Translate<F> {
    fn id(&self) -> &'static str {
        "shader.dxil"
    }
    fn version(&self) -> u32 {
        // The build fingerprint in the input does the versioning.
        1
    }
    fn convert(&self, input: &[u8]) -> anyhow::Result<Vec<u8>> {
        self.ran.store(true, Ordering::Relaxed);
        let shader = (self.translate)(&input[32..]).map_err(anyhow::Error::msg)?;
        Ok(encode(&shader))
    }
}

/// What a cache entry is made from: the context's key and the container (the entry's name
/// is a hash of it, so [`is_cached`] finds what [`cached`] stored).
fn cache_input(build: &[u8; 32], container: &[u8]) -> Vec<u8> {
    [&build[..], container].concat()
}

fn cached(
    cache: &Cache,
    build: &[u8; 32],
    container: &[u8],
    translate: impl Fn(&[u8]) -> Result<HostShader, String> + Send + Sync,
) -> Result<HostShader, String> {
    let input = cache_input(build, container);
    let converter = Translate {
        translate,
        ran: AtomicBool::new(false),
    };
    for _ in 0..2 {
        let path = cache
            .get_or_convert(&converter, &input)
            .map_err(|e| format!("{e:#}"))?;
        if let Some(shader) = std::fs::read(&path).ok().and_then(|b| decode(&b)) {
            let (counter, what) = if converter.ran.load(Ordering::Relaxed) {
                (&MISSES, "translated and cached")
            } else {
                (&HITS, "from the cache")
            };
            counter.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(path = %path.display(), "shader {what}");
            return Ok(shader);
        }
        tracing::debug!(path = %path.display(), "unreadable shader cache entry; translating again");
        let _ = std::fs::remove_file(&path);
    }
    MISSES.fetch_add(1, Ordering::Relaxed);
    (converter.translate)(container)
}

const MAGIC: &[u8; 4] = b"VSH1";
const STAGES: [HwStage; 8] = [
    HwStage::Ls,
    HwStage::Hs,
    HwStage::Es,
    HwStage::Gs,
    HwStage::Vs,
    HwStage::Ps,
    HwStage::Ms,
    HwStage::Cs,
];

fn root_code(r: RootType) -> u32 {
    match r {
        RootType::Constants => 0,
        RootType::Descriptor => 1,
        RootType::Table => 2,
        RootType::ImmediateSampler => 3,
        RootType::ResourceHeap => 4,
        RootType::SamplerHeap => 5,
        RootType::Other(x) => 6 + x,
    }
}

fn root_type(code: u32) -> RootType {
    match code {
        0 => RootType::Constants,
        1 => RootType::Descriptor,
        2 => RootType::Table,
        3 => RootType::ImmediateSampler,
        4 => RootType::ResourceHeap,
        5 => RootType::SamplerHeap,
        x => RootType::Other(x - 6),
    }
}

/// Little-endian words and length-prefixed lists.
fn encode(s: &HostShader) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    let word = |out: &mut Vec<u8>, v: u32| out.extend(v.to_le_bytes());
    let stage = STAGES.iter().position(|&x| x == s.stage).unwrap_or(0) as u32;
    word(&mut out, stage);
    word(&mut out, s.dxil.len() as u32);
    out.extend(&s.dxil);
    word(&mut out, s.user_data.len() as u32);
    for u in &s.user_data {
        for v in [u.index, u32::from(u.slot), root_code(u.root), u.dword] {
            word(&mut out, v);
        }
    }
    word(&mut out, s.samplers.len() as u32);
    for sampler in &s.samplers {
        for &v in sampler {
            word(&mut out, v);
        }
    }
    word(&mut out, s.static_samplers.len() as u32);
    for t in &s.static_samplers {
        let [u, v, w] = t.address;
        let words = [
            t.filter,
            u,
            v,
            w,
            t.mip_lod_bias.to_bits(),
            t.max_anisotropy,
            t.comparison,
            t.border,
            t.min_lod.to_bits(),
            t.max_lod.to_bits(),
        ];
        for v in words {
            word(&mut out, v);
        }
    }
    for slots in [&s.root_cbvs, &s.root_srvs, &s.root_uavs] {
        word(&mut out, slots.len() as u32);
        out.extend(slots);
    }
    word(&mut out, s.root_indexed_views.len() as u32);
    for view in &s.root_indexed_views {
        for value in [
            u32::from(view.table_slot),
            u32::from(view.constant_slot),
            view.constant_dword,
            view.byte_offset,
            u32::from(view.shift),
        ] {
            word(&mut out, value);
        }
    }
    out
}

fn decode(bytes: &[u8]) -> Option<HostShader> {
    struct Reader<'a>(&'a [u8]);
    impl<'a> Reader<'a> {
        fn take(&mut self, n: usize) -> Option<&'a [u8]> {
            let (head, rest) = self.0.split_at_checked(n)?;
            self.0 = rest;
            Some(head)
        }
        fn word(&mut self) -> Option<u32> {
            Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
        }
        fn list(&mut self) -> Option<Vec<u8>> {
            let n = self.word()? as usize;
            Some(self.take(n)?.to_vec())
        }
    }
    let mut r = Reader(bytes);
    if r.take(4)? != MAGIC {
        return None;
    }
    let stage = *STAGES.get(r.word()? as usize)?;
    let dxil = r.list()?;
    let n = r.word()? as usize;
    let mut user_data = Vec::with_capacity(n.min(256));
    for _ in 0..n {
        user_data.push(UserData {
            index: r.word()?,
            slot: u8::try_from(r.word()?).ok()?,
            root: root_type(r.word()?),
            dword: r.word()?,
        });
    }
    let n = r.word()? as usize;
    let mut samplers = Vec::with_capacity(n.min(256));
    for _ in 0..n {
        samplers.push([r.word()?, r.word()?, r.word()?, r.word()?]);
    }
    let n = r.word()? as usize;
    let mut static_samplers = Vec::with_capacity(n.min(256));
    for _ in 0..n {
        static_samplers.push(StaticSampler {
            filter: r.word()?,
            address: [r.word()?, r.word()?, r.word()?],
            mip_lod_bias: f32::from_bits(r.word()?),
            max_anisotropy: r.word()?,
            comparison: r.word()?,
            border: r.word()?,
            min_lod: f32::from_bits(r.word()?),
            max_lod: f32::from_bits(r.word()?),
        });
    }
    let root_cbvs = r.list()?;
    let root_srvs = r.list()?;
    let root_uavs = r.list()?;
    let n = r.word()? as usize;
    let mut root_indexed_views = Vec::with_capacity(n.min(256));
    for _ in 0..n {
        root_indexed_views.push(RootIndexedView {
            table_slot: u8::try_from(r.word()?).ok()?,
            constant_slot: u8::try_from(r.word()?).ok()?,
            constant_dword: r.word()?,
            byte_offset: r.word()?,
            shift: u8::try_from(r.word()?).ok().filter(|&s| s < 32)?,
        });
    }
    r.0.is_empty().then_some(HostShader {
        stage,
        dxil,
        user_data,
        samplers,
        static_samplers,
        root_cbvs,
        root_srvs,
        root_uavs,
        root_indexed_views,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shader() -> HostShader {
        HostShader {
            stage: HwStage::Ps,
            dxil: b"DXBC and the rest".to_vec(),
            user_data: vec![UserData {
                index: 8,
                slot: 1,
                root: RootType::Table,
                dword: 0,
            }],
            samplers: vec![[0x400, 0xc0_0000, 0x18f0_0000, 0]],
            static_samplers: vec![StaticSampler::from_words([
                0x400,
                0xc0_0000,
                0x18f0_0000,
                0,
            ])],
            root_cbvs: vec![2],
            root_srvs: vec![0, 3],
            root_uavs: vec![1],
            root_indexed_views: vec![RootIndexedView {
                table_slot: 1,
                constant_slot: 2,
                constant_dword: 3,
                byte_offset: 32,
                shift: 5,
            }],
        }
    }

    #[test]
    fn translations_come_back_from_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(dir.path()).unwrap();
        let build = [7u8; 32];
        let container = b"a console container".as_slice();
        let runs = AtomicU64::new(0);
        let translate = |_: &[u8]| {
            runs.fetch_add(1, Ordering::Relaxed);
            Ok(shader())
        };
        assert_eq!(cached(&cache, &build, container, translate), Ok(shader()));
        assert_eq!(cached(&cache, &build, container, translate), Ok(shader()));
        assert_eq!(runs.load(Ordering::Relaxed), 1, "the second came from disk");

        // Another build translates again.
        assert_eq!(cached(&cache, &[8; 32], container, translate), Ok(shader()));
        assert_eq!(runs.load(Ordering::Relaxed), 2);

        // A damaged entry is a miss, translated and stored again.
        for e in walk(dir.path()) {
            std::fs::write(e, b"VSH1 broken").unwrap();
        }
        assert_eq!(cached(&cache, &build, container, translate), Ok(shader()));
        assert_eq!(runs.load(Ordering::Relaxed), 3);
        assert_eq!(cached(&cache, &build, container, translate), Ok(shader()));
        assert_eq!(runs.load(Ordering::Relaxed), 3);

        // Failures are not stored.
        let fail = |_: &[u8]| Err::<HostShader, _>("unsupported".to_owned());
        assert!(cached(&cache, &build, b"other", fail).is_err());
    }

    #[test]
    fn an_entry_is_found_for_the_translator_that_stored_it_only() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(dir.path()).unwrap();
        let (container, mine, other) = (b"a container".as_slice(), [1; 32], [2; 32]);
        assert!(!has_entry(&cache, &mine, container));
        let translate = |_: &[u8]| Ok(shader());
        cached(&cache, &mine, container, translate).unwrap();
        assert!(has_entry(&cache, &mine, container));
        // another translator's key, or another container, finds nothing
        assert!(!has_entry(&cache, &other, container));
        assert!(!has_entry(&cache, &mine, b"another container"));
    }

    /// The digest `victory-shader`'s `build.rs` makes, from its sources as they are now.
    fn digest_of_translator_sources() -> String {
        fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<(String, PathBuf)>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, root, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let rel = path
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace(std::path::MAIN_SEPARATOR, "/");
                    out.push((rel, path));
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../victory-shader/src");
        let mut files = Vec::new();
        walk(&root, &root, &mut files);
        files.sort_by(|a, b| a.0.cmp(&b.0));
        let mut hasher = blake3::Hasher::new();
        for (rel, path) in files {
            let contents = std::fs::read(path).unwrap();
            hasher.update(rel.as_bytes());
            hasher.update(&[0]);
            hasher.update(&(contents.len() as u64).to_le_bytes());
            hasher.update(&contents);
        }
        hasher.finalize().to_hex().to_string()
    }

    #[test]
    fn the_source_hash_is_the_translators_sources_digest() {
        // so a changed translator source changes the key (the build script reruns when a file
        // under `victory-shader/src` changes) and nothing else does
        assert_eq!(victory_shader::SOURCE_HASH, digest_of_translator_sources());
    }

    #[test]
    fn the_key_follows_the_translator_and_its_settings_and_nothing_else() {
        let k = |translator: &[u8], limit: u32, wide: Option<&str>| {
            key(
                translator,
                limit,
                1,
                &[("VICTORY_WAVE_WIDE", wide.map(Into::into))],
                3,
            )
        };
        let base = k(b"fingerprint A", 4096, None);
        assert_eq!(
            base,
            k(b"fingerprint A", 4096, None),
            "the same inputs, the same key"
        );
        // another translator: another key
        assert_ne!(base, k(b"fingerprint B", 4096, None));
        // its settings: the loop limit, a switch that picks the wave64 form
        assert_ne!(base, k(b"fingerprint A", 16, None));
        assert_ne!(base, k(b"fingerprint A", 4096, Some("1")));
        // (a switch set to nothing reads as unset in the translator, so it keys alike)
        assert_eq!(k(b"x", 1, None), k(b"x", 1, Some("")));
    }

    #[test]
    fn the_translator_of_this_build_is_named_by_a_fingerprint_not_the_executable() {
        // 64 hex digits of the BLAKE3 of the translator's sources (victory-shader's build.rs)
        assert_eq!(victory_shader::SOURCE_HASH.len(), 64);
        assert!(victory_shader::SOURCE_HASH.bytes().all(|b| b.is_ascii_hexdigit()));
        // and, unless the executable is asked for, the key is made of it
        if std::env::var_os("VICTORY_SHADER_CACHE_KEY").is_none() {
            let expected = key(
                &[&CACHE_SCHEMA_VERSION.to_le_bytes()[..], victory_shader::SOURCE_HASH.as_bytes()].concat(),
                victory_shader::emit::loop_limit(),
                victory_shader::emit::group_sync(),
                &[
                    "VICTORY_WAVE_WIDE",
                    "VICTORY_WAVE_DEEP_SKIPS",
                    "VICTORY_WAVE_OWN_BITS",
                    "VICTORY_F16_NEAREST",
                ]
                .map(|name| (name, std::env::var_os(name))),
                victory_shader::emit::shader_opt(),
            );
            assert_eq!(build(), &expected);
        }
    }

    fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            if e.path().is_dir() {
                files.extend(walk(&e.path()));
            } else {
                files.push(e.path());
            }
        }
        files
    }

    #[test]
    fn entries_round_trip_and_reject_damage() {
        let bytes = encode(&shader());
        assert_eq!(decode(&bytes), Some(shader()));
        assert_eq!(decode(&bytes[..bytes.len() - 1]), None);
        assert_eq!(decode(&[bytes.as_slice(), &[0]].concat()), None);
        assert_eq!(decode(b"VSH0"), None);
    }
}
