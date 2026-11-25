use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, LlamaModel},
    sampling::LlamaSampler,
};
use std::{num::NonZeroU32, path::Path, sync::Mutex};
use tauri::AppHandle;

const CONTEXT: usize = 4096;
static FORMATTER: Mutex<Option<Formatter>> = Mutex::new(None);

struct Formatter {
    model: LlamaModel,
    backend: LlamaBackend,
}

fn with_model<T>(
    app: &AppHandle,
    f: impl FnOnce(&Formatter) -> Result<T, String>,
) -> Result<T, String> {
    let mut state = FORMATTER.lock().map_err(|_| "Formatter lock poisoned")?;
    if state.is_none() {
        let path = super::with_worker(app, |worker| super::send_request(worker, b'F', b""))?;
        *state = Some(Formatter::load(Path::new(&path))?);
    }
    f(state.as_ref().expect("formatter is initialized"))
}

pub fn prepare(app: &AppHandle) -> Result<(), String> {
    with_model(app, |_| Ok(()))
}

pub fn clean(app: &AppHandle, text: &str) -> Result<Option<String>, String> {
    with_model(app, |formatter| formatter.clean(text))
}

impl Formatter {
    fn load(path: &Path) -> Result<Self, String> {
        let mut backend = LlamaBackend::init().map_err(|err| err.to_string())?;
        backend.void_logs();
        let model = LlamaModel::load_from_file(&backend, path, &LlamaModelParams::default())
            .map_err(|err| err.to_string())?;
        Ok(Self { model, backend })
    }

    fn clean(&self, text: &str) -> Result<Option<String>, String> {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            return Ok(None);
        }
        let prompt = format!("text: {text}\nout:");
        let vocab = self.model.vocab();
        let tokens = vocab.tokenize(prompt.as_bytes(), true, false);
        if tokens.is_empty() || tokens.len() * 2 >= CONTEXT {
            return Ok(None);
        }
        let limit = (tokens.len() + 64).min(CONTEXT - tokens.len());
        let threads =
            std::thread::available_parallelism().map_or(2, |count| count.get().min(6)) as i32;
        let params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new((tokens.len() + limit) as u32))
            .with_n_batch(tokens.len() as u32)
            .with_n_threads(threads)
            .with_n_threads_batch(threads);
        let mut context = self
            .model
            .new_context(&self.backend, params)
            .map_err(|err| err.to_string())?;
        let mut batch = LlamaBatch::new(tokens.len(), 1);
        batch
            .add_sequence(&tokens, 0, false)
            .map_err(|err| err.to_string())?;
        context.decode(&mut batch).map_err(|err| err.to_string())?;

        let mut sampler = LlamaSampler::greedy();
        let mut bytes = Vec::new();
        for position in tokens.len()..tokens.len() + limit {
            let token = sampler.sample(&context, batch.n_tokens() - 1);
            if vocab.is_eog(token) {
                let text = String::from_utf8(bytes).map_err(|err| err.to_string())?;
                let text = text.trim();
                return Ok((!text.is_empty()).then(|| text.to_owned()));
            }
            vocab.token_to_piece_into(token, &mut bytes, false, None);
            batch.clear();
            batch
                .add(token, position as i32, &[0], true)
                .map_err(|err| err.to_string())?;
            context.decode(&mut batch).map_err(|err| err.to_string())?;
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires VTYPE_TEST_GGUF pointing to Spokenly's Q8 fixture"]
    fn spokenly_prompt_cleans_fillers() {
        let path = std::env::var_os("VTYPE_TEST_GGUF").expect("set VTYPE_TEST_GGUF");
        let formatter = Formatter::load(Path::new(&path)).unwrap();
        let text = formatter
            .clean("um so i think we should uh meet on friday at three pm")
            .unwrap()
            .expect("model should finish the transcript");
        assert_eq!(text, "I think we should meet on Friday at 3 pm.");
    }
}
