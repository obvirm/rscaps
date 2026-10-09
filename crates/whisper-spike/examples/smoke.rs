use std::error::Error;
use std::time::Instant;

fn main() -> Result<(), Box<dyn Error>> {
    ort::set_api(ort_candle::api());

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../models/encoder_model.onnx");
    let t0 = Instant::now();
    let mut session = ort::session::Session::builder()?.commit_from_file(path)?;
    println!("session loaded in {:.1}s", t0.elapsed().as_secs_f32());

    for input in session.inputs() {
        println!("input  {} type={:?}", input.name(), input.dtype());
    }
    for output in session.outputs() {
        println!("output {} type={:?}", output.name(), output.dtype());
    }

    // Whisper encoder expects log-mel features: (batch=1, n_mels=80, n_frames=3000).
    // For this spike we feed deterministic pseudo-noise; real feature extraction
    // (STFT -> mel -> log) is a separate step.
    let mel = ndarray::Array3::from_shape_fn((1, 80, 3000), |(_, i, j)| {
        let x = (i as f32 * 0.017 + j as f32 * 0.0013).sin();
        x * 0.5 - 1.6 // roughly log-mel-ish range
    });

    let input_name = session.inputs()[0].name().to_string();
    let value = ort::value::Value::from_array(mel)?;
    let t1 = Instant::now();
    let outputs = session.run(ort::inputs![input_name.as_str() => value])?;
    let elapsed = t1.elapsed();

    let (shape, data) = outputs[0].try_extract_tensor::<f32>()?;
    let mut sum = 0f64;
    let mut absmax = 0f32;
    for &v in data {
        sum += v as f64;
        absmax = absmax.max(v.abs());
    }
    println!(
        "encoder ran in {:.0} ms · output shape={:?} mean={:.4} absmax={:.3}",
        elapsed.as_millis(),
        shape,
        sum / data.len() as f64,
        absmax
    );

    Ok(())
}
