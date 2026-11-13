//! wav2vec2 with a CTC head, in Candle (pure Rust, CPU): the sound in 16 kHz samples → seven convolutions
//! (one frame per 20 ms) → a projection → a convolutional position embedding → transformer layers → the
//! log-probability of each letter or phoneme per frame. Reads Hugging Face's `config.json`, `vocab.json` and
//! weights (`model.safetensors`, or `pytorch_model.bin` where a model has no safetensors file).
use super::ear::{Acoustic, CHECK_S, Choice, Units, speed_check_with};
use super::fetch;
use candle_core::{D, DType, Device, Module, Tensor};
use candle_nn::{Conv1d, Conv1dConfig, GroupNorm, LayerNorm, Linear, VarBuilder};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

/// The parts of Hugging Face's `Wav2Vec2Config` the model needs.
#[derive(Clone, Debug, Deserialize, serde::Serialize)]
pub struct Config {
    pub conv_dim: Vec<usize>,
    pub conv_kernel: Vec<usize>,
    pub conv_stride: Vec<usize>,
    #[serde(default)]
    pub conv_bias: bool,
    /// "group": a group norm after the first convolution only; "layer": a layer norm after each.
    #[serde(default = "group")]
    pub feat_extract_norm: String,
    /// Layer norm before each transformer block (large models) instead of after.
    #[serde(default)]
    pub do_stable_layer_norm: bool,
    pub hidden_size: usize,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    pub intermediate_size: usize,
    #[serde(default = "eps")]
    pub layer_norm_eps: f64,
    pub num_conv_pos_embeddings: usize,
    pub num_conv_pos_embedding_groups: usize,
    pub vocab_size: usize,
    #[serde(default)]
    pub pad_token_id: usize,
}

fn group() -> String { "group".into() }
fn eps() -> f64 { 1e-5 }

type R<T> = candle_core::Result<T>;

enum Norm { None, Group(GroupNorm), Layer(LayerNorm) }

struct ConvLayer { conv: Conv1d, norm: Norm }

struct Block {
    q: Linear,
    k: Linear,
    v: Linear,
    out: Linear,
    norm1: LayerNorm,
    ff_in: Linear,
    ff_out: Linear,
    norm2: LayerNorm,
}

/// `Wav2Vec2ForCTC`.
pub struct Wav2Vec2 {
    convs: Vec<ConvLayer>,
    proj_norm: LayerNorm,
    proj: Linear,
    pos_conv: Conv1d,
    pos_kernel: usize,
    enc_norm: LayerNorm,
    blocks: Vec<Block>,
    stable: bool,
    heads: usize,
    head: Linear,
    device: Device,
}

fn layer_norm(size: usize, eps: f64, vb: VarBuilder) -> R<LayerNorm> { candle_nn::layer_norm(size, eps, vb) }

/// The position convolution's weight. Saved with weight norm (`weight_g`, `weight_v`, or the newer
/// `parametrizations.weight.original0/1`) over the kernel axis: w = g · v / ‖v‖.
fn pos_weight(h: usize, groups: usize, k: usize, vb: &VarBuilder) -> R<Tensor> {
    let names = [("weight_g", "weight_v"), ("parametrizations.weight.original0", "parametrizations.weight.original1")];
    for (g, v) in names {
        if vb.contains_tensor(g) && vb.contains_tensor(v) {
            let g = vb.get((1, 1, k), g)?;
            let v = vb.get((h, h / groups, k), v)?;
            let norm = v.sqr()?.sum_keepdim(0)?.sum_keepdim(1)?.sqrt()?;
            return v.broadcast_mul(&g.broadcast_div(&norm)?);
        }
    }
    vb.get((h, h / groups, k), "weight")
}

impl Wav2Vec2 {
    /// Builds the model from `vb` (the root of a `Wav2Vec2ForCTC` checkpoint).
    pub fn new(cfg: &Config, vb: VarBuilder) -> R<Wav2Vec2> {
        let bad = |what: &str| candle_core::Error::Msg(format!("unsupported wav2vec2 config: {what}"));
        let n = cfg.conv_dim.len();
        if n == 0 || cfg.conv_kernel.len() != n || cfg.conv_stride.len() != n { return Err(bad("conv layers")) }
        if cfg.num_attention_heads == 0 || cfg.hidden_size % cfg.num_attention_heads != 0 { return Err(bad("attention heads")) }
        if cfg.num_conv_pos_embedding_groups == 0 || cfg.hidden_size % cfg.num_conv_pos_embedding_groups != 0 { return Err(bad("position groups")) }
        let layer_mode = match cfg.feat_extract_norm.as_str() { "group" => false, "layer" => true, _ => return Err(bad("feat_extract_norm")) };
        let w = vb.pp("wav2vec2");
        let fe = w.pp("feature_extractor").pp("conv_layers");
        let mut convs = Vec::with_capacity(n);
        let mut ch = 1;
        for i in 0..n {
            let l = fe.pp(i);
            let c = Conv1dConfig { stride: cfg.conv_stride[i], ..Default::default() };
            let (out, k) = (cfg.conv_dim[i], cfg.conv_kernel[i]);
            let conv = if cfg.conv_bias { candle_nn::conv1d(ch, out, k, c, l.pp("conv"))? } else { candle_nn::conv1d_no_bias(ch, out, k, c, l.pp("conv"))? };
            let norm = if layer_mode { Norm::Layer(layer_norm(out, 1e-5, l.pp("layer_norm"))?) }
                else if i == 0 { Norm::Group(candle_nn::group_norm(out, out, 1e-5, l.pp("layer_norm"))?) }
                else { Norm::None };
            convs.push(ConvLayer { conv, norm });
            ch = out;
        }
        let h = cfg.hidden_size;
        let fp = w.pp("feature_projection");
        let proj_norm = layer_norm(ch, cfg.layer_norm_eps, fp.pp("layer_norm"))?;
        let proj = candle_nn::linear(ch, h, fp.pp("projection"))?;
        let enc = w.pp("encoder");
        let pc = enc.pp("pos_conv_embed").pp("conv");
        let (k, groups) = (cfg.num_conv_pos_embeddings, cfg.num_conv_pos_embedding_groups);
        let pos_conv = Conv1d::new(pos_weight(h, groups, k, &pc)?, Some(pc.get(h, "bias")?),
            Conv1dConfig { padding: k / 2, groups, ..Default::default() });
        let enc_norm = layer_norm(h, cfg.layer_norm_eps, enc.pp("layer_norm"))?;
        let mut blocks = Vec::with_capacity(cfg.num_hidden_layers);
        for i in 0..cfg.num_hidden_layers {
            let l = enc.pp("layers").pp(i);
            let a = l.pp("attention");
            let ff = l.pp("feed_forward");
            blocks.push(Block {
                q: candle_nn::linear(h, h, a.pp("q_proj"))?,
                k: candle_nn::linear(h, h, a.pp("k_proj"))?,
                v: candle_nn::linear(h, h, a.pp("v_proj"))?,
                out: candle_nn::linear(h, h, a.pp("out_proj"))?,
                norm1: layer_norm(h, cfg.layer_norm_eps, l.pp("layer_norm"))?,
                ff_in: candle_nn::linear(h, cfg.intermediate_size, ff.pp("intermediate_dense"))?,
                ff_out: candle_nn::linear(cfg.intermediate_size, h, ff.pp("output_dense"))?,
                norm2: layer_norm(h, cfg.layer_norm_eps, l.pp("final_layer_norm"))?,
            });
        }
        let head = candle_nn::linear(h, cfg.vocab_size, vb.pp("lm_head"))?;
        Ok(Wav2Vec2 { convs, proj_norm, proj, pos_conv, pos_kernel: k, enc_norm, blocks, stable: cfg.do_stable_layer_norm,
            heads: cfg.num_attention_heads, head, device: vb.device().clone() })
    }

    fn attention(&self, b: &Block, x: &Tensor) -> R<Tensor> {
        let (n, t, h) = x.dims3()?;
        let hd = h / self.heads;
        let split = |y: Tensor| -> R<Tensor> { y.reshape((n, t, self.heads, hd))?.transpose(1, 2)?.contiguous() };
        let q = split((b.q.forward(x)? * (hd as f64).powf(-0.5))?)?;
        let k = split(b.k.forward(x)?)?;
        let v = split(b.v.forward(x)?)?;
        let w = candle_nn::ops::softmax_last_dim(&q.matmul(&k.t()?.contiguous()?)?)?;
        let y = w.matmul(&v)?.transpose(1, 2)?.contiguous()?.reshape((n, t, h))?;
        b.out.forward(&y)
    }

    fn feed_forward(b: &Block, x: &Tensor) -> R<Tensor> { b.ff_out.forward(&b.ff_in.forward(x)?.gelu_erf()?) }

    /// Log-probabilities `(frames, vocab)` for `pcm` (16 kHz, −1…1; at least 400 samples).
    pub fn forward(&self, pcm: &[f32]) -> R<Tensor> {
        // Zero mean, unit variance, as Hugging Face's feature extractor does.
        let n = pcm.len().max(1) as f32;
        let mean = pcm.iter().sum::<f32>() / n;
        let var = pcm.iter().map(|s| (s - mean) * (s - mean)).sum::<f32>() / n;
        let norm: Vec<f32> = pcm.iter().map(|s| (s - mean) / (var + 1e-7).sqrt()).collect();
        let mut x = Tensor::from_vec(norm, (1, 1, pcm.len()), &self.device)?;
        for c in &self.convs {
            x = c.conv.forward(&x)?;
            x = match &c.norm {
                Norm::None => x,
                Norm::Group(g) => g.forward(&x)?,
                Norm::Layer(l) => l.forward(&x.transpose(1, 2)?.contiguous()?)?.transpose(1, 2)?.contiguous()?,
            };
            x = x.gelu_erf()?;
        }
        let x = x.transpose(1, 2)?.contiguous()?;
        let mut x = self.proj.forward(&self.proj_norm.forward(&x)?)?;
        let t = x.dim(1)?;
        let mut p = self.pos_conv.forward(&x.transpose(1, 2)?.contiguous()?)?;
        if self.pos_kernel % 2 == 0 { p = p.narrow(2, 0, t)? }
        x = (x + p.gelu_erf()?.transpose(1, 2)?)?.contiguous()?;
        if !self.stable { x = self.enc_norm.forward(&x)? }
        for b in &self.blocks {
            if self.stable {
                let a = self.attention(b, &b.norm1.forward(&x)?)?;
                x = (x + a)?;
                let f = Self::feed_forward(b, &b.norm2.forward(&x)?)?;
                x = (x + f)?;
            } else {
                let a = self.attention(b, &x)?;
                x = b.norm1.forward(&(x + a)?)?;
                let f = Self::feed_forward(b, &x)?;
                x = b.norm2.forward(&(x + f)?)?;
            }
        }
        if self.stable { x = self.enc_norm.forward(&x)? }
        candle_nn::ops::log_softmax(&self.head.forward(&x)?, D::Minus1)?.squeeze(0)
    }
}

/// A loaded model with its tokens: the ear's `Acoustic`.
pub struct Ctc { net: Wav2Vec2, vocab: Vec<String>, units: Units, blank: usize }

/// The tokens in id order from `vocab.json` (`{"<pad>": 0, …}`); an id with no token (a duplicate key in the
/// file) is `<unk>`.
pub fn read_vocab(json: &str, size: usize) -> Result<Vec<String>, String> {
    let map: HashMap<String, usize> = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut v = vec!["<unk>".to_string(); size.max(map.values().max().map_or(0, |m| m + 1))];
    for (t, i) in map { v[i] = t }
    Ok(v)
}

/// Loads a model folder (`fetch::installed`).
pub fn load(dir: &Path) -> Result<Ctc, String> {
    let read = |f: &str| std::fs::read_to_string(dir.join(f)).map_err(|e| format!("{}: {e}", dir.join(f).display()));
    let cfg: Config = serde_json::from_str(&read("config.json")?).map_err(|e| format!("config.json: {e}"))?;
    let vocab = read_vocab(&read("vocab.json")?, cfg.vocab_size).map_err(|e| format!("vocab.json: {e}"))?;
    let dev = Device::Cpu;
    let st = dir.join("model.safetensors");
    let vb = if st.is_file() {
        // SAFETY: the file is ours (checked against its fingerprint) and not changed while mapped.
        unsafe { VarBuilder::from_mmaped_safetensors(&[st], DType::F32, &dev) }
    } else {
        VarBuilder::from_pth(dir.join("pytorch_model.bin"), DType::F32, &dev)
    }.map_err(|e| e.to_string())?;
    let net = Wav2Vec2::new(&cfg, vb).map_err(|e| e.to_string())?;
    let units = if vocab.iter().any(|t| t == "|") { Units::Letters } else { Units::Phonemes };
    Ok(Ctc { net, vocab, units, blank: cfg.pad_token_id })
}

impl Acoustic for Ctc {
    fn log_probs(&self, pcm: &[f32]) -> Result<Vec<Vec<f32>>, String> {
        if pcm.len() < 400 { return Ok(vec![]) }
        self.net.forward(pcm).and_then(|t| t.to_vec2::<f32>()).map_err(|e| e.to_string())
    }
    fn vocab(&self) -> &[String] { &self.vocab }
    fn units(&self) -> Units { self.units }
    fn blank(&self) -> usize { self.blank }
}

/// How long `model` takes to hear `CHECK_S` seconds of sound (after one warm-up run).
pub fn time(model: &dyn Acoustic) -> Option<Duration> {
    let pcm: Vec<f32> = (0..(CHECK_S * 16000.0) as usize).map(|i| (i as f32 * 0.07).sin() * 0.1).collect();
    model.log_probs(&pcm[..16000]).ok()?;
    let t = Instant::now();
    model.log_probs(&pcm).ok()?;
    Some(t.elapsed())
}

/// Runs each downloaded model on 3 s of sound and picks the biggest one fast enough on this computer.
pub fn speed_check() -> Choice { speed_check_in(&fetch::deps_dir()) }

fn speed_check_in(deps: &Path) -> Choice {
    speed_check_with(|c| {
        let dir = fetch::installed(deps, c.model()?)?;
        time(&load(&dir).ok()?)
    })
}

/// Where the speed check's answer is kept, so it runs once per computer (and again when a model is added).
fn choice_file(deps: &Path) -> std::path::PathBuf { fetch::models_dir(deps).join("choice.json") }

/// The model the speed check picked, loaded; `None` when no model is downloaded or fast enough.
pub fn chosen(deps: &Path) -> Option<Ctc> {
    let have: Vec<Choice> = [Choice::Best, Choice::Fast].into_iter().filter(|c| c.model().and_then(|m| fetch::installed(deps, m)).is_some()).collect();
    if have.is_empty() { return None }
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Kept { choice: Choice, had: Vec<Choice> }
    let kept: Option<Kept> = std::fs::read(choice_file(deps)).ok().and_then(|b| serde_json::from_slice(&b).ok()).filter(|k: &Kept| k.had == have);
    let choice = match kept {
        Some(k) => k.choice,
        None => {
            let c = speed_check_in(deps);
            if let Ok(b) = serde_json::to_vec(&Kept { choice: c, had: have }) { let _ = crate::store::write_atomic(&choice_file(deps), &b); }
            c
        }
    };
    let dir = fetch::installed(deps, choice.model()?)?;
    load(&dir).map_err(|e| eprintln!("auto-follow: couldn't load {}: {e}", dir.display())).ok()
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
