use alloc::string::String;

use wasm_bindgen::{JsCast, JsValue};

pub fn value_to_string(value: &JsValue) -> String {
	js_sys::Object::<JsValue>::unchecked_from_js_ref(value).to_js_string().into()
}

/// Like value_to_string but appends the JS `stack` property when present —
/// needed to locate TypeErrors thrown deep inside onnxruntime-web.
pub fn error_with_stack(value: &JsValue) -> String {
	let base = value_to_string(value);
	if let Ok(stack) = js_sys::Reflect::get(value, &JsValue::from_str("stack")) {
		if let Some(s) = stack.as_string() {
			if !s.is_empty() {
				return alloc::format!("{base}\n{s}");
			}
		}
	}
	base
}

pub fn num_elements(dims: &[i32]) -> usize {
	let mut size = 1usize;
	for dim in dims {
		if *dim < 0 {
			return 0;
		}
		size *= *dim as usize;
	}
	size
}
