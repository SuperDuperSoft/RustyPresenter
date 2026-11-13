use super::*;
use crate::follow::ear::{Lexicon, Stream};
use candle_nn::VarMap;

fn tiny(layer_norms: bool) -> Config {
    Config {
        conv_dim: vec![8; 7], conv_kernel: vec![10, 3, 3, 3, 3, 2, 2], conv_stride: vec![5, 2, 2, 2, 2, 2, 2], conv_bias: layer_norms,
        feat_extract_norm: if layer_norms { "layer" } else { "group" }.into(), do_stable_layer_norm: layer_norms, hidden_size: 16,
        num_hidden_layers: 2, num_attention_heads: 2, intermediate_size: 32, layer_norm_eps: 1e-5, num_conv_pos_embeddings: 8,
        num_conv_pos_embedding_groups: 2, vocab_size: 32, pad_token_id: 0,
    }
}

/// Random weights for `cfg`, as `Wav2Vec2ForCTC` saves them (the position convolution with weight norm).
fn random_weights(cfg: &Config) -> HashMap<String, Tensor> {
    let dev = Device::Cpu;
    let vm = VarMap::new();
    Wav2Vec2::new(cfg, VarBuilder::from_varmap(&vm, DType::F32, &dev)).unwrap();
    let mut out = HashMap::new();
    for (name, var) in vm.data().lock().unwrap().iter() {
        out.insert(name.clone(), Tensor::randn(0f32, 0.3, var.shape(), &dev).unwrap());
    }
    let pos = "wav2vec2.encoder.pos_conv_embed.conv.weight";
    let v = out.remove(pos).unwrap();
    out.insert(format!("{pos}_v"), v);
    out.insert(format!("{pos}_g"), (Tensor::ones((1, 1, cfg.num_conv_pos_embeddings), DType::F32, &dev).unwrap() * 2.0).unwrap());
    out
}

fn save_tiny(cfg: &Config) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rp-follow-model-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.json"), serde_json::to_vec(cfg).unwrap()).unwrap();
    let vocab: HashMap<String, usize> = crate::follow::ear::tests::letters_vocab().into_iter().enumerate().map(|(i, t)| (t, i)).collect();
    std::fs::write(dir.join("vocab.json"), serde_json::to_vec(&vocab).unwrap()).unwrap();
    candle_core::safetensors::save(&random_weights(cfg), dir.join("model.safetensors")).unwrap();
    dir
}

#[test]
fn model_loads_and_runs_on_silence() {
    for layer_norms in [false, true] {
        let dir = save_tiny(&tiny(layer_norms));
        let m = load(&dir).unwrap();
        assert_eq!((m.units(), m.blank(), m.vocab()[4].as_str()), (Units::Letters, 0, "|"));
        // One second: 49 frames of 20 ms, each a probability over the 32 tokens.
        let rows = m.log_probs(&vec![0.0; 16000]).unwrap();
        assert_eq!(rows.len(), 49);
        for r in &rows {
            assert_eq!(r.len(), 32);
            let total: f32 = r.iter().map(|l| l.exp()).sum();
            assert!((total - 1.0).abs() < 1e-3, "{total}");
        }
        // Real sound too, and too little sound for one frame.
        let tone: Vec<f32> = (0..8000).map(|i| (i as f32 * 0.1).sin() * 0.3).collect();
        assert_eq!(m.log_probs(&tone).unwrap().len(), 24);
        assert!(m.log_probs(&[0.0; 300]).unwrap().is_empty());
        // Streaming silence through it: nothing breaks (random weights may "hear" anything).
        let mut s = Stream::new(Box::new(m), Lexicon::default());
        for i in 0..100 { let _ = s.feed(&[0; 320], i as f64 * 0.02); }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn vocab_with_a_duplicate_key_still_reads() {
    let v = read_vocab(r#"{"<pad>": 0, "a": 1, "ũ": 2, "b": 3, "ũ": 4}"#, 5).unwrap();
    assert_eq!(v, ["<pad>", "a", "<unk>", "b", "ũ"]);
    assert!(load(Path::new("/no/such/model")).is_err());
}

/// The real base model's size, with random weights: how fast it hears 3 s here.
/// `RP_TIMEOUT=900 /root/bin/rpcargo test --offline --lib follow::model::tests::base_model_speed -- --ignored --nocapture`
#[test]
#[ignore]
fn base_model_speed() {
    let cfg: Config = serde_json::from_str(r#"{"conv_dim":[512,512,512,512,512,512,512],"conv_kernel":[10,3,3,3,3,2,2],
        "conv_stride":[5,2,2,2,2,2,2],"conv_bias":false,"feat_extract_norm":"group","do_stable_layer_norm":false,"hidden_size":768,
        "num_hidden_layers":12,"num_attention_heads":12,"intermediate_size":3072,"num_conv_pos_embeddings":128,
        "num_conv_pos_embedding_groups":16,"vocab_size":32}"#).unwrap();
    let vm = VarMap::new();
    let net = Wav2Vec2::new(&cfg, VarBuilder::from_varmap(&vm, DType::F32, &Device::Cpu)).unwrap();
    let m = Ctc { net, vocab: crate::follow::ear::tests::letters_vocab(), units: Units::Letters, blank: 0 };
    let t = time(&m).unwrap();
    eprintln!("base-960h size: 3 s of sound in {:.2} s ({:.2}x real time)", t.as_secs_f64(), t.as_secs_f64() / CHECK_S);
}
