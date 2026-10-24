//! Phonon, a small text to speech model, run through candle.
//!
//! Text is tokenized, a flow-matching language model turns the tokens into Mimi codec latents a
//! frame at a time (12.5 a second), and Mimi's decoder turns each latent into 1920 samples at
//! 24 kHz. Ported from gradium's `ptts` crate (MIT or Apache 2.0), checkpoint
//! `gradium/phonon-31m-en` (Apache 2.0); the numbers are checked against `ptts` stage by stage.

mod conditions;
mod conv;
mod flow;
mod mimi;
mod seanet;
mod transformer;

use candle_core::{DType, Device, Module, Result, Tensor};
use candle_nn::VarBuilder;
use conditions::{ConditionerConfig, FuserConfig};
use flow::{FlowConfig, FlowLm};
use mimi::{Mimi, MimiConfig};
use std::path::Path;
use transformer::{Gelu, State};

pub use mimi::MimiState;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config {
    pub flow_lm: FlowConfig,
    pub mimi: MimiConfig,
    pub lsd_decode_steps: usize,
    pub eos_threshold: f32,
    #[serde(default)]
    pub model_id: Option<serde_json::Value>,
    #[serde(default)]
    pub conditioners: Vec<ConditionerConfig>,
    #[serde(default)]
    pub fuser: Option<FuserConfig>,
    #[serde(default)]
    pub prefix_sinks: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default)]
    pub voices: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub embeddings_ext: Option<Vec<String>>,
}

impl Config {
    pub fn read(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| candle_core::Error::msg(format!("{}: {e}", path.display())))?;
        serde_json::from_str(&text)
            .map_err(|e| candle_core::Error::msg(format!("{}: {e}", path.display())))
    }

    /// Erf for checkpoints that name themselves (audium and phonon), tanh for Pocket TTS.
    fn gelu(&self) -> Gelu {
        if self.model_id.is_some() { Gelu::Erf } else { Gelu::Tanh }
    }
}

/// Checkpoint names in the shape the modules load them: what `ptts::loader::remap_key` does.
fn remap(name: &str) -> Option<String> {
    if name.contains("flow.w_s_t")
        || name.contains("quantizer.vq")
        || name.contains("quantizer.logvar_proj")
    {
        return None;
    }
    Some(
        name.replace(
            "flow_lm.condition_provider.conditioners.speaker_wavs.output_proj.weight",
            "flow_lm.speaker_proj_weight",
        )
        .replace(
            "flow_lm.condition_provider.conditioners.transcript_in_segment.",
            "flow_lm.conditioner.",
        )
        .replace("flow_lm.backbone.", "flow_lm.transformer.")
        .replace("flow_lm.flow.", "flow_lm.flow_net.")
        .replace("mimi.model.", "mimi."),
    )
}

fn load_tensors(path: &Path, dtype: DType, dev: &Device) -> Result<VarBuilder<'static>> {
    let raw = candle_core::safetensors::load(path, &Device::Cpu)?;
    let mut map = std::collections::HashMap::new();
    for (name, t) in raw {
        if let Some(name) = remap(&name) {
            map.insert(name, t.to_dtype(dtype)?.to_device(dev)?);
        }
    }
    Ok(VarBuilder::from_tensors(map, dtype, dev))
}

pub struct Model {
    pub flow: FlowLm,
    pub mimi: Mimi,
    speaker_proj: Option<candle_nn::Linear>,
    conditions: Option<Tensor>,
    pub lsd_steps: usize,
    pub eos_threshold: f32,
    pub sample_rate: usize,
    pub frame_rate: f64,
    pub embeddings_ext: Option<Vec<String>>,
    pub dtype: DType,
}

impl Model {
    /// The checkpoint in `dir`: `config.json` and `model.safetensors`.
    pub fn load(dir: &Path, dtype: DType, dev: &Device) -> Result<Model> {
        let cfg = Config::read(&dir.join("config.json"))?;
        if cfg.prefix_sinks.as_ref().is_some_and(|m| !m.is_empty()) {
            candle_core::bail!("this checkpoint has prefix sinks, which are not supported yet")
        }
        if cfg.voices.as_ref().is_some_and(|v| !v.is_empty()) {
            candle_core::bail!("this checkpoint has baked-in voices, which are not supported yet")
        }
        let vb = load_tensors(&dir.join("model.safetensors"), dtype, dev)?;
        let gelu = cfg.gelu();
        let flow = FlowLm::load(vb.pp("flow_lm"), &cfg.flow_lm, gelu)?;
        let mimi = Mimi::load(vb.pp("mimi"), &cfg.mimi, cfg.mimi.gelu.unwrap_or(gelu))?;
        let speaker_proj = if vb.contains_tensor("flow_lm.speaker_proj_weight") {
            let w =
                vb.get((cfg.flow_lm.d_model, cfg.mimi.dimension), "flow_lm.speaker_proj_weight")?;
            Some(candle_nn::Linear::new(w, None))
        } else {
            None
        };
        let conditions = conditions::defaults(
            vb.pp("flow_lm.condition_provider.conditioners"),
            &cfg.conditioners,
            cfg.fuser.as_ref(),
            cfg.flow_lm.d_model,
        )?;
        Ok(Model {
            flow,
            mimi,
            speaker_proj,
            conditions,
            lsd_steps: cfg.lsd_decode_steps,
            eos_threshold: cfg.eos_threshold,
            sample_rate: cfg.mimi.sample_rate,
            frame_rate: cfg.mimi.frame_rate,
            embeddings_ext: cfg.embeddings_ext,
            dtype,
        })
    }

    /// A voice file: either a ready embedding (`emb`, `[t, dim]`) or Mimi latents
    /// (`speaker_wavs`, `[1, C, t]`) that go through the speaker projection. `[1, t, dim]` out.
    pub fn load_voice(&self, path: &Path, dev: &Device) -> Result<Tensor> {
        let tensors = candle_core::safetensors::load(path, &Device::Cpu)?;
        if let Some(want) = &self.embeddings_ext
            && let Ok((_, meta)) = safetensors_meta(path)
            && let (Some(sig), Some(epoch)) = (meta.get("sig"), meta.get("epoch"))
        {
            let got = format!("{sig}@{epoch}");
            if !want.contains(&got) {
                candle_core::bail!(
                    "{} is a voice for model {got}, not one of {want:?}",
                    path.display()
                )
            }
        }
        let (latents, t) = match (tensors.get("emb"), tensors.get("speaker_wavs")) {
            (Some(e), _) => (false, e.clone()),
            (None, Some(s)) => (true, s.clone()),
            (None, None) => match tensors.values().next() {
                Some(t) => (false, t.clone()),
                None => candle_core::bail!("{} holds no tensors", path.display()),
            },
        };
        let t = match t.rank() {
            2 => t.unsqueeze(0)?,
            3 => t,
            _ => candle_core::bail!("{} holds a voice of shape {:?}", path.display(), t.dims()),
        };
        let t = t.to_dtype(self.dtype)?.to_device(dev)?;
        if !latents {
            return Ok(t);
        }
        let proj = self.speaker_proj.as_ref().ok_or_else(|| {
            candle_core::Error::msg("this checkpoint has no speaker projection for latent voices")
        })?;
        proj.forward(&t.transpose(1, 2)?.contiguous()?)
    }

    pub fn conditions(&self) -> Option<&Tensor> {
        self.conditions.as_ref()
    }

    /// A fresh flow state primed with `voice` (`[1, t, dim]`), ready for a chunk's text.
    pub fn prime(&self, voice: &Tensor) -> Result<State> {
        let mut state = self.flow.transformer.init_state();
        self.flow.prompt(&mut state, voice)?;
        Ok(state)
    }

    /// Feed a chunk's tokens after the voice.
    pub fn prompt_text(&self, state: &mut State, tokens: &[u32]) -> Result<()> {
        let emb = self.flow.embed_tokens(tokens)?;
        self.flow.prompt(state, &emb)
    }

    /// One frame: the latent and whether the model says speech has ended.
    pub fn step(
        &self,
        state: &mut State,
        previous: Option<&Tensor>,
        noise: &Tensor,
    ) -> Result<(Tensor, bool, f32)> {
        let (latent, logit) =
            self.flow.step(state, previous, self.conditions.as_ref(), noise, self.lsd_steps)?;
        Ok((latent, logit > self.eos_threshold, logit))
    }

    /// The samples for one or more latents (`[1, t, ldim]`), streaming through `mimi`.
    pub fn decode(&self, latent: &Tensor, mimi: &mut MimiState) -> Result<Vec<f32>> {
        let z = self.flow.denormalize(latent)?;
        let audio = self.mimi.decode(&z, mimi)?;
        audio.flatten_all()?.to_dtype(DType::F32)?.to_vec1::<f32>()
    }

    pub fn mimi_state(&self) -> Result<MimiState> {
        self.mimi.init_state(1)
    }
}

fn safetensors_meta(
    path: &Path,
) -> std::io::Result<((), std::collections::HashMap<String, String>)> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut len = [0u8; 8];
    f.read_exact(&mut len)?;
    let mut head = vec![0u8; u64::from_le_bytes(len) as usize];
    f.read_exact(&mut head)?;
    let v: serde_json::Value = serde_json::from_slice(&head).map_err(std::io::Error::other)?;
    let meta = v
        .get("__metadata__")
        .and_then(|m| m.as_object())
        .map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
        .unwrap_or_default();
    Ok(((), meta))
}
