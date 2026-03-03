use extism_pdk::*;

#[plugin_fn]
pub fn greet(_input: String) -> FnResult<String> {
    Ok("hello".to_string())
}
